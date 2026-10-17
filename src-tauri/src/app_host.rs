//! Tauri implementation of the session [`Host`].
//!
//! Durable writes happen here, in lifecycle order: session row on creation, segments as they
//! finalize, transcript stages before delivery, outcome at the end. A database error is logged
//! and never blocks dictation.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};

use crate::audio::capture::CaptureConfig;
use crate::audio::recording::RecordingWriter;
use crate::audio::store::RecordingsDir;
use crate::db::sessions::{self, NewSession, Processing};
use crate::db::Db;
use crate::engine::{self, EngineConfig, EngineHandle, EngineHooks, LevelEvent};
use crate::logging::text_shape;
use crate::services::Services;
use crate::session::host::Host;
use crate::session::machine::{DeliveryOutcome, Input, Outcome, Phase, Processed, SessionId, Snapshot, Trigger};
use crate::session::SessionHandle;
use crate::{flowbar, paste, refine, settings, symbols};

pub const SNAPSHOT_EVENT: &str = "session://snapshot";
pub const LEVEL_EVENT: &str = "session://level";
/// History changed (a session was created or finished).
pub const HISTORY_EVENT: &str = "history://changed";

type Recorders = Arc<Mutex<HashMap<SessionId, RecordingWriter>>>;

pub struct TauriHost {
  app: AppHandle,
  engine: Mutex<Option<EngineHandle>>,
  /// Open local recordings by session (normally zero or one).
  recorders: Recorders,
}

/// Closes a session's recording (if any) and encodes it off the async runtime.
fn close_recording(app: &AppHandle, recorders: &Recorders, id: SessionId) {
  let Some(writer) = recorders.lock().unwrap().remove(&id) else {
    return;
  };
  let app = app.clone();
  tauri::async_runtime::spawn_blocking(move || {
    let staged = writer.close();
    if staged.write_errors > 0 {
      tracing::error!(session = %id, errors = staged.write_errors, "recording had write errors");
    }
    let dir = app.state::<RecordingsDir>();
    let db = app.state::<Db>();
    let _ = dir.finalize(&db, &id.to_string());
    let _ = app.emit(HISTORY_EVENT, ());
  });
}

fn db_warn<T>(what: &str, r: rusqlite::Result<T>) -> Option<T> {
  match r {
    Ok(v) => Some(v),
    Err(e) => {
      tracing::warn!(op = what, error = %e, "database write failed");
      None
    }
  }
}

impl TauriHost {
  pub fn new(app: AppHandle) -> Self {
    Self {
      app,
      engine: Mutex::new(None),
      recorders: Arc::new(Mutex::new(HashMap::new())),
    }
  }

  fn with_engine(&self, id: SessionId, f: impl FnOnce(&mut EngineHandle)) {
    let mut slot = self.engine.lock().unwrap();
    if let Some(h) = slot.as_mut().filter(|h| h.session_id == id) {
      f(h);
    }
  }

  fn db(&self) -> tauri::State<'_, Db> {
    self.app.state::<Db>()
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

  fn created(&self, id: SessionId, trigger: Trigger) {
    let s = settings::load(&self.app);
    let sid = id.to_string();
    let db = self.db();
    db_warn(
      "create_session",
      db.create_session(&NewSession {
        id: &sid,
        trigger: trigger.as_str(),
        provider: &s.stt_provider,
        model: s.stt_model(),
        language: &s.language,
        retry_of: None,
      }),
    );
    db_warn(
      "start_attempt",
      db.start_attempt(&sid, "live", &s.stt_provider, s.stt_model(), &s.language),
    );
    let _ = self.app.emit(HISTORY_EVENT, ());
  }

