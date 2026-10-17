//! Durable local recording.
//!
//! While capturing, processed 16 kHz mono PCM is appended to a raw staging file by a dedicated
//! writer thread (the DSP thread only hands over buffers and never touches the disk). The file
//! has no header, so everything written before a crash is recoverable. After capture stops the
//! staging file is encoded to Ogg Opus, the result is decoded back and checked, and only then is
//! the staging file deleted. If encoding or verification fails, the staging file stays.

use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender, TrySendError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ogg::writing::PacketWriteEndInfo;

use super::capture::FrameSink;

pub const SAMPLE_RATE: u32 = 16_000;
/// 20 ms Opus frames at 16 kHz.
const OPUS_FRAME: usize = 320;
/// Ogg Opus granule positions always count 48 kHz samples (RFC 7845).
const GRANULE_PER_SAMPLE: u64 = 3;
const BITRATE: i32 = 24_000;
/// Writer queue: 100 ms chunks, so 600 is a minute of slack if the disk stalls.
const WRITER_QUEUE: usize = 600;
const CHUNK: usize = 1_600;

#[derive(Debug)]
pub enum RecordingError {
  Io(String),
  Codec(String),
  Verify(String),
  Empty,
}

impl std::fmt::Display for RecordingError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      RecordingError::Io(e) => write!(f, "file error: {e}"),
      RecordingError::Codec(e) => write!(f, "encoder error: {e}"),
      RecordingError::Verify(e) => write!(f, "verification failed: {e}"),
      RecordingError::Empty => write!(f, "no audio"),
    }
  }
}

fn io(e: std::io::Error) -> RecordingError {
  RecordingError::Io(e.kind().to_string())
}

/// Summary of a closed staging file.
#[derive(Debug, Clone, Default)]
pub struct StagingInfo {
  pub samples: u64,
  pub dropped_chunks: u64,
  pub write_errors: u64,
}

impl StagingInfo {
  pub fn duration_ms(&self) -> i64 {
    (self.samples * 1000 / SAMPLE_RATE as u64) as i64
  }
}

enum Msg {
  Pcm(Vec<i16>),
  Close,
}

/// Appends PCM to a staging file on its own thread.
pub struct RecordingWriter {
  path: PathBuf,
  tx: Option<SyncSender<Msg>>,
  thread: Option<JoinHandle<StagingInfo>>,
}

impl RecordingWriter {
  /// Creates the staging file immediately so a failure is known before capture starts.
  pub fn create(path: &Path) -> Result<RecordingWriter, RecordingError> {
    if let Some(dir) = path.parent() {
      std::fs::create_dir_all(dir).map_err(io)?;
    }
    let file = OpenOptions::new().create(true).append(true).open(path).map_err(io)?;
    let (tx, rx) = sync_channel::<Msg>(WRITER_QUEUE);
    let thread = std::thread::Builder::new()
      .name("audio-recorder".into())
      .spawn(move || {
        let mut out = BufWriter::with_capacity(64 * 1024, file);
        let mut info = StagingInfo::default();
        let mut last_flush = Instant::now();
        let mut last_sync = Instant::now();
        let mut bytes = Vec::with_capacity(CHUNK * 2);
        loop {
          match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Msg::Pcm(pcm)) => {
              bytes.clear();
              for s in &pcm {
                bytes.extend_from_slice(&s.to_le_bytes());
              }
              if out.write_all(&bytes).is_err() {
                info.write_errors += 1;
              } else {
                info.samples += pcm.len() as u64;
              }
            }
            Ok(Msg::Close) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
          }
          // Bounded crash window: data reaches the OS within ~1 s and the disk within ~5 s,
          // whether or not more audio arrives.
          if last_flush.elapsed() >= Duration::from_secs(1) {
            let _ = out.flush();
            last_flush = Instant::now();
          }
          if last_sync.elapsed() >= Duration::from_secs(5) {
            let _ = out.get_ref().sync_data();
            last_sync = Instant::now();
          }
        }
        let _ = out.flush();
        let _ = out.get_ref().sync_all();
        info
      })
      .map_err(io)?;
    Ok(RecordingWriter {
      path: path.to_path_buf(),
      tx: Some(tx),
      thread: Some(thread),
    })
  }

  pub fn path(&self) -> &Path {
    &self.path
  }

  /// A sink for the capture DSP thread. Never blocks: if the writer queue is full the chunk is
  /// kept and retried with the next one (up to 5 minutes of audio) before anything is dropped.
  /// Dropping the sink hands over the final partial chunk.
  pub fn sink(&self) -> FrameSink {
    let mut state = SinkState {
      tx: self.tx.clone().expect("writer open"),
      batch: Vec::with_capacity(CHUNK),
      backlog: Default::default(),
    };
    Box::new(move |frame: &[i16]| state.push(frame))
  }

  /// Flushes everything (sinks must be dropped first) and returns what was written.
  pub fn close(mut self) -> StagingInfo {
    self.finish()
  }

  fn finish(&mut self) -> StagingInfo {
    if let Some(tx) = self.tx.take() {
      let _ = tx.send(Msg::Close);
    }
    self.thread.take().and_then(|t| t.join().ok()).unwrap_or_default()
  }
}

