//! History commands and audio retention.
//!
//! Everything here works on stored data: no live session is touched and nothing is pasted.
//! Retry and recovery add a new attempt (lineage kept) and update the session's text; files
//! are only deleted through [`RecordingsDir::remove`], which checks path containment.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tokio_util::sync::CancellationToken;

use crate::app_host::{process_text_with, ProcessOutput, HISTORY_EVENT};
use crate::audio::recording::{decode_ogg_opus, write_wav};
use crate::audio::store::RecordingsDir;
use crate::db::history::{AttemptRow, HistoryItem, HistoryQuery, StorageStats, TextUpdate};
use crate::db::sessions::status;
use crate::db::{now_ms, Db};
use crate::providers::{replay, ProviderError};
use crate::services::Services;
use crate::settings;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const SWEEP_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// Sessions with a retry, recovery, or refinement running (one job per session).
#[derive(Default)]
pub struct Busy(Mutex<HashSet<String>>);

struct BusyGuard<'a> {
  busy: &'a Busy,
  id: String,
}

impl Busy {
  fn claim(&self, id: &str) -> Result<BusyGuard<'_>, String> {
    if !self.0.lock().unwrap().insert(id.to_string()) {
      return Err("Already working on this transcript.".into());
    }
    Ok(BusyGuard {
      busy: self,
      id: id.to_string(),
    })
  }
}

impl Drop for BusyGuard<'_> {
  fn drop(&mut self) {
    self.busy.0.lock().unwrap().remove(&self.id);
  }
}

#[derive(Serialize)]
pub struct HistoryDetail {
  #[serde(flatten)]
  pub item: HistoryItem,
  pub attempts: Vec<AttemptRow>,
}

#[derive(Serialize)]
pub struct StorageView {
  #[serde(flatten)]
  pub stats: StorageStats,
  pub retention_days: u32,
}

fn db_err(e: rusqlite::Error) -> String {
  tracing::warn!(error = %e, "history query failed");
  "History is unavailable right now.".into()
}

fn item(db: &Db, id: &str) -> Result<HistoryItem, String> {
  db.history_item(id)
    .map_err(db_err)?
    .ok_or_else(|| "This transcript no longer exists.".into())
}

fn provider_message(e: &ProviderError) -> String {
  match e {
    ProviderError::MissingKey => "Add an API key for the speech provider in Settings.".into(),
    ProviderError::Auth => "The speech provider rejected the API key.".into(),
    ProviderError::RateLimited => "The speech provider is rate limiting requests. Try again shortly.".into(),
    ProviderError::QuotaExceeded => {
      "The speech provider refused the request for quota or credits. Check the keys in Settings.".into()
    }
    ProviderError::Network => "Could not reach the speech provider.".into(),
    ProviderError::Timeout => "The speech provider took too long to answer.".into(),
    ProviderError::Unsupported(what) => format!("Not supported: {what}."),
    ProviderError::Protocol(_) => "The speech provider returned an error.".into(),
  }
}

fn changed(app: &AppHandle) {
  let _ = app.emit(HISTORY_EVENT, ());
}

fn save_output(db: &Db, id: &str, raw: &str, o: &ProcessOutput) -> Result<(), String> {
  db.update_text(
    id,
    &TextUpdate {
      raw_text: raw,
      formatted_text: &o.formatted,
      final_text: &o.final_text,
      refine_provider: o.refine_provider.as_deref(),
      refine_model: o.refine_model.as_deref(),
      refine_status: o.refine_status,
    },
  )
  .map_err(db_err)
}

#[tauri::command]
pub async fn history_list(db: State<'_, Db>, query: HistoryQuery) -> Result<Vec<HistoryItem>, String> {
  db.history(&query).map_err(db_err)
}

#[tauri::command]
pub async fn history_detail(db: State<'_, Db>, id: String) -> Result<HistoryDetail, String> {
  Ok(HistoryDetail {
    item: item(&db, &id)?,
    attempts: db.attempts(&id).map_err(db_err)?,
  })
}

