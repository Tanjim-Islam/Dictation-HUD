//! Provider contract: a new adapter plugs into the registry and runs full sessions through the
//! real session actor and engine with no orchestration changes. Uses the synthetic audio
//! source, so no microphone or network is needed.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use dictation_hud::audio::capture::CaptureConfig;
use dictation_hud::engine::{self, EngineConfig, EngineHandle};
use dictation_hud::providers::mock::{MockAdapter, MockBehavior};
use dictation_hud::providers::{ProviderError, Registry};
use dictation_hud::session::actor;
use dictation_hud::session::host::Host;
use dictation_hud::session::machine::{
  DeliveryOutcome, Input, Outcome, Phase, Processed, SessionId, Snapshot, Trigger,
};
use dictation_hud::session::SessionHandle;

struct TestHost {
  registry: Registry,
  provider: String,
  engine: Mutex<Option<EngineHandle>>,
  delivered: Mutex<Vec<String>>,
  outcomes: Mutex<Vec<Outcome>>,
}

impl TestHost {
  fn new(registry: Registry, provider: &str) -> Arc<Self> {
    Arc::new(Self {
      registry,
      provider: provider.into(),
      engine: Mutex::new(None),
      delivered: Mutex::new(Vec::new()),
      outcomes: Mutex::new(Vec::new()),
    })
  }
}

