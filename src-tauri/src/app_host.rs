//! Tauri implementation of the session [`Host`].

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::logging::text_shape;
use crate::session::host::Host;
use crate::session::machine::{DeliveryOutcome, Input, Outcome, Phase, Processed, SessionId, Snapshot};
use crate::session::SessionHandle;
use crate::{flowbar, paste, refine, settings, symbols};

pub const SNAPSHOT_EVENT: &str = "session://snapshot";

pub struct TauriHost {
  app: AppHandle,
}

impl TauriHost {
  pub fn new(app: AppHandle) -> Self {
    Self { app }
  }
}

/// Phase 1 bridge: the WebView still runs microphone capture and the provider socket. It
/// receives these commands and reports `engine_event`s tagged with the session id.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct EngineCommand {
  session_id: String,
  provider: String,
  echo_cancellation: bool,
  noise_suppression: bool,
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

  fn start_engine(&self, id: SessionId, _session: &SessionHandle) {
    let prefs = settings::load_behavior(&self.app);
    let cmd = EngineCommand {
      session_id: id.to_string(),
      provider: prefs.stt_provider,
      echo_cancellation: prefs.echo_cancellation,
      noise_suppression: prefs.noise_suppression,
    };
    let _ = self.app.emit_to("hud", "engine://start", cmd);
  }

  fn finish_engine(&self, id: SessionId) {
    let _ = self.app.emit_to("hud", "engine://finish", id.to_string());
  }

  fn cancel_engine(&self, id: SessionId) {
    let _ = self.app.emit_to("hud", "engine://cancel", id.to_string());
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
      let auto_paste = settings::load_behavior(&app).auto_paste;
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

/// Deterministic formatting, then optional AI refinement with deterministic fallback.
pub async fn process_text(app: &AppHandle, transcript: &str) -> Processed {
  let formatted = symbols::replace_symbols(transcript);
  if formatted.trim().is_empty() {
    return Processed::Empty;
  }
  let prefs = settings::load_behavior(app);
  if !prefs.ai_refine {
    return Processed::Text(formatted);
  }
  match refine::refine_with_provider(app, &prefs.ai_provider, &formatted).await {
    Ok(refined) => Processed::Text(refined),
    Err(_) => Processed::Text(refine::basic_punctuation_cleanup(&formatted)),
  }
}
