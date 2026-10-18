//! Dev check: `cargo run --example coding_probe -- <16k mono wav> <prose|editor|terminal>`.
//! Streams a recording through the real Deepgram adapter (key from Windows Credential Manager,
//! never printed) and prints the provider text and the pipeline output for that target.

use std::sync::Arc;

use dictation_hud::credentials::{names, OsSecretStore, SecretStore, SERVICE};
use dictation_hud::providers::deepgram::Deepgram;
use dictation_hud::providers::{replay, SttAdapter};
use dictation_hud::textproc::{self, Lexicon, Target};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() {
  let mut args = std::env::args().skip(1);
  let wav = args.next().expect("wav path");
  let target = match args.next().as_deref() {
    Some("editor") => Target::Editor,
    Some("terminal") => Target::Terminal,
    _ => Target::Prose,
  };
  let pcm = dictation_hud::audio::capture::load_wav_16k(std::path::Path::new(&wav)).expect("wav");
  let store = OsSecretStore::new(SERVICE);
  let adapter: Arc<dyn SttAdapter> = Arc::new(Deepgram::new(Arc::new(move || {
    store
      .get(names::DEEPGRAM)
      .or_else(|| dictation_hud::services::dev_env("DEEPGRAM_API_KEY"))
  })));
  let options = adapter
    .descriptor()
    .resolve("nova-3", "en", Vec::new())
    .expect("options");
  let raw = replay::transcribe(
    adapter,
    options,
    &pcm,
    replay::Pace::default(),
    CancellationToken::new(),
  )
  .await
  .expect("transcribe");
  let p = textproc::prepare(&raw, &Lexicon::default(), true, target);
  println!("provider: {raw}");
  println!(
    "output:   {}",
    textproc::finish(&p.text, &p.expansions).unwrap_or(p.text)
  );
}
