//! Native audio spike: enumerate devices and time opening the default input.
//! Run: cargo run --example audio_probe
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
  let host = cpal::default_host();
  println!("host: {:?}", host.id());
  for d in host.input_devices().unwrap() {
    let name = d.description().map(|x| x.name().to_string()).unwrap_or_default();
    let cfg = d
      .default_input_config()
      .map(|c| format!("{} Hz {} ch {:?}", c.sample_rate(), c.channels(), c.sample_format()));
    println!(
      "input: {name} | id={} | {cfg:?}",
      d.id().map(|i| i.to_string()).unwrap_or_default()
    );
  }
  let out = host.default_output_device().unwrap();
  println!(
    "default output: {:?} {:?}",
    out.description().map(|x| x.name().to_string()),
    out.default_output_config().map(|c| (c.sample_rate(), c.channels()))
  );
  let t0 = Instant::now();
  let dev = host.default_input_device().expect("no input");
  let sc = dev.default_input_config().unwrap();
  let frames = Arc::new(AtomicUsize::new(0));
  let first = Arc::new(std::sync::Mutex::new(None::<Duration>));
  let (f2, first2) = (frames.clone(), first.clone());
  let stream = dev
    .build_input_stream::<f32, _, _>(
      sc.config(),
      move |data, _| {
        if f2.fetch_add(data.len(), Ordering::Relaxed) == 0 {
          *first2.lock().unwrap() = Some(t0.elapsed());
        }
      },
      |e| eprintln!("stream error: {e}"),
      None,
    )
    .unwrap();
  let built = t0.elapsed();
  stream.play().unwrap();
  std::thread::sleep(Duration::from_millis(1500));
  drop(stream);
  println!(
    "build={built:?} first_callback={:?} samples_1.5s={}",
    first.lock().unwrap(),
    frames.load(Ordering::Relaxed)
  );
}
