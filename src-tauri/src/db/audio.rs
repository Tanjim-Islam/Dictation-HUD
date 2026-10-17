//! Audio records: one per session. `staging` while recording (raw PCM), `encoded` after the
//! verified Ogg Opus file exists, `failed` when encoding failed (the staging file is kept),
//! `deleted` after retention or user deletion.

use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::{now_ms, Db};

pub mod state {
  pub const STAGING: &str = "staging";
  pub const ENCODED: &str = "encoded";
  pub const FAILED: &str = "failed";
  pub const DELETED: &str = "deleted";
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AudioRow {
  pub session_id: String,
  pub state: String,
  pub path: String,
  pub bytes: Option<i64>,
  pub duration_ms: Option<i64>,
  pub codec: Option<String>,
  pub created_at: i64,
}

impl Db {
  pub fn audio_staging(&self, session_id: &str, path: &str) -> rusqlite::Result<()> {
    self.conn().execute(
      "INSERT INTO audio (session_id, state, path, created_at) VALUES (?1, 'staging', ?2, ?3)
       ON CONFLICT(session_id) DO UPDATE SET state = 'staging', path = ?2",
      params![session_id, path, now_ms()],
    )?;
    Ok(())
  }

  pub fn audio_encoded(&self, session_id: &str, path: &str, bytes: i64, duration_ms: i64) -> rusqlite::Result<()> {
    self.conn().execute(
      "INSERT INTO audio (session_id, state, path, bytes, duration_ms, codec, created_at, encoded_at)
       VALUES (?1, 'encoded', ?2, ?3, ?4, 'opus', ?5, ?5)
       ON CONFLICT(session_id) DO UPDATE SET state = 'encoded', path = ?2, bytes = ?3, duration_ms = ?4,
         codec = 'opus', error = NULL, encoded_at = ?5",
      params![session_id, path, bytes, duration_ms, now_ms()],
    )?;
    Ok(())
  }

  pub fn audio_failed(&self, session_id: &str, error: &str) -> rusqlite::Result<()> {
    self.conn().execute(
      "UPDATE audio SET state = 'failed', error = ?2 WHERE session_id = ?1",
      params![session_id, error],
    )?;
    Ok(())
  }

  pub fn audio_deleted(&self, session_id: &str) -> rusqlite::Result<()> {
    self.conn().execute(
      "UPDATE audio SET state = 'deleted', bytes = 0 WHERE session_id = ?1",
      [session_id],
    )?;
    Ok(())
  }

  pub fn audio_for(&self, session_id: &str) -> rusqlite::Result<Option<AudioRow>> {
    self
      .conn()
      .query_row(
        "SELECT session_id, state, path, bytes, duration_ms, codec, created_at FROM audio WHERE session_id = ?1",
        [session_id],
        |r| {
          Ok(AudioRow {
            session_id: r.get(0)?,
            state: r.get(1)?,
            path: r.get(2)?,
            bytes: r.get(3)?,
            duration_ms: r.get(4)?,
            codec: r.get(5)?,
            created_at: r.get(6)?,
          })
        },
      )
      .optional()
  }

  /// Total bytes of retained audio files.
  pub fn audio_bytes_total(&self) -> rusqlite::Result<i64> {
    self.conn().query_row(
      "SELECT COALESCE(SUM(bytes), 0) FROM audio WHERE state = 'encoded'",
      [],
      |r| r.get(0),
    )
  }

  pub fn session_exists(&self, id: &str) -> rusqlite::Result<bool> {
    self
      .conn()
      .query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)", [id], |r| {
        r.get(0)
      })
  }

  /// A session row for audio found on disk with no matching record (recovered after a crash
  /// that happened before the row was written, or a lost database).
  pub fn create_recovered_session(&self, id: &str, created_at: i64) -> rusqlite::Result<()> {
    self.conn().execute(
      "INSERT OR IGNORE INTO sessions (id, created_at, ended_at, trigger, status, failure, provider, model, language)
       VALUES (?1, ?2, ?2, 'recovery', 'interrupted', 'recovered_audio', 'unknown', 'unknown', 'unknown')",
      params![id, created_at],
    )?;
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::sessions::NewSession;

  #[test]
  fn audio_lifecycle_and_totals() {
    let db = Db::open_in_memory().unwrap();
    db.create_session(&NewSession {
      id: "s",
      trigger: "hotkey",
      provider: "deepgram",
      model: "nova-3",
      language: "en",
      retry_of: None,
    })
    .unwrap();
    db.audio_staging("s", "C:/x/staging/s.pcm").unwrap();
    assert_eq!(db.audio_for("s").unwrap().unwrap().state, state::STAGING);
    db.audio_encoded("s", "C:/x/archive/s.opus", 30_000, 10_000).unwrap();
    let a = db.audio_for("s").unwrap().unwrap();
    assert_eq!(a.state, state::ENCODED);
    assert_eq!(a.codec.as_deref(), Some("opus"));
    assert_eq!(db.audio_bytes_total().unwrap(), 30_000);
    db.audio_deleted("s").unwrap();
    assert_eq!(db.audio_bytes_total().unwrap(), 0);
  }

  #[test]
  fn recovered_session_for_orphan_audio() {
    let db = Db::open_in_memory().unwrap();
    assert!(!db.session_exists("orphan").unwrap());
    db.create_recovered_session("orphan", 1_000).unwrap();
    db.audio_encoded("orphan", "p", 1, 1).unwrap();
    let s = db.session("orphan").unwrap().unwrap();
    assert_eq!(s.status, "interrupted");
    assert_eq!(s.failure.as_deref(), Some("recovered_audio"));
  }

  #[test]
  fn deleting_session_cascades_audio_row() {
    let db = Db::open_in_memory().unwrap();
    db.create_recovered_session("x", 1).unwrap();
    db.audio_staging("x", "p").unwrap();
    db.conn().execute("DELETE FROM sessions WHERE id = 'x'", []).unwrap();
    assert!(db.audio_for("x").unwrap().is_none());
  }
}