impl Drop for RecordingWriter {
  fn drop(&mut self) {
    if self.thread.is_some() {
      self.finish();
    }
  }
}

struct SinkState {
  tx: SyncSender<Msg>,
  batch: Vec<i16>,
  backlog: std::collections::VecDeque<Vec<i16>>,
}

impl SinkState {
  fn push(&mut self, frame: &[i16]) {
    self.batch.extend_from_slice(frame);
    if self.batch.len() >= CHUNK {
      self
        .backlog
        .push_back(std::mem::replace(&mut self.batch, Vec::with_capacity(CHUNK)));
      self.drain(false);
    }
  }

  fn drain(&mut self, blocking: bool) {
    while let Some(chunk) = self.backlog.pop_front() {
      let sent = if blocking {
        self.tx.send(Msg::Pcm(chunk)).map_err(|_| ())
      } else {
        match self.tx.try_send(Msg::Pcm(chunk)) {
          Ok(()) => Ok(()),
          Err(TrySendError::Full(Msg::Pcm(chunk))) => {
            self.backlog.push_front(chunk);
            return;
          }
          Err(_) => Err(()),
        }
      };
      if sent.is_err() {
        return;
      }
    }
    while self.backlog.len() > 3_000 {
      self.backlog.pop_front();
      tracing::error!("recording backlog overflow; oldest audio chunk dropped");
    }
  }
}

impl Drop for SinkState {
  fn drop(&mut self) {
    // Capture has stopped: blocking is fine now, and nothing may be left behind.
    if !self.batch.is_empty() {
      self.backlog.push_back(std::mem::take(&mut self.batch));
    }
    self.drain(true);
  }
}

pub fn read_staging(path: &Path) -> Result<Vec<i16>, RecordingError> {
  let mut bytes = Vec::new();
  File::open(path).map_err(io)?.read_to_end(&mut bytes).map_err(io)?;
  Ok(
    bytes
      .chunks_exact(2)
      .map(|b| i16::from_le_bytes([b[0], b[1]]))
      .collect(),
  )
}

#[derive(Debug, Clone)]
pub struct EncodedInfo {
  pub path: PathBuf,
  pub bytes: u64,
  pub samples: u64,
}

impl EncodedInfo {
  pub fn duration_ms(&self) -> i64 {
    (self.samples * 1000 / SAMPLE_RATE as u64) as i64
  }
}

