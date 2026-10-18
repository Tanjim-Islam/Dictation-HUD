//! Dictation-core soak: `cargo run --release --example engine_soak -- <16k wav> <minutes>`.
//! Runs back-to-back sessions through the real session actor and engine (synthetic capture,
//! DSP, Deepgram streaming, finalization) with no UI, clipboard, or paste. Prints one line per
//! session: outcome, words, timing. Memory is sampled from outside the process.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dictation_hud::audio::capture::CaptureConfig;
use dictation_hud::credentials::{names, OsSecretStore, SecretStore, SERVICE};
use dictation_hud::engine::{self, EngineConfig, EngineHandle};
use dictation_hud::providers::deepgram::Deepgram;
use dictation_hud::providers::Registry;
use dictation_hud::session::actor;
use dictation_hud::session::host::Host;
use dictation_hud::session::machine::{
  DeliveryOutcome, Input, Outcome, Phase, Processed, SessionId, Snapshot, Trigger,
};
use dictation_hud::session::SessionHandle;

struct SoakHost {
  registry: Registry,
  audio: Arc<Vec<i16>>,
  engine: Mutex<Option<EngineHandle>>,
  outcomes: Mutex<Vec<(Outcome, usize)>>,
}

impl Host for SoakHost {
  fn publish(&self, _: &Snapshot) {}
  fn start_engine(&self, id: SessionId, session: &SessionHandle) {
    let cfg = EngineConfig {
      provider: "deepgram".into(),
      model: "nova-3".into(),
      language: "en".into(),
      keyterms: Vec::new(),
      capture: CaptureConfig {
        device_id: None,
        echo_cancellation: true,
        noise_suppression: true,
        synthetic: Some(self.audio.clone()),
      },
    };
    let h = engine::start(&self.registry, cfg, id, session.clone(), engine::EngineHooks::none());
    if let Some(old) = self.engine.lock().unwrap().replace(h) {
      old.cancel();
    }
  }
  fn finish_engine(&self, id: SessionId) {
    if let Some(h) = self.engine.lock().unwrap().as_mut().filter(|h| h.session_id == id) {
      h.finish();
    }
  }
  fn cancel_engine(&self, id: SessionId) {
    if let Some(h) = self.engine.lock().unwrap().as_ref().filter(|h| h.session_id == id) {
      h.cancel();
    }
  }
  fn process(&self, id: SessionId, text: String, session: &SessionHandle) {
    let s = session.clone();
    tauri::async_runtime::spawn(async move { s.send(Input::Processed(id, Processed::Text(text))).await });
  }
  fn deliver(&self, id: SessionId, _text: String, session: &SessionHandle) {
    let s = session.clone();
    tauri::async_runtime::spawn(async move { s.send(Input::Delivered(id, DeliveryOutcome::Pasted)).await });
  }
  fn record(&self, _: SessionId, outcome: &Outcome, transcript: &str) {
    self
      .outcomes
      .lock()
      .unwrap()
      .push((outcome.clone(), transcript.split_whitespace().count()));
  }
}

#[tokio::main]
async fn main() {
  let mut args = std::env::args().skip(1);
  let wav = args.next().expect("wav path");
  let minutes: u64 = args.next().and_then(|m| m.parse().ok()).unwrap_or(20);
  let audio = Arc::new(dictation_hud::audio::capture::load_wav_16k(std::path::Path::new(&wav)).expect("wav"));
  let store = OsSecretStore::new(SERVICE);
  let mut registry = Registry::default();
  registry.register(Arc::new(Deepgram::new(Arc::new(move || {
    store.get(names::DEEPGRAM).or_else(|| {
      // Dev machines keep the key in .env, which release builds of the app never read.
      let _ = dotenvy::dotenv();
      std::env::var("DEEPGRAM_API_KEY").ok()
    })
  }))));
  let host = Arc::new(SoakHost {
    registry,
    audio,
    engine: Mutex::new(None),
    outcomes: Mutex::new(Vec::new()),
  });
  let session = actor::spawn(host.clone());
  let deadline = Instant::now() + Duration::from_secs(minutes * 60);
  let mut n = 0;
  let mut failures = 0;
  println!("pid={}", std::process::id());
  while Instant::now() < deadline {
    n += 1;
    let started = Instant::now();
    let before = host.outcomes.lock().unwrap().len();
    session.send(Input::Start(Trigger::System)).await;
    // Speak (the synthetic source plays the file), then finish.
    tokio::time::sleep(Duration::from_secs(13)).await;
    session.send(Input::Finish(Trigger::System)).await;
    let waited = Instant::now();
    while host.outcomes.lock().unwrap().len() == before && waited.elapsed() < Duration::from_secs(30) {
      tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let last = host.outcomes.lock().unwrap().last().cloned();
    let phase = session.snapshot().await.map(|s| s.phase);
    match last {
      Some((Outcome::Completed { .. }, words)) if host.outcomes.lock().unwrap().len() > before => {
        println!(
          "#{n} ok words={words} ms={} idle={}",
          started.elapsed().as_millis(),
          phase == Some(Phase::Idle)
        );
      }
      other => {
        failures += 1;
        println!("#{n} FAIL {:?} phase={phase:?}", other.map(|o| o.0));
        session.send(Input::Cancel(Trigger::System)).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
      }
    }
  }
  println!("done sessions={n} failures={failures}");
  session.shutdown(Duration::from_secs(5)).await;
}
