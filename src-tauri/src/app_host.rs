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
use crate::delivery::{self, CopyReason, Delivered};
use crate::engine::{self, EngineConfig, EngineHandle, EngineHooks, LevelEvent};
use crate::logging::text_shape;
use crate::services::Services;
use crate::session::host::Host;
use crate::session::machine::{DeliveryOutcome, Input, Outcome, Processed, SessionId, Snapshot, Trigger};
use crate::session::SessionHandle;
use crate::target::FocusSnapshot;
use crate::target::{self, Target};
use crate::{flowbar, learning, refine, settings, textproc};

pub const SNAPSHOT_EVENT: &str = "session://snapshot";
pub const LEVEL_EVENT: &str = "session://level";
/// History changed (a session was created or finished).
pub const HISTORY_EVENT: &str = "history://changed";

type Recorders = Arc<Mutex<HashMap<SessionId, RecordingWriter>>>;

/// Per-session facts read from the focused control at start (on a background thread).
/// Context text lives only here, in memory, and is dropped when the session is recorded.
#[derive(Default, Clone)]
struct Extras {
  focus: Option<FocusSnapshot>,
  context: Option<String>,
}

type ExtrasMap = Arc<Mutex<HashMap<SessionId, Extras>>>;

/// Characters of field text before the caret given to AI refinement.
const CONTEXT_CHARS: usize = 1_500;

pub struct TauriHost {
  app: AppHandle,
  engine: Mutex<Option<EngineHandle>>,
  /// Open local recordings by session (normally zero or one).
  recorders: Recorders,
  /// Where each session's text is meant to go, captured at start.
  targets: Mutex<HashMap<SessionId, Option<Target>>>,
  extras: ExtrasMap,
}