fn opus_head(pre_skip: u16) -> Vec<u8> {
  let mut h = Vec::with_capacity(19);
  h.extend_from_slice(b"OpusHead");
  h.push(1); // version
  h.push(1); // channels
  h.extend_from_slice(&pre_skip.to_le_bytes());
  h.extend_from_slice(&SAMPLE_RATE.to_le_bytes()); // original input rate
  h.extend_from_slice(&0i16.to_le_bytes()); // output gain
  h.push(0); // mapping family
  h
}

fn opus_tags() -> Vec<u8> {
  let vendor = b"Dictation HUD";
  let mut t = Vec::new();
  t.extend_from_slice(b"OpusTags");
  t.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
  t.extend_from_slice(vendor);
  t.extend_from_slice(&0u32.to_le_bytes());
  t
}

/// Encodes 16 kHz mono PCM to an Ogg Opus file (RFC 7845).
pub fn encode_ogg_opus(pcm: &[i16], out: &Path) -> Result<EncodedInfo, RecordingError> {
  if pcm.is_empty() {
    return Err(RecordingError::Empty);
  }
  let codec = |e: opus::Error| RecordingError::Codec(e.description().to_string());
  let mut enc = opus::Encoder::new(SAMPLE_RATE, opus::Channels::Mono, opus::Application::Voip).map_err(codec)?;
  enc.set_bitrate(opus::Bitrate::Bits(BITRATE)).map_err(codec)?;
  let lookahead = enc.get_lookahead().map_err(codec)? as u64; // at 16 kHz
  let pre_skip = (lookahead * GRANULE_PER_SAMPLE) as u16;

  if let Some(dir) = out.parent() {
    std::fs::create_dir_all(dir).map_err(io)?;
  }
  let tmp = out.with_extension("opus.part");
  let file = File::create(&tmp).map_err(io)?;
  let mut w = ogg::writing::PacketWriter::new(BufWriter::new(file));
  let serial: u32 = 0x4448_5544; // "DHUD"
  w.write_packet(opus_head(pre_skip), serial, PacketWriteEndInfo::EndPage, 0)
    .map_err(io)?;
  w.write_packet(opus_tags(), serial, PacketWriteEndInfo::EndPage, 0)
    .map_err(io)?;

  // Pad the tail with silence so the encoder's lookahead flushes the last real samples.
  let total = pcm.len() as u64;
  let padded_len = (pcm.len() + lookahead as usize).div_ceil(OPUS_FRAME) * OPUS_FRAME;
  let mut padded = pcm.to_vec();
  padded.resize(padded_len, 0);
  let frames = padded.chunks(OPUS_FRAME).count();
  let mut packet = vec![0u8; 4000];
  for (i, frame) in padded.chunks(OPUS_FRAME).enumerate() {
    let n = enc.encode(frame, &mut packet).map_err(codec)?;
    let last = i + 1 == frames;
    let granule = if last {
      // Final granule = pre-skip + real sample count: trims the padding on decode.
      pre_skip as u64 + total * GRANULE_PER_SAMPLE
    } else {
      pre_skip as u64 + ((i + 1) * OPUS_FRAME) as u64 * GRANULE_PER_SAMPLE
    };
    let end = if last {
      PacketWriteEndInfo::EndStream
    } else {
      PacketWriteEndInfo::NormalPacket
    };
    w.write_packet(packet[..n].to_vec(), serial, end, granule).map_err(io)?;
  }
  let mut inner = w.into_inner();
  inner.flush().map_err(io)?;
  inner.get_ref().sync_all().map_err(io)?;
  drop(inner);
  std::fs::rename(&tmp, out).map_err(io)?;
  let bytes = std::fs::metadata(out).map_err(io)?.len();
  Ok(EncodedInfo {
    path: out.to_path_buf(),
    bytes,
    samples: total,
  })
}

