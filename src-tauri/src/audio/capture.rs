//! Native microphone capture (WASAPI via cpal).
//!
//! Thread layout:
//! - device callbacks: downmix to mono and push into lock-free rings. No allocation, no IO,
//!   no locks.
//! - device thread: owns the cpal streams (not `Send`) until stop.
//! - DSP thread: resample to 16 kHz, echo cancellation / noise suppression, level metering,
//!   then fan-out to sinks. A slow consumer never blocks capture: provider audio is sent with
//!   `try_send` into a large bounded channel and drops are counted.

use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};
use tokio::sync::mpsc;

use super::dsp::{to_i16, Processor, Resample, FRAME};

/// Reads a 16 kHz mono 16-bit WAV for the synthetic source.
pub fn load_wav_16k(path: &std::path::Path) -> Result<Vec<i16>, String> {
  let mut r = hound::WavReader::open(path).map_err(|e| e.to_string())?;
  let spec = r.spec();
  if spec.sample_rate != 16_000 || spec.channels != 1 || spec.bits_per_sample != 16 {
    return Err(format!(
      "expected 16 kHz mono 16-bit WAV, got {} Hz {} ch {} bit",
      spec.sample_rate, spec.channels, spec.bits_per_sample
    ));
  }
  r.samples::<i16>()
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}
use super::levels::LevelMeter;
use crate::providers::AudioMsg;

/// Samples handed to the provider per message (100 ms).
const PROVIDER_BATCH: usize = 1_600;
/// Ring capacity per stream: 2 s at 48 kHz.
const RING: usize = 96_000;
/// Keep the echo reference close to realtime; drop backlog beyond 200 ms.
const MAX_REF_BACKLOG: usize = 20 * FRAME;

#[derive(Debug, Clone, PartialEq)]
pub enum AudioError {
  NoDevice,
  Open(String),
}

impl std::fmt::Display for AudioError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      AudioError::NoDevice => write!(f, "no microphone found"),
      AudioError::Open(e) => write!(f, "could not open microphone: {e}"),
    }
  }
}

#[derive(Debug, Clone)]
pub struct CaptureConfig {
  pub device_id: Option<String>,
  pub echo_cancellation: bool,
  pub noise_suppression: bool,
  /// Test/diagnostic source: 16 kHz mono samples played back in real time instead of the
  /// microphone (followed by silence until stop). Never set in normal use.
  pub synthetic: Option<Arc<Vec<i16>>>,
}

/// Receives every processed 10 ms frame (16 kHz mono i16) on the DSP thread. Must be fast.
pub type FrameSink = Box<dyn FnMut(&[i16]) + Send>;

pub struct CaptureOutputs {
  pub provider: Option<mpsc::Sender<AudioMsg>>,
  pub levels: Arc<LevelMeter>,
  pub frame_sinks: Vec<FrameSink>,
  /// Called once if the device disappears mid-capture.
  pub on_device_lost: Box<dyn Fn() + Send + Sync>,
}

#[derive(Debug, Clone, Default)]
pub struct CaptureStats {
  pub frames: u64,
  pub ring_overruns: u64,
  pub provider_drops: u64,
  pub device_name: String,
  pub device_rate: u32,
  pub echo_reference: bool,
}

impl CaptureStats {
  /// Captured audio length (10 ms frames).
  pub fn duration_ms(&self) -> i64 {
    self.frames as i64 * 10
  }
}

pub struct Capture {
  stop: Arc<AtomicBool>,
  stop_device: Option<std_mpsc::Sender<()>>,
  device_thread: Option<JoinHandle<()>>,
  dsp_thread: Option<JoinHandle<CaptureStats>>,
  pub device_name: String,
  pub opened_in: Duration,
}

struct Opened {
  mic_rate: u32,
  ref_rate: Option<u32>,
  name: String,
}

fn find_device(host: &cpal::Host, id: Option<&str>) -> Result<cpal::Device, AudioError> {
  if let Some(id) = id {
    if let Ok(did) = cpal::DeviceId::from_str(id) {
      if let Some(d) = host.device_by_id(&did) {
        return Ok(d);
      }
    }
    tracing::warn!("selected microphone not found; using the system default");
  }
  host.default_input_device().ok_or(AudioError::NoDevice)
}