/// Why delivery copied instead of pasting, until the outcome is recorded (managed state, so
/// the blocking delivery task can write it).
#[derive(Default)]
pub struct CopyReasons(pub Mutex<HashMap<SessionId, CopyReason>>);

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
      targets: Mutex::new(HashMap::new()),
      extras: Arc::new(Mutex::new(HashMap::new())),
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
    let anchor = snapshot.session_id.as_deref().and_then(|sid| {
      let id = uuid::Uuid::parse_str(sid).ok()?;
      self.targets.lock().unwrap().get(&id).cloned().flatten().map(|t| t.hwnd)
    });
    flowbar::present(&self.app, snapshot.phase, anchor, snapshot.error.as_deref());
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
    // Capture the destination before anything else can take focus.
    let target = target::capture();
    match &target {
      Some(t) => {
        tracing::info!(session = %id, process = %t.process, kind = ?t.kind, elevated = t.elevated_mismatch, "target captured");
        db_warn("set_target", db.set_target(&sid, &t.app_label(), &t.title, &t.process));
      }
      None => tracing::info!(session = %id, "no target window"),
    }
    // Focused control, secure-field flag, selected tab, and (if enabled) context, read off
    // the actor so a slow app never delays the microphone.
    if let Some(t) = target.clone().filter(|t| t.kind == target::TargetKind::App) {
      let extras = self.extras.clone();
      let want_context = s.context_awareness;
      std::thread::spawn(move || {
        if target::foreground_relative_to(&t) != target::Foreground::Target {
          return;
        }
        let focus = target::focus_snapshot(t.hwnd);
        let secure = focus.as_ref().is_some_and(|f| f.secure);
        let context = if want_context && !secure {
          target::text_before_caret(CONTEXT_CHARS)
        } else {
          None
        };
        tracing::debug!(
          session = %id,
          secure,
          tab = focus.as_ref().is_some_and(|f| f.tab.is_some()),
          context = %context.as_deref().map(text_shape).unwrap_or_default(),
          "focus captured"
        );
        extras.lock().unwrap().insert(id, Extras { focus, context });
      });
    }
    self.targets.lock().unwrap().insert(id, target);
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
      // Dictionary terms as recognition hints; adapters drop them for models without hints.
      keyterms: lexicon(&self.app).vocabulary,
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
        let _ = level_app.emit_to(flowbar::LABEL, LEVEL_EVENT, ev);
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
    let context = self.extras.lock().unwrap().get(&id).and_then(|e| e.context.clone());
    let kind = self
      .targets
      .lock()
      .unwrap()
      .get(&id)
      .cloned()
      .flatten()
      .map(|t| textproc::Target::from_process(&t.process))
      .unwrap_or_default();
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
      let out = process_text_with(&app, &transcript, false, context.as_deref(), kind).await;
      let result = match &out {
        None => Processed::Empty,
        Some(o) => {
          // Persist every stage before delivery starts.
          let db = app.state::<Db>();
          db_warn("count_uses", db.count_uses(&o.corrections, &o.snippets));
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
    let target = self.targets.lock().unwrap().get(&id).cloned().flatten();
    let focus = self.extras.lock().unwrap().get(&id).and_then(|e| e.focus.clone());
    let s = settings::load(&app);
    let auto_paste = s.auto_paste;
    let learn_mode = learning::Mode::parse(&s.correction_learning).unwrap_or_default();
    let reasons_app = self.app.clone();
    tauri::async_runtime::spawn(async move {
      let tab = focus.as_ref().and_then(|f| f.tab.clone());
      let pasted_text = text.clone();
      let result = tauri::async_runtime::spawn_blocking(move || {
        delivery::deliver(&reasons_app, &text, target.as_ref(), tab, auto_paste)
      })
      .await
      .unwrap_or(Delivered::Failed);
      let outcome = match &result {
        Delivered::Pasted => {
          if let Some(f) = focus.filter(|f| !f.secure && learn_mode != learning::Mode::Off) {
            watch_corrections(app.clone(), pasted_text, f.runtime_id, learn_mode);
          }
          DeliveryOutcome::Pasted
        }
        Delivered::Copied(reason) => {
          tracing::info!(session = %id, reason = reason.code(), "copied instead of pasting");
          if let Some(state) = app.try_state::<CopyReasons>() {
            state.0.lock().unwrap().insert(id, *reason);
          }
          if let Some(msg) = reason.notice() {
            flowbar::notice(&app, msg);
          }
          DeliveryOutcome::Copied
        }
        Delivered::Failed => {
          flowbar::notice(&app, "Could not copy or paste the text. It is saved in History.");
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
    self.targets.lock().unwrap().remove(&id);
    // Context is ephemeral: gone with the session.
    self.extras.lock().unwrap().remove(&id);
    let reason = self
      .app
      .try_state::<CopyReasons>()
      .and_then(|r| r.0.lock().unwrap().remove(&id));
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
          delivery_reason: reason.map(CopyReason::code),
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

fn normalize_newlines(s: &str) -> String {
  s.replace("\r\n", "\n").replace('\r', "\n")
}

/// After a paste, reads the same focused control again a few times (ephemerally) and offers
/// small fixes the user made inside the pasted text as dictionary entries.
fn watch_corrections(app: AppHandle, inserted: String, focus_id: Vec<i32>, mode: learning::Mode) {
  std::thread::spawn(move || {
    let inserted = normalize_newlines(inserted.trim());
    std::thread::sleep(std::time::Duration::from_millis(700));
    let Some((id0, before)) = target::focused_text(learning::MAX_FIELD_CHARS) else {
      return;
    };
    let before = normalize_newlines(&before);
    if (!focus_id.is_empty() && id0 != focus_id) || !before.contains(&inserted) {
      return;
    }
    let mut after = None;
    let mut waited = std::time::Duration::ZERO;
    for &check in learning::CHECKS {
      std::thread::sleep(check - waited);
      waited = check;
      match target::focused_text(learning::MAX_FIELD_CHARS) {
        Some((id, text)) if id == id0 => after = Some(normalize_newlines(&text)),
        // Focus moved away: keep the last read.
        _ => break,
      }
    }
    let Some(after) = after else {
      return;
    };
    let learned = learning::learn(&inserted, &before, &after);
    if learned.is_empty() {
      return;
    }
    let status = if mode == learning::Mode::Auto {
      "active"
    } else {
      "suggested"
    };
    let db = app.state::<Db>();
    let mut added = 0;
    for l in &learned {
      if db.add_term(&l.phrase, l.heard.as_deref(), "learned", status).is_ok() {
        added += 1;
      }
    }
    tracing::info!(found = learned.len(), added, status, "correction learning");
    if added > 0 {
      let _ = app.emit(crate::dictionary::LEXICON_EVENT, ());
    }
  });
}

pub struct ProcessOutput {
  pub formatted: String,
  pub final_text: String,
  pub refine_provider: Option<String>,
  pub refine_model: Option<String>,
  /// ok | failed | off
  pub refine_status: &'static str,
  /// Dictionary corrections and snippets used (row ids, one per use).
  pub corrections: Vec<i64>,
  pub snippets: Vec<i64>,
}

/// The user's active dictionary and snippets (empty if the database is unavailable).
pub fn lexicon(app: &AppHandle) -> textproc::Lexicon {
  match app.state::<Db>().lexicon() {
    Ok(l) => l,
    Err(e) => {
      tracing::warn!(error = %e, "dictionary unavailable; continuing without it");
      textproc::Lexicon::default()
    }
  }
}

/// Deterministic pipeline (dictionary, snippets, spoken commands, smart formatting), then
/// optional AI refinement with deterministic fallback. `None` means there was no text.
/// `force_refine` runs AI refinement even when it is turned off (History "Retry
/// refinement"); `context` is field text before the caret (never stored).
pub async fn process_text_with(
  app: &AppHandle,
  transcript: &str,
  force_refine: bool,
  context: Option<&str>,
  target: textproc::Target,
) -> Option<ProcessOutput> {
  let s = settings::load(app);
  let lex = lexicon(app);
  let target = if s.coding_awareness {
    target
  } else {
    textproc::Target::Prose
  };
  let prepared = textproc::prepare(transcript, &lex, s.smart_formatting, target);
  // Restoring into the deterministic text cannot fail: it holds every placeholder once.
  let formatted = textproc::finish(&prepared.text, &prepared.expansions).unwrap_or_else(|_| prepared.text.clone());
  if formatted.trim().is_empty() {
    return None;
  }
  let off = |formatted: String| ProcessOutput {
    final_text: formatted.clone(),
    formatted,
    refine_provider: None,
    refine_model: None,
    refine_status: "off",
    corrections: prepared.corrections.clone(),
    snippets: prepared.snippets.clone(),
  };
  // Nothing to refine when the dictation was only snippet triggers.
  if (!s.ai_refine && !force_refine) || !prepared.has_prose() {
    return Some(off(formatted));
  }
  let p = refine::provider(&s.ai_provider);
  let model = if p.id == "megallm" {
    s.megallm_model.clone()
  } else {
    s.openrouter_model.clone()
  };
  let key = app.state::<Services>().refine_key(p);
  let req = refine::Request {
    text: &prepared.text,
    vocabulary: &lex.vocabulary,
    context,
    code: target.is_code(),
    timeout: if force_refine {
      refine::RETRY_TIMEOUT
    } else {
      refine::LIVE_TIMEOUT
    },
  };
  let (final_text, refine_status) = match refine::refine(p, key, &model, &req).await {
    Ok(refined) => match textproc::finish(&refined, &prepared.expansions) {
      Ok(text) => (text, "ok"),
      Err(_) => {
        tracing::warn!("refinement altered a snippet placeholder; using formatted text");
        (formatted.clone(), "failed")
      }
    },
    Err(_) => (formatted.clone(), "failed"),
  };
  Some(ProcessOutput {
    formatted,
    final_text,
    refine_provider: Some(p.id.to_string()),
    refine_model: Some(model),
    refine_status,
    corrections: prepared.corrections.clone(),
    snippets: prepared.snippets.clone(),
  })
}