/// Decodes an Ogg Opus file written by [`encode_ogg_opus`] back to 16 kHz mono PCM.
pub fn decode_ogg_opus(path: &Path) -> Result<Vec<i16>, RecordingError> {
  let codec = |e: opus::Error| RecordingError::Codec(e.description().to_string());
  let mut r = ogg::reading::PacketReader::new(BufReader::new(File::open(path).map_err(io)?));
  let head = r
    .read_packet()
    .map_err(|e| RecordingError::Codec(e.to_string()))?
    .ok_or(RecordingError::Empty)?;
  if head.data.len() < 19 || &head.data[..8] != b"OpusHead" {
    return Err(RecordingError::Codec("missing OpusHead".into()));
  }
  let pre_skip = u16::from_le_bytes([head.data[10], head.data[11]]) as u64;
  let _tags = r.read_packet().map_err(|e| RecordingError::Codec(e.to_string()))?;
  let mut dec = opus::Decoder::new(SAMPLE_RATE, opus::Channels::Mono).map_err(codec)?;
  let mut out = Vec::new();
  let mut buf = vec![0i16; 5_760];
  let mut final_granule = 0u64;
  while let Some(p) = r.read_packet().map_err(|e| RecordingError::Codec(e.to_string()))? {
    let n = dec.decode(&p.data, &mut buf, false).map_err(codec)?;
    out.extend_from_slice(&buf[..n]);
    if p.last_in_stream() {
      final_granule = p.absgp_page();
    }
  }
  let skip = (pre_skip / GRANULE_PER_SAMPLE) as usize;
  let keep = if final_granule > pre_skip {
    ((final_granule - pre_skip) / GRANULE_PER_SAMPLE) as usize
  } else {
    out.len()
  };
  let start = skip.min(out.len());
  let end = (start + keep).min(out.len());
  Ok(out[start..end].to_vec())
}

/// Encodes a staging file, verifies the result by decoding it, then deletes the staging file.
/// On any failure the staging file is kept for a later retry.
pub fn finalize_staging(staging: &Path, archive: &Path) -> Result<EncodedInfo, RecordingError> {
  let pcm = read_staging(staging)?;
  if pcm.is_empty() {
    let _ = std::fs::remove_file(staging);
    return Err(RecordingError::Empty);
  }
  let info = encode_ogg_opus(&pcm, archive)?;
  let decoded = decode_ogg_opus(archive)?;
  let diff = (decoded.len() as i64 - pcm.len() as i64).abs();
  if diff > OPUS_FRAME as i64 {
    let _ = std::fs::remove_file(archive);
    return Err(RecordingError::Verify(format!(
      "length {} vs {}",
      decoded.len(),
      pcm.len()
    )));
  }
  std::fs::remove_file(staging).map_err(io)?;
  Ok(info)
}