fn build_mono_stream<T>(
  device: &cpal::Device,
  config: &cpal::StreamConfig,
  mut producer: rtrb::Producer<f32>,
  overruns: Arc<AtomicU64>,
  lost: Arc<AtomicBool>,
) -> Result<cpal::Stream, AudioError>
where
  T: SizedSample + Send + 'static,
  f32: FromSample<T>,
{
  let channels = config.channels.max(1) as usize;
  let lost_cb = lost.clone();
  device
    .build_input_stream::<T, _, _>(
      *config,
      move |data: &[T], _| {
        for frame in data.chunks(channels) {
          let sum: f32 = frame.iter().map(|s| f32::from_sample(*s)).sum();
          if producer.push(sum / channels as f32).is_err() {
            overruns.fetch_add(1, Ordering::Relaxed);
          }
        }
      },
      move |e| {
        use cpal::ErrorKind as K;
        if matches!(e.kind(), K::DeviceNotAvailable | K::StreamInvalidated) {
          lost_cb.store(true, Ordering::SeqCst);
        }
      },
      None,
    )
    .map_err(|e| AudioError::Open(e.to_string()))
}

fn open_stream(
  device: &cpal::Device,
  supported: cpal::SupportedStreamConfig,
  producer: rtrb::Producer<f32>,
  overruns: Arc<AtomicU64>,
  lost: Arc<AtomicBool>,
) -> Result<(cpal::Stream, u32), AudioError> {
  let config: cpal::StreamConfig = supported.config();
  let rate = config.sample_rate;
  let stream = match supported.sample_format() {
    cpal::SampleFormat::F32 => build_mono_stream::<f32>(device, &config, producer, overruns, lost),
    cpal::SampleFormat::I16 => build_mono_stream::<i16>(device, &config, producer, overruns, lost),
    cpal::SampleFormat::I32 => build_mono_stream::<i32>(device, &config, producer, overruns, lost),
    cpal::SampleFormat::U16 => build_mono_stream::<u16>(device, &config, producer, overruns, lost),
    other => Err(AudioError::Open(format!("unsupported sample format {other:?}"))),
  }?;
  stream.play().map_err(|e| AudioError::Open(e.to_string()))?;
  Ok((stream, rate))
}

/// Per-frame processing shared by the microphone and synthetic sources.
struct FramePipeline {
  processor: Processor,
  outputs: CaptureOutputs,
  batch: Vec<i16>,
  stats: CaptureStats,
}

impl FramePipeline {
  fn frame(&mut self, frame: &mut [f32; FRAME], far: Option<&[f32; FRAME]>) {
    if self.processor.echo_cancellation {
      self.processor.render(far.unwrap_or(&[0.0; FRAME]));
    }
    self.processor.capture(frame);
    self.outputs.levels.push(frame);
    let pcm = to_i16(frame);
    for sink in self.outputs.frame_sinks.iter_mut() {
      sink(&pcm);
    }
    self.stats.frames += 1;
    self.batch.extend_from_slice(&pcm);
    if self.batch.len() >= PROVIDER_BATCH {
      send_batch(&self.outputs.provider, &mut self.batch, &mut self.stats);
    }
  }

  /// Trailing partial frame (under 10 ms) is zero padded so no speech is cut.
  fn finish(mut self, tail: &[f32]) -> CaptureStats {
    if !tail.is_empty() {
      let n = tail.len().min(FRAME);
      let mut frame = [0.0f32; FRAME];
      frame[..n].copy_from_slice(&tail[..n]);
      let pcm = to_i16(&frame);
      for sink in self.outputs.frame_sinks.iter_mut() {
        sink(&pcm);
      }
      self.batch.extend_from_slice(&pcm);
    }
    send_batch(&self.outputs.provider, &mut self.batch, &mut self.stats);
    self.stats
  }
}

