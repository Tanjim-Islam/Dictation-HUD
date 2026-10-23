//! Dev check for local speech models. Build the helper first: `npm run dev:asr`.
//!
//! `cargo run --example asr_probe -- download <model-id> [models-dir]`
//!   Downloads a catalog model with the app's real downloader (resume, hashes, atomic install).
//! `cargo run --example asr_probe -- run <model-id> <16k mono wav> [models-dir] [lang]`
//!   Runs a recording through the real helper and LocalAdapter; prints timing and the text
//!   (use test audio only).
//!
//! `cargo run --example asr_probe -- link <folder>`
//!   Shows which catalog model a folder holds, as "Use a model on this PC" would.
//!
//! models-dir defaults to DICTATION_MODELS_DIR.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use dictation_hud::local_asr::catalog;
use dictation_hud::local_asr::client::{Helper, Limits, ProcessLauncher};
use dictation_hud::local_asr::download::{download_model, Hosts};
use dictation_hud::local_asr::store::{default_hf_hub, ModelsDir};
use dictation_hud::providers::local::{LocalAdapter, ModelLocator};
use dictation_hud::providers::{replay, SttAdapter};
use tokio_util::sync::CancellationToken;

struct Probe(ModelsDir);

impl ModelLocator for Probe {
  fn model_dir(&self, id: &str) -> Option<PathBuf> {
    self.0.resolve(catalog::get(id)?, None).dir()
  }
  fn vad_path(&self) -> Option<PathBuf> {
    Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/silero_vad.onnx"))
  }
  fn enough_memory(&self, _: u64) -> bool {
    true
  }
}

fn models(root: Option<String>) -> ModelsDir {
  let root = root
    .or_else(|| std::env::var("DICTATION_MODELS_DIR").ok())
    .map(PathBuf::from)
    .expect("models dir (or DICTATION_MODELS_DIR)");
  ModelsDir::new(root, default_hf_hub(|k| std::env::var(k).ok()))
}

#[tokio::main]
async fn main() {
  let mut args = std::env::args().skip(1);
  let mode = args.next().expect("download, run, or link");
  if mode == "link" {
    let folder = PathBuf::from(args.next().expect("folder"));
    match dictation_hud::local_asr::link::identify(&folder, catalog::CATALOG) {
      Ok((m, dir)) => println!("link: {} at {}", m.id, dir.display()),
      Err(e) => println!("link refused: {}", e.message()),
    }
    return;
  }
  let id = args.next().expect("model id");
  let m = catalog::get(&id).expect("catalog model id");
  match mode.as_str() {
    "download" => {
      let dir = models(args.next());
      let t = Instant::now();
      let mut last = 0u64;
      let r = download_model(m, &dir, &Hosts::default(), CancellationToken::new(), |p| {
        let pct = p.done * 100 / p.total.max(1);
        if pct >= last + 10 {
          last = pct;
          println!("  {pct}%");
        }
      })
      .await;
      println!("download {id}: {r:?} in {} s", t.elapsed().as_secs());
      println!("state: {:?}", dir.resolve(m, None));
    }
    "run" => {
      let wav = args.next().expect("wav path");
      let dir = models(args.next());
      let lang = args.next().unwrap_or_else(|| "en".into());
      let launcher = ProcessLauncher::find();
      println!("helper: {} (exists: {})", launcher.exe.display(), launcher.exe.exists());
      let helper = Helper::new(Arc::new(launcher), Limits::default());
      let adapter: Arc<dyn SttAdapter> = Arc::new(LocalAdapter::new(helper, Arc::new(Probe(dir))));
      let pcm = dictation_hud::audio::capture::load_wav_16k(Path::new(&wav)).expect("wav");
      let options = adapter
        .descriptor()
        .resolve(&id, &lang, Vec::new())
        .expect("model and language");
      let audio_s = pcm.len() as f64 / 16_000.0;
      for round in ["cold", "warm"] {
        let t = Instant::now();
        let pace = replay::Pace {
          speed: 0,
          ..Default::default()
        };
        let r = replay::transcribe(adapter.clone(), options.clone(), &pcm, pace, CancellationToken::new()).await;
        let secs = t.elapsed().as_secs_f64();
        println!(
          "{round}: {secs:.2} s for {audio_s:.1} s audio (RTF {:.3}) -> {r:?}",
          secs / audio_s
        );
      }
    }
    other => panic!("unknown mode {other}"),
  }
}
