//! Per-frame cost of the capture DSP (AEC + NS). Run: cargo run --release --example dsp_bench
use dictation_hud::audio::dsp::{Processor, FRAME};
use std::time::Instant;

fn main() {
  for (aec, ns) in [(true, true), (false, true), (true, false), (false, false)] {
    let mut p = Processor::new(aec, ns);
    let mut seed = 1u32;
    let n = 3000; // 30 s of audio
    let t = Instant::now();
    for _ in 0..n {
      let mut far = [0.0f32; FRAME];
      let mut mic = [0.0f32; FRAME];
      for (f, m) in far.iter_mut().zip(mic.iter_mut()) {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        *f = ((seed >> 16) as f32 / 32768.0 - 1.0) * 0.2;
        *m = *f * 0.3;
      }
      p.render(&far);
      p.capture(&mut mic);
    }
    let per = t.elapsed().as_secs_f64() / n as f64 * 1e6;
    println!(
      "aec={aec} ns={ns}: {per:.1} us per 10 ms frame ({:.2}% of one core)",
      per / 100.0
    );
  }
}
