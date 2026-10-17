//! Tauri implementation of the session [`Host`].

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};

use crate::audio::capture::CaptureConfig;
use crate::engine::{self, EngineConfig, EngineHandle, LevelEvent};
use crate::logging::text_shape;
use crate::services::Services;
use crate::session::host::Host;
use crate::session::machine::{DeliveryOutcome, Input, Outcome, Phase, Processed, SessionId, Snapshot};
use crate::session::SessionHandle;
use crate::{flowbar, paste, refine, settings, symbols};

pub const SNAPSHOT_EVENT: &str = "session://snapshot";
pub const LEVEL_EVENT: &str = "session://level";

pub struct TauriHost {
  app: AppHandle,
  engine: Mutex<Option<EngineHandle>>,
}

impl TauriHost {
  pub fn new(app: AppHandle) -> Self {
    Self {
      app,
      engine: Mutex::new(None),
    }
  }

  fn with_engine(&self, id: SessionId, f: impl FnOnce(&mut EngineHandle)) {
    let mut slot = self.engine.lock().unwrap();
    if let Some(h) = slot.as_mut().filter(|h| h.session_id == id) {
      f(h);
    }
  }
}

impl Host for TauriHost {
  fn publish(&self, snapshot: &Snapshot) {
    if snapshot.phase == Phase::Idle {
      flowbar::hide(&self.app);
    } else {
      flowbar::show(&self.app);
    }
    let _ = self.app.emit(SNAPSHOT_EVENT, snapshot);
  }

  fn start_engine(&self, id: SessionId, session: &SessionHandle) {
    let s = settings::load(&self.app);
    let cfg = EngineConfig {
      provider: s.stt_provider.clone(),
      model: s.stt_model().to_string(),
      language: s.language.clone(),
      keyterms: Vec::new(),
      capture: CaptureConfig {
        device_id: s.input_device.clone(),
        echo_cancellation: s.echo_cancellation,
        noise_suppression: s.noise_suppression,
        synthetic: synthetic_source(),
      },
    };
    let app = self.app.clone();
    let levels: engine::LevelEmitter = Arc::new(move |ev: LevelEvent| {
      let _ = app.emit_to("hud", LEVEL_EVENT, ev);
    });
    let services = self.app.state::<Services>();
    let handle = engine::start(&services.registry, cfg, id, session.clone(), levels);
    if let Some(old) = self.engine.lock().unwrap().replace(handle) {
      old.cancel();
    }
  }

  fn finish_engine(&self, id: SessionId) {
    self.with_engine(id, |h| h.finish());
  }

  fn cancel_engine(&self, id: SessionId) {
    self.with_engine(id, |h| h.cancel());
  }

  fn process(&self, id: SessionId, transcript: String, session: &SessionHandle) {
    let app = self.app.clone();
    let session = session.clone();
    tauri::async_runtime::spawn(async move {
      let result = process_text(&app, &transcript).await;
      session.send(Input::Processed(id, result)).await;
    });
  }

  fn deliver(&self, id: SessionId, text: String, session: &SessionHandle) {
    let app = self.app.clone();
    let session = session.clone();
    tauri::async_runtime::spawn(async move {
      let auto_paste = settings::load(&app).auto_paste;
      let outcome = match paste::copy_and_paste(&app, &text, auto_paste).await {
        Ok(true) => DeliveryOutcome::Pasted,
        Ok(false) => DeliveryOutcome::Copied,
        Err(e) => {
          tracing::warn!(error = %e, "delivery failed");
          DeliveryOutcome::Failed
        }
      };
      session.send(Input::Delivered(id, outcome)).await;
    });
  }

  fn record(&self, id: SessionId, outcome: &Outcome, transcript: &str) {
    tracing::info!(session = %id, outcome = ?outcome, transcript = %text_shape(transcript), "session recorded");
    if let Some(state) = self.app.try_state::<crate::LastTranscript>() {
      if matches!(outcome, Outcome::Completed { .. }) && !transcript.trim().is_empty() {
        *state.0.lock().unwrap() = Some(transcript.to_string());
      }
    }
  }
}

/// Debug builds only: `DICTATION_TEST_AUDIO=<16 kHz mono WAV>` replaces the microphone so
/// live provider tests can run with recorded speech.
fn synthetic_source() -> Option<Arc<Vec<i16>>> {
  if !cfg!(debug_assertions) {
    return None;
  }
  let path = std::env::var("DICTATION_TEST_AUDIO").ok()?;
  match crate::audio::capture::load_wav_16k(std::path::Path::new(&path)) {
    Ok(samples) => Some(Arc::new(samples)),
    Err(e) => {
      tracing::warn!(error = %e, "DICTATION_TEST_AUDIO ignored");
      None
    }
  }
}

/// Deterministic formatting, then optional AI refinement with deterministic fallback.
pub async fn process_text(app: &AppHandle, transcript: &str) -> Processed {
  let formatted = symbols::replace_symbols(transcript);
  if formatted.trim().is_empty() {
    return Processed::Empty;
  }
  let s = settings::load(app);
  if !s.ai_refine {
    return Processed::Text(formatted);
  }
  let p = refine::provider(&s.ai_provider);
  let model = if p.id == "megallm" {
    s.megallm_model.clone()
  } else {
    s.openrouter_model.clone()
  };
  let key = app.state::<Services>().refine_key(p);
  match refine::refine(p, key, &model, &formatted).await {
    Ok(refined) => Processed::Text(refined),
    Err(_) => Processed::Text(refine::basic_punctuation_cleanup(&formatted)),
  }
}
