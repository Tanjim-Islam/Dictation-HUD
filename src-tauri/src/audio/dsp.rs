//! Audio processing on the dedicated DSP thread (never in the device callback).

use rubato::{FftFixedInOut, Resampler};
use sonora::config::{EchoCanceller, HighPassFilter, NoiseSuppression, NoiseSuppressionLevel};
use sonora::{AudioProcessing, Config, StreamConfig};

pub const TARGET_RATE: u32 = 16_000;
/// 10 ms at 16 kHz: the frame size the audio processor works with.
pub const FRAME: usize = 160;

/// Mono resampler to 16 kHz with internal accumulation. Passthrough at 16 kHz.
pub struct Resample {
  inner: Option<FftFixedInOut<f32>>,
  pending: Vec<f32>,
  out_buf: Vec<Vec<f32>>,
}

impl Resample {
  pub fn new(input_rate: u32) -> Self {
    if input_rate == TARGET_RATE {
      return Self {
        inner: None,
        pending: Vec::new(),
        out_buf: Vec::new(),
      };
    }
    let chunk = (input_rate / 100).max(1) as usize;
    let inner =
      FftFixedInOut::<f32>::new(input_rate as usize, TARGET_RATE as usize, chunk, 1).expect("valid sample rates");
    let out_len = inner.output_frames_max();
    Self {
      inner: Some(inner),
      pending: Vec::with_capacity(chunk * 4),
      out_buf: vec![vec![0.0; out_len]],
    }
  }

  /// Appends resampled output for `input` to `out`.
  pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
    let Some(r) = self.inner.as_mut() else {
      out.extend_from_slice(input);
      return;
    };
    self.pending.extend_from_slice(input);
    loop {
      let need = r.input_frames_next();
      if self.pending.len() < need {
        break;
      }
      if let Ok((used, produced)) = r.process_into_buffer(&[&self.pending[..need]], &mut self.out_buf, None) {
        out.extend_from_slice(&self.out_buf[0][..produced]);
        self.pending.drain(..used);
      } else {
        self.pending.drain(..need);
      }
    }
  }

  /// Pushes out buffered samples at the end of a stream.
  pub fn flush(&mut self, out: &mut Vec<f32>) {
    let Some(r) = self.inner.as_mut() else { return };
    let rest = std::mem::take(&mut self.pending);
    let input = [rest.as_slice()];
    if let Ok((_, produced)) = r.process_partial_into_buffer(Some(&input), &mut self.out_buf, None) {
      out.extend_from_slice(&self.out_buf[0][..produced]);
    }
  }
}

/// Echo cancellation and noise suppression. Both are optional and independent; with both off
/// the signal passes through untouched.
pub struct Processor {
  apm: Option<AudioProcessing>,
  pub echo_cancellation: bool,
  pub noise_suppression: bool,
}

impl Processor {
  pub fn new(echo_cancellation: bool, noise_suppression: bool) -> Self {
    let apm = (echo_cancellation || noise_suppression).then(|| {
      let config = Config {
        high_pass_filter: Some(HighPassFilter::default()),
        echo_canceller: echo_cancellation.then(EchoCanceller::default),
        noise_suppression: noise_suppression.then(|| NoiseSuppression {
          level: NoiseSuppressionLevel::Moderate,
          ..Default::default()
        }),
        ..Default::default()
      };
      AudioProcessing::builder()
        .config(config)
        .capture_config(StreamConfig::new(TARGET_RATE, 1))
        .render_config(StreamConfig::new(TARGET_RATE, 1))
        .build()
    });
    Self {
      apm,
      echo_cancellation,
      noise_suppression,
    }
  }

  /// Far-end (speaker) reference, one 10 ms frame. Only used for echo cancellation.
  pub fn render(&mut self, frame: &[f32; FRAME]) {
    if !self.echo_cancellation {
      return;
    }
    if let Some(apm) = self.apm.as_mut() {
      let mut sink = [0.0f32; FRAME];
      let _ = apm.process_render_f32(&[frame], &mut [&mut sink]);
    }
  }

  /// Near-end (microphone) frame, processed in place.
  pub fn capture(&mut self, frame: &mut [f32; FRAME]) {
    let Some(apm) = self.apm.as_mut() else { return };
    let input = *frame;
    if apm.process_capture_f32(&[&input], &mut [frame]).is_err() {
      *frame = input;
    }
  }
}

pub fn to_i16(frame: &[f32]) -> Vec<i16> {
  frame.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16).collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn sine(rate: u32, hz: f32, secs: f32) -> Vec<f32> {
    let n = (rate as f32 * secs) as usize;
    (0..n)
      .map(|i| (i as f32 / rate as f32 * hz * std::f32::consts::TAU).sin() * 0.5)
      .collect()
  }

  #[test]
  fn resamples_common_device_rates_to_16k() {
    for rate in [8_000u32, 16_000, 44_100, 48_000] {
      let input = sine(rate, 440.0, 1.0);
      let mut r = Resample::new(rate);
      let mut out = Vec::new();
      for chunk in input.chunks(333) {
        r.push(chunk, &mut out);
      }
      r.flush(&mut out);
      let expected = 16_000i64;
      assert!(
        (out.len() as i64 - expected).abs() < 400,
        "rate {rate}: got {}",
        out.len()
      );
    }
  }

  #[test]
  fn processor_passthrough_when_both_off() {
    let mut p = Processor::new(false, false);
    let mut f = [0.25f32; FRAME];
    p.capture(&mut f);
    assert!(f.iter().all(|s| (*s - 0.25).abs() < 1e-6));
  }

  #[test]
  fn noise_suppression_attenuates_steady_noise() {
    let mut p = Processor::new(false, true);
    // Deterministic pseudo-noise.
    let mut seed = 1u32;
    let mut energy_in = 0.0f32;
    let mut energy_out = 0.0f32;
    for i in 0..300 {
      let mut f = [0.0f32; FRAME];
      for s in f.iter_mut() {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        *s = ((seed >> 16) as f32 / 32768.0 - 1.0) * 0.05;
      }
      if i > 150 {
        energy_in += f.iter().map(|s| s * s).sum::<f32>();
      }
      p.capture(&mut f);
      if i > 150 {
        energy_out += f.iter().map(|s| s * s).sum::<f32>();
      }
    }
    assert!(energy_out < energy_in * 0.5, "in {energy_in} out {energy_out}");
  }

  #[test]
  fn echo_cancellation_reduces_far_end_echo() {
    let mut p = Processor::new(true, false);
    let mut seed = 7u32;
    let mut echo_in = 0.0f32;
    let mut echo_out = 0.0f32;
    for i in 0..500 {
      let mut far = [0.0f32; FRAME];
      for s in far.iter_mut() {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        *s = ((seed >> 16) as f32 / 32768.0 - 1.0) * 0.3;
      }
      p.render(&far);
      // Mic hears the speaker attenuated (simple echo path, same frame).
      let mut mic = [0.0f32; FRAME];
      for (m, f) in mic.iter_mut().zip(far.iter()) {
        *m = f * 0.5;
      }
      if i > 300 {
        echo_in += mic.iter().map(|s| s * s).sum::<f32>();
      }
      p.capture(&mut mic);
      if i > 300 {
        echo_out += mic.iter().map(|s| s * s).sum::<f32>();
      }
    }
    assert!(echo_out < echo_in * 0.5, "echo in {echo_in} out {echo_out}");
  }
}
