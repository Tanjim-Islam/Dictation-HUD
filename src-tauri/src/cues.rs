//! Short sounds when the Flow Bar opens (dictation starts) and closes (dictation finishes or
//! is cancelled). Both are two soft bell-like notes synthesized at startup: rising to open,
//! falling to close, so they sound related but distinct. No audio files are shipped.

use std::sync::OnceLock;

const RATE: u32 = 44_100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
  Open,
  Close,
}

/// One note: a sine with a soft octave partial, a 4 ms attack and an exponential decay.
fn note(out: &mut [f32], start: usize, freq: f32, gain: f32, decay_s: f32) {
  let attack = (RATE as f32 * 0.004) as usize;
  for (i, s) in out[start..].iter_mut().enumerate() {
    let t = i as f32 / RATE as f32;
    let env = (i as f32 / attack as f32).min(1.0) * (-t / decay_s).exp();
    let phase = std::f32::consts::TAU * freq * t;
    *s += gain * env * (phase.sin() + 0.18 * (2.0 * phase).sin() * (-t / (decay_s * 0.4)).exp());
  }
}

/// 16-bit mono PCM samples for a cue.
pub fn samples(cue: Cue) -> Vec<i16> {
  let len = (RATE as f32 * 0.26) as usize;
  let mut buf = vec![0f32; len];
  let gap = (RATE as f32 * 0.07) as usize;
  // A4 up to E5 to open, the same pair down to close: soft, low enough not to be shrill.
  let (first, second) = match cue {
    Cue::Open => (440.0, 659.25),
    Cue::Close => (659.25, 440.0),
  };
  note(&mut buf, 0, first, 0.13, 0.06);
  note(&mut buf, gap, second, 0.15, 0.08);
  // Fade the tail to silence so the buffer never ends on a click.
  let fade = (RATE as f32 * 0.03) as usize;
  for i in 0..fade {
    buf[len - fade + i] *= 1.0 - i as f32 / fade as f32;
  }
  buf
    .iter()
    .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
    .collect()
}

/// A complete RIFF/WAVE file in memory.
pub fn wav(samples: &[i16]) -> Vec<u8> {
  let data_len = (samples.len() * 2) as u32;
  let mut v = Vec::with_capacity(44 + data_len as usize);
  v.extend_from_slice(b"RIFF");
  v.extend_from_slice(&(36 + data_len).to_le_bytes());
  v.extend_from_slice(b"WAVEfmt ");
  v.extend_from_slice(&16u32.to_le_bytes());
  v.extend_from_slice(&1u16.to_le_bytes()); // PCM
  v.extend_from_slice(&1u16.to_le_bytes()); // mono
  v.extend_from_slice(&RATE.to_le_bytes());
  v.extend_from_slice(&(RATE * 2).to_le_bytes());
  v.extend_from_slice(&2u16.to_le_bytes());
  v.extend_from_slice(&16u16.to_le_bytes());
  v.extend_from_slice(b"data");
  v.extend_from_slice(&data_len.to_le_bytes());
  for s in samples {
    v.extend_from_slice(&s.to_le_bytes());
  }
  v
}

fn sound(cue: Cue) -> &'static [u8] {
  static OPEN: OnceLock<Vec<u8>> = OnceLock::new();
  static CLOSE: OnceLock<Vec<u8>> = OnceLock::new();
  let cell = match cue {
    Cue::Open => &OPEN,
    Cue::Close => &CLOSE,
  };
  cell.get_or_init(|| wav(&samples(cue)))
}

/// Plays a cue without blocking. A new cue replaces one still playing.
#[cfg(windows)]
pub fn play(cue: Cue) {
  use windows::core::PCWSTR;
  use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
  let bytes = sound(cue);
  // SND_MEMORY takes the WAV bytes in place of a file name; they live for the whole process.
  let ok = unsafe {
    PlaySoundW(
      PCWSTR(bytes.as_ptr() as *const u16),
      None,
      SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
    )
  };
  if ok.as_bool() {
    tracing::debug!(cue = ?cue, "cue sound played");
  } else {
    tracing::debug!(cue = ?cue, "cue sound failed");
  }
}

#[cfg(not(windows))]
pub fn play(cue: Cue) {
  let _ = sound(cue);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn cues_are_short_quiet_and_distinct() {
    let open = samples(Cue::Open);
    let close = samples(Cue::Close);
    assert_eq!(open.len(), close.len());
    assert!(open.len() < RATE as usize / 2, "under half a second");
    let peak = open.iter().map(|s| s.unsigned_abs()).max().unwrap();
    assert!(peak < i16::MAX as u16 / 2, "never loud or clipped");
    assert_ne!(open, close);
    assert_eq!(*open.last().unwrap(), 0, "ends in silence");
  }

  #[test]
  fn wav_header_is_valid() {
    let w = wav(&[1, -1]);
    assert_eq!(&w[0..4], b"RIFF");
    assert_eq!(&w[8..12], b"WAVE");
    assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 4);
    assert_eq!(w.len(), 48);
  }
}