/// The session's Ogg Opus recording as raw bytes (played by the WebView from a Blob).
#[tauri::command]
pub async fn history_audio(dir: State<'_, RecordingsDir>, id: String) -> Result<tauri::ipc::Response, String> {
  let path = dir.archive_path(&id).ok_or("Invalid transcript id.")?;
  if !dir.contains(&path) {
    return Err("The recording is no longer available.".into());
  }
  let bytes = tokio::fs::read(&path)
    .await
    .map_err(|_| "The recording could not be read.".to_string())?;
  Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub async fn history_copy(app: AppHandle, db: State<'_, Db>, id: String) -> Result<(), String> {
  let row = item(&db, &id)?.session;
  let text = row
    .final_text
    .or(row.raw_text)
    .filter(|t| !t.trim().is_empty())
    .ok_or("There is no text to copy.")?;
  app
    .clipboard()
    .write_text(text)
    .map_err(|_| "Could not copy to the clipboard.".to_string())
}

/// Transcribes the stored recording again with the current provider settings.
async fn retranscribe(app: &AppHandle, id: &str) -> Result<(), String> {
  let dir = app.state::<RecordingsDir>();
  let db = app.state::<Db>();
  let archive = dir.archive_path(id).ok_or("Invalid transcript id.")?;
  let has_audio = db.audio_for(id).map_err(db_err)?.is_some_and(|a| a.state == "encoded");
  if !has_audio || !dir.contains(&archive) {
    return Err("The recording for this transcript is no longer available.".into());
  }
  let pcm = tauri::async_runtime::spawn_blocking(move || decode_ogg_opus(&archive))
    .await
    .map_err(|_| "The recording could not be read.".to_string())?
    .map_err(|_| "The recording could not be decoded.".to_string())?;

  let s = settings::load(app);
  let adapter = app
    .state::<Services>()
    .registry
    .get(&s.stt_provider)
    .ok_or("The selected speech provider is not available.")?;
  if !adapter.configured() {
    return Err(provider_message(&ProviderError::MissingKey));
  }
  let options = adapter
    .descriptor()
    .resolve(s.stt_model(), &s.language, Vec::new())
    .map_err(|e| provider_message(&e))?;
  let attempt = db
    .start_retry_attempt(id, &s.stt_provider, &options.model, &options.language)
    .map_err(db_err)?;
  changed(app);
  tracing::info!(session = id, provider = %s.stt_provider, samples = pcm.len(), "retrying transcript from recording");
  let result = replay::transcribe(
    adapter,
    options,
    &pcm,
    replay::Pace::default(),
    CancellationToken::new(),
  )
  .await;
  let text = match result {
    Ok(t) if !t.trim().is_empty() => t,
    Ok(_) => {
      let _ = db.finish_attempt(attempt, "ok", None, Some(""));
      return Err("No speech was found in the recording.".into());
    }
    Err(e) => {
      tracing::warn!(session = id, error = %e, "transcript retry failed");
      let _ = db.finish_attempt(attempt, "failed", Some(&e.to_engine().code()), None);
      return Err(provider_message(&e));
    }
  };
  db.finish_attempt(attempt, "ok", None, Some(&text)).map_err(db_err)?;
  let out = process_text_with(app, &text, false, None, crate::textproc::Target::Prose)
    .await
    .ok_or("No speech was found in the recording.")?;
  save_output(&db, id, &text, &out)?;
  let _ = db.mark_recovered(id);
  Ok(())
}

#[tauri::command]
pub async fn history_retry(app: AppHandle, busy: State<'_, Busy>, id: String) -> Result<HistoryItem, String> {
  let _guard = busy.claim(&id)?;
  let db = app.state::<Db>();
  if item(&db, &id)?.session.status == status::IN_PROGRESS {
    return Err("This dictation is still running.".into());
  }
  let r = retranscribe(&app, &id).await;
  changed(&app);
  r?;
  item(&db, &id)
}

/// Makes an interrupted or failed session usable: from its saved words when there are any
/// (no provider call), otherwise from its recording.
#[tauri::command]
pub async fn history_recover(app: AppHandle, busy: State<'_, Busy>, id: String) -> Result<HistoryItem, String> {
  let _guard = busy.claim(&id)?;
  let db = app.state::<Db>();
  let row = item(&db, &id)?;
  if row.session.status == status::IN_PROGRESS {
    return Err("This dictation is still running.".into());
  }
  let raw = row.session.raw_text.clone().unwrap_or_default();
  let has_final = row.session.final_text.as_deref().is_some_and(|t| !t.trim().is_empty());
  let incomplete = matches!(row.session.status.as_str(), status::FAILED | status::INTERRUPTED);
  let r = if incomplete && row.audio_state.as_deref() == Some("encoded") {
    retranscribe(&app, &id).await
  } else if incomplete {
    Err("The complete recording is not available. Saved text may be incomplete and can still be copied.".into())
  } else if has_final {
    db.mark_recovered(&id).map(|_| ()).map_err(db_err)
  } else if !raw.trim().is_empty() {
    match process_text_with(&app, &raw, false, None, crate::textproc::Target::Prose).await {
      Some(out) => save_output(&db, &id, &raw, &out).and_then(|_| db.mark_recovered(&id).map(|_| ()).map_err(db_err)),
      None => Err("There is nothing to recover.".into()),
    }
  } else if row.audio_state.as_deref() == Some("encoded") {
    retranscribe(&app, &id).await
  } else {
    Err("There is no saved text or recording to recover from.".into())
  };
  changed(&app);
  r?;
  item(&db, &id)
}

/// Runs AI refinement again on the transcript. The current text stays if refinement fails.
#[tauri::command]
pub async fn history_retry_refinement(
  app: AppHandle,
  busy: State<'_, Busy>,
  id: String,
) -> Result<HistoryItem, String> {
  let _guard = busy.claim(&id)?;
  let db = app.state::<Db>();
  let row = item(&db, &id)?.session;
  let raw = row
    .raw_text
    .clone()
    .filter(|t| !t.trim().is_empty())
    .ok_or("There is no transcript to refine.")?;
  let out = process_text_with(&app, &raw, true, None, crate::textproc::Target::Prose)
    .await
    .ok_or("There is no transcript to refine.")?;
  if out.refine_status != "ok" {
    return Err("AI refinement failed. The transcript was not changed.".into());
  }
  save_output(&db, &id, &raw, &out)?;
  changed(&app);
  item(&db, &id)
}

fn export_path(dir: &std::path::Path, id: &str) -> PathBuf {
  let short = &id[..id.len().min(8)];
  let mut path = dir.join(format!("Dictation {short}.wav"));
  let mut n = 2;
  while path.exists() {
    path = dir.join(format!("Dictation {short} ({n}).wav"));
    n += 1;
  }
  path
}

/// Decodes the recording to a 16 kHz WAV in the Downloads folder. Returns the file path.
#[tauri::command]
pub async fn history_export_wav(app: AppHandle, id: String) -> Result<String, String> {
  let dir = app.state::<RecordingsDir>();
  let archive = dir.archive_path(&id).ok_or("Invalid transcript id.")?;
  if !dir.contains(&archive) {
    return Err("The recording is no longer available.".into());
  }
  let downloads = app
    .path()
    .download_dir()
    .map_err(|_| "The Downloads folder was not found.".to_string())?;
  let out = export_path(&downloads, &id);
  let target = out.clone();
  tauri::async_runtime::spawn_blocking(move || {
    let pcm = decode_ogg_opus(&archive)?;
    write_wav(&pcm, &target)
  })
  .await
  .map_err(|_| "Export failed.".to_string())?
  .map_err(|_| "Export failed.".to_string())?;
  tracing::info!(session = %id, "recording exported as WAV");
  Ok(out.to_string_lossy().into_owned())
}

/// Opens Explorer with an exported file selected. Only WAV files in Downloads.
#[tauri::command]
pub async fn history_show_export(app: AppHandle, path: String) -> Result<(), String> {
  let downloads = app.path().download_dir().map_err(|e| e.to_string())?;
  let p = PathBuf::from(&path);
  let inside = matches!((downloads.canonicalize(), p.canonicalize()), (Ok(d), Ok(f)) if f.starts_with(&d));
  if !inside || p.extension().and_then(|e| e.to_str()) != Some("wav") {
    return Err("Only exported recordings can be shown.".into());
  }
  #[cfg(windows)]
  {
    let _ = std::process::Command::new("explorer").arg("/select,").arg(&p).spawn();
  }
  Ok(())
}

/// Deletes a session's recording (`audio_only`) or the whole session with its recording.
#[tauri::command]
pub async fn history_delete(app: AppHandle, busy: State<'_, Busy>, id: String, audio_only: bool) -> Result<(), String> {
  let _guard = busy.claim(&id)?;
  let db = app.state::<Db>();
  let dir = app.state::<RecordingsDir>();
  if item(&db, &id)?.session.status == status::IN_PROGRESS {
    return Err("This dictation is still running.".into());
  }
  dir.remove(&db, &id).map_err(|e| {
    tracing::warn!(session = %id, error = %e, "recording delete failed");
    "The recording could not be deleted.".to_string()
  })?;
  if !audio_only {
    db.delete_session(&id).map_err(db_err)?;
  }
  tracing::info!(session = %id, audio_only, "history item deleted");
  changed(&app);
  Ok(())
}

#[tauri::command]
pub async fn history_storage(app: AppHandle, db: State<'_, Db>) -> Result<StorageView, String> {
  Ok(StorageView {
    stats: db.storage_stats().map_err(db_err)?,
    retention_days: settings::load(&app).audio_retention_days,
  })
}

/// Local usage metrics for the Insights page. `tz_offset_minutes` is the WebView's
/// `getTimezoneOffset()`, so days and months follow the user's clock.
#[tauri::command]
pub async fn insights(db: State<'_, Db>, tz_offset_minutes: i64) -> Result<crate::db::insights::Insights, String> {
  let rows = db.insight_rows().map_err(db_err)?;
  let (fixes, snippets) = db.fix_counts().map_err(db_err)?;
  Ok(crate::db::insights::aggregate(
    &rows,
    now_ms(),
    tz_offset_minutes.clamp(-14 * 60, 14 * 60),
    fixes,
    snippets,
  ))
}

#[derive(Debug, Default, PartialEq)]
pub struct SweepReport {
  pub deleted: usize,
  pub bytes: u64,
  pub failed: usize,
}

/// Deletes recordings older than `days` (0 keeps everything). Transcripts are kept.
pub fn sweep(db: &Db, dir: &RecordingsDir, days: u32, now: i64) -> SweepReport {
  let mut report = SweepReport::default();
  if days == 0 {
    return report;
  }
  let cutoff = now - days as i64 * DAY_MS;
  let Ok(ids) = db.audio_expired(cutoff) else {
    return report;
  };
  for id in ids {
    match dir.remove(db, &id) {
      Ok(bytes) => {
        report.deleted += 1;
        report.bytes += bytes;
      }
      Err(_) => report.failed += 1,
    }
  }
  report
}

fn run_sweep(app: &AppHandle) {
  let days = settings::load(app).audio_retention_days;
  let db = app.state::<Db>();
  let dir = app.state::<RecordingsDir>();
  let r = sweep(&db, &dir, days, now_ms());
  if r.deleted > 0 || r.failed > 0 {
    tracing::info!(
      deleted = r.deleted,
      bytes = r.bytes,
      failed = r.failed,
      days,
      "audio retention sweep"
    );
    changed(app);
  }
}

/// Runs a retention sweep now, off the async runtime.
pub fn sweep_soon(app: &AppHandle) {
  let app = app.clone();
  tauri::async_runtime::spawn_blocking(move || run_sweep(&app));
}

/// Sweeps at startup and every few hours.
pub fn start_retention(app: &AppHandle) {
  let app = app.clone();
  tauri::async_runtime::spawn(async move {
    loop {
      let a = app.clone();
      let _ = tauri::async_runtime::spawn_blocking(move || run_sweep(&a)).await;
      tokio::time::sleep(SWEEP_EVERY).await;
    }
  });
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::sessions::NewSession;

  #[test]
  fn sweep_deletes_only_expired_recordings_and_keeps_text() {
    let root = std::env::temp_dir().join(format!("dh-retention-{}", uuid::Uuid::new_v4()));
    let dir = RecordingsDir::new(&root);
    std::fs::create_dir_all(root.join("archive")).unwrap();
    let db = Db::open_in_memory().unwrap();
    let now = 100 * DAY_MS;
    let mk = |age_days: i64| {
      let id = uuid::Uuid::new_v4().to_string();
      db.create_session(&NewSession {
        id: &id,
        trigger: "hotkey",
        provider: "deepgram",
        model: "nova-3",
        language: "en",
        retry_of: None,
      })
      .unwrap();
      db.save_raw(&id, "keep me").unwrap();
      let path = dir.archive_path(&id).unwrap();
      std::fs::write(&path, b"opus-bytes").unwrap();
      db.audio_encoded(&id, &path.to_string_lossy(), 10, 1_000).unwrap();
      // Fake timestamps instead of waiting real days.
      db.conn()
        .execute(
          "UPDATE audio SET created_at = ?2 WHERE session_id = ?1",
          rusqlite::params![id, now - age_days * DAY_MS],
        )
        .unwrap();
      id
    };
    let old = mk(15);
    let edge = mk(13);
    let fresh = mk(0);

    assert_eq!(sweep(&db, &dir, 0, now), SweepReport::default(), "0 keeps forever");
    let r = sweep(&db, &dir, 14, now);
    assert_eq!(r.deleted, 1);
    assert_eq!(r.bytes, 10);
    assert!(!dir.archive_path(&old).unwrap().exists());
    assert!(dir.archive_path(&edge).unwrap().exists());
    assert!(dir.archive_path(&fresh).unwrap().exists());
    assert_eq!(db.audio_for(&old).unwrap().unwrap().state, "deleted");
    assert_eq!(
      db.session(&old).unwrap().unwrap().raw_text.as_deref(),
      Some("keep me"),
      "transcripts are kept"
    );
    // Idempotent; a shorter window catches the rest.
    assert_eq!(sweep(&db, &dir, 14, now).deleted, 0);
    assert_eq!(sweep(&db, &dir, 1, now).deleted, 1);
    let _ = std::fs::remove_dir_all(root);
  }

  #[test]
  fn export_names_do_not_overwrite() {
    let d = std::env::temp_dir().join(format!("dh-export-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    let id = "0123456789abcdef";
    let first = export_path(&d, id);
    assert!(first.ends_with("Dictation 01234567.wav"));
    std::fs::write(&first, b"x").unwrap();
    assert!(export_path(&d, id).ends_with("Dictation 01234567 (2).wav"));
    let _ = std::fs::remove_dir_all(d);
  }
}