impl Capture {
  fn start_synthetic(
    cfg: CaptureConfig,
    samples: Arc<Vec<i16>>,
    outputs: CaptureOutputs,
  ) -> Result<Capture, AudioError> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_dsp = stop.clone();
    let mut pipe = FramePipeline {
      processor: Processor::new(cfg.echo_cancellation, cfg.noise_suppression),
      outputs,
      batch: Vec::with_capacity(PROVIDER_BATCH),
      stats: CaptureStats {
        device_name: "Synthetic source".into(),
        device_rate: 16_000,
        ..Default::default()
      },
    };
    let dsp_thread = std::thread::Builder::new()
      .name("audio-synthetic".into())
      .spawn(move || {
        let started = Instant::now();
        let mut pos = 0usize;
        let mut n = 0u64;
        while !stop_dsp.load(Ordering::SeqCst) {
          let mut frame = [0.0f32; FRAME];
          for (i, s) in frame.iter_mut().enumerate() {
            *s = samples.get(pos + i).map(|v| *v as f32 / 32768.0).unwrap_or(0.0);
          }
          pos += FRAME;
          pipe.frame(&mut frame, None);
          n += 1;
          // Real-time pacing: frame n is due at n * 10 ms.
          let due = Duration::from_millis(n * 10);
          if let Some(wait) = due.checked_sub(started.elapsed()) {
            std::thread::sleep(wait);
          }
        }
        pipe.finish(&[])
      })
      .map_err(|e| AudioError::Open(e.to_string()))?;
    Ok(Capture {
      stop,
      stop_device: None,
      device_thread: None,
      dsp_thread: Some(dsp_thread),
      device_name: "Synthetic source".into(),
      opened_in: Duration::ZERO,
    })
  }

  pub fn start(cfg: CaptureConfig, outputs: CaptureOutputs) -> Result<Capture, AudioError> {
    if let Some(samples) = cfg.synthetic.clone() {
      return Self::start_synthetic(cfg, samples, outputs);
    }
    let started = Instant::now();
    let (mic_prod, mic_cons) = rtrb::RingBuffer::<f32>::new(RING);
    let (ref_prod, ref_cons) = rtrb::RingBuffer::<f32>::new(RING);
    let overruns = Arc::new(AtomicU64::new(0));
    let lost = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = std_mpsc::channel::<Result<Opened, AudioError>>();
    let (stop_dev_tx, stop_dev_rx) = std_mpsc::channel::<()>();

    let dev_cfg = cfg.clone();
    let (ov, lo) = (overruns.clone(), lost.clone());
    let device_thread = std::thread::Builder::new()
      .name("audio-device".into())
      .spawn(move || {
        let host = cpal::default_host();
        let device = match find_device(&host, dev_cfg.device_id.as_deref()) {
          Ok(d) => d,
          Err(e) => return drop(ready_tx.send(Err(e))),
        };
        let name = device
          .description()
          .map(|d| d.name().to_string())
          .unwrap_or_else(|_| "Microphone".into());
        let supported = match device.default_input_config() {
          Ok(s) => s,
          Err(e) => return drop(ready_tx.send(Err(AudioError::Open(e.to_string())))),
        };
        let (mic_stream, mic_rate) = match open_stream(&device, supported, mic_prod, ov.clone(), lo.clone()) {
          Ok(s) => s,
          Err(e) => return drop(ready_tx.send(Err(e))),
        };
        // Echo reference: loopback of the default output device.
        let mut ref_stream = None;
        let mut ref_rate = None;
        if dev_cfg.echo_cancellation {
          let loopback = host
            .default_output_device()
            .ok_or(AudioError::NoDevice)
            .and_then(|out| {
              out
                .default_output_config()
                .map(|c| (out, c))
                .map_err(|e| AudioError::Open(e.to_string()))
            })
            .and_then(|(out, c)| {
              open_stream(
                &out,
                c,
                ref_prod,
                Arc::new(AtomicU64::new(0)),
                Arc::new(AtomicBool::new(false)),
              )
            });
          match loopback {
            Ok((s, r)) => {
              ref_stream = Some(s);
              ref_rate = Some(r);
            }
            Err(e) => tracing::warn!(error = %e, "echo reference unavailable; echo cancellation has no speaker signal"),
          }
        }
        let _ = ready_tx.send(Ok(Opened {
          mic_rate,
          ref_rate,
          name,
        }));
        // Keep streams alive until stop.
        let _ = stop_dev_rx.recv();
        drop(ref_stream);
        drop(mic_stream);
      })
      .map_err(|e| AudioError::Open(e.to_string()))?;

    let opened = match ready_rx.recv_timeout(Duration::from_secs(5)) {
      Ok(Ok(o)) => o,
      Ok(Err(e)) => {
        let _ = device_thread.join();
        return Err(e);
      }
      Err(_) => {
        let _ = stop_dev_tx.send(());
        return Err(AudioError::Open("timed out opening the microphone".into()));
      }
    };

    let stop = Arc::new(AtomicBool::new(false));
    let stop_dsp = stop.clone();
    let device_name = opened.name.clone();
    let stats = CaptureStats {
      device_name: opened.name.clone(),
      device_rate: opened.mic_rate,
      echo_reference: opened.ref_rate.is_some(),
      ..Default::default()
    };
    let dsp_thread = std::thread::Builder::new()
      .name("audio-dsp".into())
      .spawn(move || {
        let mut pipe = FramePipeline {
          processor: Processor::new(cfg.echo_cancellation, cfg.noise_suppression),
          outputs,
          batch: Vec::with_capacity(PROVIDER_BATCH),
          stats,
        };
        let mut mic_cons = mic_cons;
        let mut ref_cons = ref_cons;
        let mut mic_rs = Resample::new(opened.mic_rate);
        let mut ref_rs = opened.ref_rate.map(Resample::new);
        let mut raw = Vec::<f32>::with_capacity(RING);
        let mut mic16 = Vec::<f32>::with_capacity(16_000);
        let mut ref16 = std::collections::VecDeque::<f32>::with_capacity(16_000);
        let mut ref_tmp = Vec::<f32>::new();
        let mut lost_reported = false;
        let mut flushed = false;
        loop {
          let stopping = stop_dsp.load(Ordering::SeqCst);
          raw.clear();
          while let Ok(s) = mic_cons.pop() {
            raw.push(s);
          }
          let got_mic = !raw.is_empty();
          mic_rs.push(&raw, &mut mic16);
          if let Some(rs) = ref_rs.as_mut() {
            raw.clear();
            while let Ok(s) = ref_cons.pop() {
              raw.push(s);
            }
            ref_tmp.clear();
            rs.push(&raw, &mut ref_tmp);
            ref16.extend(ref_tmp.iter().copied());
            while ref16.len() > MAX_REF_BACKLOG {
              ref16.pop_front();
            }
          }
          if stopping && !flushed && mic_cons.slots() == 0 {
            mic_rs.flush(&mut mic16);
            flushed = true;
          }
          let mut consumed = 0;
          while mic16.len() - consumed >= FRAME {
            let mut frame = [0.0f32; FRAME];
            frame.copy_from_slice(&mic16[consumed..consumed + FRAME]);
            consumed += FRAME;
            let far = (ref16.len() >= FRAME).then(|| {
              let mut f = [0.0f32; FRAME];
              for s in f.iter_mut() {
                *s = ref16.pop_front().unwrap_or(0.0);
              }
              f
            });
            pipe.frame(&mut frame, far.as_ref());
          }
          mic16.drain(..consumed);
          if lost.load(Ordering::SeqCst) && !lost_reported {
            lost_reported = true;
            (pipe.outputs.on_device_lost)();
          }
          if stopping && flushed {
            let mut stats = pipe.finish(&mic16);
            stats.ring_overruns = overruns.load(Ordering::Relaxed);
            return stats;
          }
          if !got_mic && !stopping {
            std::thread::sleep(Duration::from_millis(5));
          }
        }
      })
      .map_err(|e| AudioError::Open(e.to_string()))?;

    let opened_in = started.elapsed();
    tracing::info!(
      ms = opened_in.as_millis() as u64,
      rate = opened.mic_rate,
      echo_reference = opened.ref_rate.is_some(),
      "microphone open"
    );
    Ok(Capture {
      stop,
      stop_device: Some(stop_dev_tx),
      device_thread: Some(device_thread),
      dsp_thread: Some(dsp_thread),
      device_name,
      opened_in,
    })
  }

  /// Stops the device, drains and flushes all buffered audio to the sinks, and releases the
  /// microphone. Blocks briefly; call from a blocking-capable context.
  pub fn stop(mut self) -> CaptureStats {
    self.shutdown()
  }

  fn shutdown(&mut self) -> CaptureStats {
    if let Some(tx) = self.stop_device.take() {
      let _ = tx.send(());
    }
    if let Some(t) = self.device_thread.take() {
      let _ = t.join();
    }
    self.stop.store(true, Ordering::SeqCst);
    let stats = self.dsp_thread.take().and_then(|t| t.join().ok()).unwrap_or_default();
    tracing::info!(
      frames = stats.frames,
      overruns = stats.ring_overruns,
      provider_drops = stats.provider_drops,
      "microphone released"
    );
    stats
  }
}

impl Drop for Capture {
  fn drop(&mut self) {
    if self.dsp_thread.is_some() {
      self.shutdown();
    }
  }
}

fn send_batch(provider: &Option<mpsc::Sender<AudioMsg>>, batch: &mut Vec<i16>, stats: &mut CaptureStats) {
  if batch.is_empty() {
    return;
  }
  let pcm = std::mem::replace(batch, Vec::with_capacity(PROVIDER_BATCH));
  if let Some(tx) = provider {
    if tx.try_send(AudioMsg::Pcm(pcm)).is_err() {
      stats.provider_drops += 1;
    }
  }
}