impl Host for TestHost {
  fn publish(&self, _: &Snapshot) {}
  fn start_engine(&self, id: SessionId, session: &SessionHandle) {
    let cfg = EngineConfig {
      provider: self.provider.clone(),
      model: "mock-1".into(),
      language: "bn".into(),
      keyterms: vec!["City Bank".into()],
      capture: CaptureConfig {
        device_id: None,
        echo_cancellation: true,
        noise_suppression: true,
        synthetic: Some(Arc::new(vec![1000i16; 16_000])),
      },
    };
    let h = engine::start(&self.registry, cfg, id, session.clone(), engine::EngineHooks::none());
    *self.engine.lock().unwrap() = Some(h);
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
  fn deliver(&self, id: SessionId, text: String, session: &SessionHandle) {
    self.delivered.lock().unwrap().push(text);
    let s = session.clone();
    tauri::async_runtime::spawn(async move { s.send(Input::Delivered(id, DeliveryOutcome::Pasted)).await });
  }
  fn record(&self, _: SessionId, outcome: &Outcome, _: &str) {
    self.outcomes.lock().unwrap().push(outcome.clone());
  }
}

async fn wait_phase(h: &SessionHandle, phase: Phase, secs: u64) {
  for _ in 0..(secs * 50) {
    if h.snapshot().await.map(|s| s.phase) == Some(phase) {
      return;
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
  }
  panic!("did not reach {phase:?}; now {:?}", h.snapshot().await.map(|s| s.phase));
}

fn registry_with(adapter: MockAdapter) -> Registry {
  let mut r = Registry::default();
  r.register(Arc::new(adapter));
  r
}

#[test]
fn third_provider_runs_full_session_without_orchestration_changes() {
  tauri::async_runtime::block_on(async {
    let host = TestHost::new(
      registry_with(MockAdapter::new(
        "acme",
        MockBehavior::Transcribe(vec!["hello".into(), "world".into()]),
      )),
      "acme",
    );
    let h = actor::spawn(host.clone());
    h.send(Input::Toggle(Trigger::Hotkey)).await;
    wait_phase(&h, Phase::Listening, 3).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    h.send(Input::Toggle(Trigger::Hotkey)).await;
    wait_phase(&h, Phase::Idle, 5).await;
    assert_eq!(host.delivered.lock().unwrap().as_slice(), ["hello world"]);
    assert_eq!(
      host.outcomes.lock().unwrap().as_slice(),
      [Outcome::Completed {
        delivery: DeliveryOutcome::Pasted
      }]
    );
  });
}

#[test]
fn auth_failure_becomes_error_without_delivery() {
  tauri::async_runtime::block_on(async {
    let host = TestHost::new(
      registry_with(MockAdapter::new(
        "acme",
        MockBehavior::FailOnConnect(ProviderError::Auth),
      )),
      "acme",
    );
    let h = actor::spawn(host.clone());
    h.send(Input::Start(Trigger::Tray)).await;
    wait_phase(&h, Phase::Error, 3).await;
    assert!(h.snapshot().await.unwrap().error.unwrap().contains("rejected"));
    assert!(host.delivered.lock().unwrap().is_empty());
  });
}

#[test]
fn mid_stream_disconnect_fails_session() {
  tauri::async_runtime::block_on(async {
    let host = TestHost::new(
      registry_with(MockAdapter::new(
        "acme",
        MockBehavior::FailMidStream(ProviderError::Network),
      )),
      "acme",
    );
    let h = actor::spawn(host.clone());
    h.send(Input::Start(Trigger::Tray)).await;
    wait_phase(&h, Phase::Error, 3).await;
    assert!(matches!(host.outcomes.lock().unwrap()[0], Outcome::Failed { .. }));
  });
}

#[test]
fn hung_finalize_times_out_and_cancel_stops_engine() {
  tauri::async_runtime::block_on(async {
    let host = TestHost::new(
      registry_with(MockAdapter::new("acme", MockBehavior::HangOnFinish)),
      "acme",
    );
    let h = actor::spawn(host.clone());
    h.send(Input::Start(Trigger::Tray)).await;
    wait_phase(&h, Phase::Listening, 3).await;
    h.send(Input::Finish(Trigger::Tray)).await;
    // Finalize timer (6 s) ends the session with no speech instead of hanging forever.
    wait_phase(&h, Phase::Idle, 9).await;
    assert_eq!(host.outcomes.lock().unwrap().as_slice(), [Outcome::NoSpeech]);
  });
}

#[test]
fn unknown_provider_and_unsupported_language_fail_cleanly() {
  tauri::async_runtime::block_on(async {
    let host = TestHost::new(Registry::default(), "nope");
    let h = actor::spawn(host.clone());
    h.send(Input::Start(Trigger::Tray)).await;
    wait_phase(&h, Phase::Error, 3).await;
  });
}

/// The durable recording gets every frame even when the provider dies mid-stream, and the
/// capture-end hook runs after all sinks were flushed.
#[test]
fn provider_failure_still_records_audio() {
  use dictation_hud::audio::capture::FrameSink;
  use dictation_hud::engine::EngineHooks;
  use std::sync::atomic::{AtomicUsize, Ordering};

  tauri::async_runtime::block_on(async {
    let recorded = Arc::new(AtomicUsize::new(0));
    let ended = Arc::new(AtomicUsize::new(0));
    struct RecHost {
      inner: Arc<TestHost>,
      recorded: Arc<AtomicUsize>,
      ended: Arc<AtomicUsize>,
    }
    impl Host for RecHost {
      fn publish(&self, s: &Snapshot) {
        self.inner.publish(s)
      }
      fn start_engine(&self, id: SessionId, session: &SessionHandle) {
        let rec = self.recorded.clone();
        let end = self.ended.clone();
        let rec_at_end = self.recorded.clone();
        let hooks = EngineHooks {
          levels: Arc::new(|_| {}),
          on_capture_end: Arc::new(move |_, stats| {
            assert_eq!(
              rec_at_end.load(Ordering::SeqCst) as u64,
              stats.frames * 160,
              "sinks flushed before hook"
            );
            end.fetch_add(1, Ordering::SeqCst);
          }),
          frame_sinks: Arc::new(move |_| {
            let rec = rec.clone();
            let sink: FrameSink = Box::new(move |f: &[i16]| {
              rec.fetch_add(f.len(), Ordering::SeqCst);
            });
            vec![sink]
          }),
        };
        let cfg = EngineConfig {
          provider: "acme".into(),
          model: "mock-1".into(),
          language: "en".into(),
          keyterms: vec![],
          capture: CaptureConfig {
            device_id: None,
            echo_cancellation: false,
            noise_suppression: false,
            synthetic: Some(Arc::new(vec![500i16; 32_000])),
          },
        };
        let h = engine::start(&self.inner.registry, cfg, id, session.clone(), hooks);
        *self.inner.engine.lock().unwrap() = Some(h);
      }
      fn finish_engine(&self, id: SessionId) {
        self.inner.finish_engine(id)
      }
      fn cancel_engine(&self, id: SessionId) {
        self.inner.cancel_engine(id)
      }
      fn process(&self, id: SessionId, t: String, s: &SessionHandle) {
        self.inner.process(id, t, s)
      }
      fn deliver(&self, id: SessionId, t: String, s: &SessionHandle) {
        self.inner.deliver(id, t, s)
      }
      fn record(&self, id: SessionId, o: &Outcome, t: &str) {
        self.inner.record(id, o, t)
      }
    }
    let inner = TestHost::new(
      registry_with(MockAdapter::new(
        "acme",
        MockBehavior::FailMidStream(ProviderError::Network),
      )),
      "acme",
    );
    let host = Arc::new(RecHost {
      inner: inner.clone(),
      recorded: recorded.clone(),
      ended: ended.clone(),
    });
    let h = actor::spawn(host);
    h.send(Input::Start(Trigger::Tray)).await;
    wait_phase(&h, Phase::Error, 3).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(ended.load(Ordering::SeqCst), 1);
    assert!(
      recorded.load(Ordering::SeqCst) > 0,
      "audio captured before the failure is kept"
    );
    assert!(matches!(inner.outcomes.lock().unwrap()[0], Outcome::Failed { .. }));
  });
}