/// Writes 16 kHz mono PCM as a standard 16-bit WAV (for "Extract audio").
pub fn write_wav(pcm: &[i16], out: &Path) -> Result<(), RecordingError> {
  let spec = hound::WavSpec {
    channels: 1,
    sample_rate: SAMPLE_RATE,
    bits_per_sample: 16,
    sample_format: hound::SampleFormat::Int,
  };
  let mut w = hound::WavWriter::create(out, spec).map_err(|e| RecordingError::Io(e.to_string()))?;
  for s in pcm {
    w.write_sample(*s).map_err(|e| RecordingError::Io(e.to_string()))?;
  }
  w.finalize().map_err(|e| RecordingError::Io(e.to_string()))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("dh-rec-{}-{name}", uuid::Uuid::new_v4()))
  }

  fn tone(secs: f32) -> Vec<i16> {
    let n = (SAMPLE_RATE as f32 * secs) as usize;
    (0..n)
      .map(|i| ((i as f32 / SAMPLE_RATE as f32 * 440.0 * std::f32::consts::TAU).sin() * 8000.0) as i16)
      .collect()
  }

  #[test]
  fn writer_persists_everything_after_close() {
    let path = tmp("a.pcm");
    let w = RecordingWriter::create(&path).unwrap();
    let mut sink = w.sink();
    let pcm = tone(2.0);
    for f in pcm.chunks(160) {
      sink(f);
    }
    sink(&[7, 7, 7]); // partial tail, flushed when the sink drops
    drop(sink);
    let info = w.close();
    assert_eq!(info.samples, pcm.len() as u64 + 3);
    let staged = read_staging(&path).unwrap();
    assert_eq!(&staged[..pcm.len()], &pcm[..]);
    assert_eq!(&staged[pcm.len()..], &[7, 7, 7]);
    let _ = std::fs::remove_file(path);
  }

  #[test]
  fn staged_audio_survives_without_close() {
    // Simulates a crash: data written so far is on disk even though close never ran.
    let path = tmp("crash.pcm");
    {
      let w = RecordingWriter::create(&path).unwrap();
      let mut sink = w.sink();
      for f in tone(1.0).chunks(160) {
        sink(f);
      }
      drop(sink);
      std::mem::forget(w); // no close, no drop flush beyond the OS buffers
    }
    std::thread::sleep(Duration::from_millis(1_300));
    let on_disk = read_staging(&path).unwrap();
    assert!(
      on_disk.len() >= 14_400,
      "most of the second is on disk: {}",
      on_disk.len()
    );
    let _ = std::fs::remove_file(path);
  }

  #[test]
  fn opus_round_trip_keeps_length_and_signal() {
    let out = tmp("a.opus");
    let pcm = tone(3.3);
    let info = encode_ogg_opus(&pcm, &out).unwrap();
    assert!(
      info.bytes > 0 && info.bytes < (pcm.len() * 2) as u64 / 4,
      "compressed: {}",
      info.bytes
    );
    let dec = decode_ogg_opus(&out).unwrap();
    assert!(
      (dec.len() as i64 - pcm.len() as i64).abs() <= OPUS_FRAME as i64,
      "{} vs {}",
      dec.len(),
      pcm.len()
    );
    let energy = |v: &[i16]| v.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / v.len() as f64;
    let ratio = energy(&dec) / energy(&pcm);
    assert!((0.6..1.4).contains(&ratio), "energy ratio {ratio}");
    let _ = std::fs::remove_file(out);
  }

  #[test]
  fn finalize_deletes_staging_only_after_verified_encode() {
    let staging = tmp("s.pcm");
    let archive = tmp("s.opus");
    std::fs::write(
      &staging,
      tone(1.0).iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<u8>>(),
    )
    .unwrap();
    let info = finalize_staging(&staging, &archive).unwrap();
    assert!(!staging.exists());
    assert!(archive.exists());
    assert_eq!(info.samples, 16_000);
    let _ = std::fs::remove_file(archive);
  }

  #[test]
  fn failed_encode_keeps_staging() {
    let staging = tmp("keep.pcm");
    std::fs::write(
      &staging,
      tone(0.5).iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<u8>>(),
    )
    .unwrap();
    // Archive path inside a file (not a directory): creating it fails.
    let blocker = tmp("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let archive = blocker.join("sub").join("x.opus");
    assert!(finalize_staging(&staging, &archive).is_err());
    assert!(staging.exists(), "raw audio must survive a failed encode");
    let _ = std::fs::remove_file(staging);
    let _ = std::fs::remove_file(blocker);
  }

  #[test]
  fn unwritable_staging_path_errors_up_front() {
    let blocker = tmp("blocker2");
    std::fs::write(&blocker, b"x").unwrap();
    assert!(RecordingWriter::create(&blocker.join("x.pcm")).is_err());
    let _ = std::fs::remove_file(blocker);
  }

  #[test]
  fn wav_export_is_standard_pcm() {
    let out = tmp("x.wav");
    write_wav(&tone(0.25), &out).unwrap();
    let r = hound::WavReader::open(&out).unwrap();
    assert_eq!(r.spec().sample_rate, 16_000);
    assert_eq!(r.spec().channels, 1);
    assert_eq!(r.len(), 4_000);
    let _ = std::fs::remove_file(out);
  }
}
