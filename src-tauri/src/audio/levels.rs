//! Microphone level metering. The DSP thread pushes 10 ms frames; the UI side reads the latest
//! snapshot at display rate. Raw audio never goes to the WebView.

use std::sync::Mutex;

use serde::Serialize;

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq)]
pub struct LevelFrame {
  /// RMS in dBFS (-100..0).
  pub rms_db: f32,
  /// Peak in dBFS (-100..0).
  pub peak_db: f32,
}

#[derive(Default)]
pub struct LevelMeter {
  inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
  /// Frames since the last read, folded into one value: max peak, max rms.
  pending: Option<LevelFrame>,
  seq: u64,
}

fn to_db(x: f32) -> f32 {
  if x <= 1e-5 {
    -100.0
  } else {
    (20.0 * x.log10()).max(-100.0)
  }
}

impl LevelMeter {
  pub fn push(&self, frame: &[f32]) {
    if frame.is_empty() {
      return;
    }
    let sum: f32 = frame.iter().map(|s| s * s).sum();
    let rms = (sum / frame.len() as f32).sqrt();
    let peak = frame.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let f = LevelFrame {
      rms_db: to_db(rms),
      peak_db: to_db(peak),
    };
    let mut inner = self.inner.lock().unwrap();
    inner.pending = Some(match inner.pending {
      Some(p) => LevelFrame {
        rms_db: p.rms_db.max(f.rms_db),
        peak_db: p.peak_db.max(f.peak_db),
      },
      None => f,
    });
    inner.seq += 1;
  }

  /// Latest folded level since the previous call, if any frames arrived.
  pub fn take(&self) -> Option<LevelFrame> {
    self.inner.lock().unwrap().pending.take()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn silence_and_full_scale() {
    let m = LevelMeter::default();
    m.push(&[0.0; 160]);
    assert_eq!(m.take().unwrap().rms_db, -100.0);
    assert!(m.take().is_none());
    m.push(&[1.0; 160]);
    let f = m.take().unwrap();
    assert!(f.rms_db.abs() < 0.01 && f.peak_db.abs() < 0.01);
  }

  #[test]
  fn folds_max_between_reads() {
    let m = LevelMeter::default();
    m.push(&[0.5; 160]);
    m.push(&[0.01; 160]);
    let f = m.take().unwrap();
    assert!((f.peak_db - to_db(0.5)).abs() < 0.01);
  }
}