  fn segment(&self, id: SessionId, text: &str) {
    let db = self.db();
    if let Some(Some(attempt)) = db_warn("current_attempt", db.current_attempt(&id.to_string())) {
      db_warn("add_segment", db.add_segment(attempt, text));
    }
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
    let level_app = self.app.clone();
    let end_app = self.app.clone();
    let sink_app = self.app.clone();
    let end_recorders = self.recorders.clone();
    let sink_recorders = self.recorders.clone();
    let hooks = EngineHooks {
      levels: Arc::new(move |ev: LevelEvent| {
        let _ = level_app.emit_to("hud", LEVEL_EVENT, ev);
      }),
      on_capture_end: Arc::new(move |sid, stats| {
        let db = end_app.state::<Db>();
        db_warn("set_duration", db.set_duration(&sid.to_string(), stats.duration_ms()));
        close_recording(&end_app, &end_recorders, sid);
      }),
      // Durable local recording starts with the first captured frame, before the provider.
      frame_sinks: Arc::new(move |sid| {
        let dir = sink_app.state::<RecordingsDir>();
        let Some(path) = dir.staging_path(&sid.to_string()) else {
          return Vec::new();
        };
        match RecordingWriter::create(&path) {
          Ok(writer) => {
            let db = sink_app.state::<Db>();
            db_warn(
              "audio_staging",
              db.audio_staging(&sid.to_string(), &path.to_string_lossy()),
            );
            let sink = writer.sink();
            sink_recorders.lock().unwrap().insert(sid, writer);
            vec![sink]
          }
          Err(e) => {
            tracing::error!(session = %sid, error = %e, "local recording unavailable; dictation continues without it");
            Vec::new()
          }
        }
      }),
    };
    let services = self.app.state::<Services>();
    let handle = engine::start(&services.registry, cfg, id, session.clone(), hooks);
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
      let sid = id.to_string();
      {
        let db = app.state::<Db>();
        if let Some(Some(attempt)) = db_warn("current_attempt", db.current_attempt(&sid)) {
          db_warn(
            "finish_attempt",
            db.finish_attempt(attempt, "ok", None, Some(&transcript)),
          );
        }
      }
      let out = process_text(&app, &transcript).await;
      let result = match &out {
        None => Processed::Empty,
        Some(o) => {
          // Persist every stage before delivery starts.
          let db = app.state::<Db>();
          db_warn(
            "save_processing",
            db.save_processing(
              &sid,
              &Processing {
                raw_text: &transcript,
                formatted_text: &o.formatted,
                final_text: &o.final_text,
                refine_provider: o.refine_provider.as_deref(),
                refine_model: o.refine_model.as_deref(),
                refine_status: o.refine_status,
              },
            ),
          );
          Processed::Text(o.final_text.clone())
        }
      };
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
    let sid = id.to_string();
    let failure;
    let (status, failure, delivery) = match outcome {
      Outcome::Completed { delivery } => (
        sessions::status::COMPLETED,
        None,
        Some(match delivery {
          DeliveryOutcome::Pasted => "pasted",
          DeliveryOutcome::Copied => "copied",
          DeliveryOutcome::Failed => "failed",
        }),
      ),
      Outcome::NoSpeech => (sessions::status::NO_SPEECH, None, None),
      Outcome::Cancelled => (sessions::status::CANCELLED, None, None),
      Outcome::Interrupted => (sessions::status::INTERRUPTED, Some("app_closed"), None),
      Outcome::Failed { error } => {
        failure = error.code();
        (sessions::status::FAILED, Some(failure.as_str()), None)
      }
    };
    // Covers sessions whose capture never stopped normally (start failure, shutdown).
    close_recording(&self.app, &self.recorders, id);
    let db = self.db();
    if !transcript.trim().is_empty() {
      db_warn("save_raw", db.save_raw(&sid, transcript));
    }
    db_warn(
      "finalize",
      db.finalize(
        &sid,
        &sessions::Outcome {
          status,
          failure,
          delivery,
          duration_ms: None,
        },
      ),
    );
    let _ = self.app.emit(HISTORY_EVENT, ());
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

pub struct ProcessOutput {
  pub formatted: String,
  pub final_text: String,
  pub refine_provider: Option<String>,
  pub refine_model: Option<String>,
  /// ok | failed | off
  pub refine_status: &'static str,
}

/// Deterministic formatting, then optional AI refinement with deterministic fallback.
/// `None` means there was no text.
pub async fn process_text(app: &AppHandle, transcript: &str) -> Option<ProcessOutput> {
  let formatted = symbols::replace_symbols(transcript);
  if formatted.trim().is_empty() {
    return None;
  }
  let s = settings::load(app);
  if !s.ai_refine {
    return Some(ProcessOutput {
      final_text: formatted.clone(),
      formatted,
      refine_provider: None,
      refine_model: None,
      refine_status: "off",
    });
  }
  let p = refine::provider(&s.ai_provider);
  let model = if p.id == "megallm" {
    s.megallm_model.clone()
  } else {
    s.openrouter_model.clone()
  };
  let key = app.state::<Services>().refine_key(p);
  let (final_text, refine_status) = match refine::refine(p, key, &model, &formatted).await {
    Ok(refined) => (refined, "ok"),
    Err(_) => (refine::basic_punctuation_cleanup(&formatted), "failed"),
  };
  Some(ProcessOutput {
    formatted,
    final_text,
    refine_provider: Some(p.id.to_string()),
    refine_model: Some(model),
    refine_status,
  })
}
