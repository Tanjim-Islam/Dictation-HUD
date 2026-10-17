//! Session repository: the durable record of every dictation.
//!
//! Write order per session: `create` at start, `start_attempt` for each provider stream,
//! `add_segment` for each final segment, `finish_attempt`, `save_processing` (final text is
//! stored before delivery starts), `finalize` with the outcome. `reconcile_interrupted` runs
//! at startup for rows a crash left in progress.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::{now_ms, Db};

pub mod status {
  pub const IN_PROGRESS: &str = "in_progress";
  pub const COMPLETED: &str = "completed";
  pub const CANCELLED: &str = "cancelled";
  pub const FAILED: &str = "failed";
  pub const NO_SPEECH: &str = "no_speech";
  pub const INTERRUPTED: &str = "interrupted";
}

#[derive(Debug, Clone)]
pub struct NewSession<'a> {
  pub id: &'a str,
  pub trigger: &'a str,
  pub provider: &'a str,
  pub model: &'a str,
  pub language: &'a str,
  pub retry_of: Option<&'a str>,
}

#[derive(Debug, Clone, Default)]
pub struct Processing<'a> {
  pub raw_text: &'a str,
  pub formatted_text: &'a str,
  pub final_text: &'a str,
  pub refine_provider: Option<&'a str>,
  pub refine_model: Option<&'a str>,
  /// ok | failed | off
  pub refine_status: &'a str,
}

#[derive(Debug, Clone)]
pub struct Outcome<'a> {
  pub status: &'a str,
  pub failure: Option<&'a str>,
  pub delivery: Option<&'a str>,
  pub delivery_reason: Option<&'a str>,
  pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SessionRow {
  pub id: String,
  pub created_at: i64,
  pub ended_at: Option<i64>,
  pub trigger: String,
  pub status: String,
  pub failure: Option<String>,
  pub provider: String,
  pub model: String,
  pub language: String,
  pub raw_text: Option<String>,
  pub formatted_text: Option<String>,
  pub final_text: Option<String>,
  pub refine_status: Option<String>,
  pub delivery_status: Option<String>,
  pub delivery_reason: Option<String>,
  pub target_app: Option<String>,
  pub duration_ms: Option<i64>,
  pub word_count: Option<i64>,
  pub retry_of: Option<String>,
}

const SESSION_COLS: &str = "id, created_at, ended_at, trigger, status, failure, provider, model, language, raw_text, \
   formatted_text, final_text, refine_status, delivery_status, target_app, duration_ms, word_count, retry_of, delivery_reason";

fn map_session(r: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRow> {
  Ok(SessionRow {
    id: r.get(0)?,
    created_at: r.get(1)?,
    ended_at: r.get(2)?,
    trigger: r.get(3)?,
    status: r.get(4)?,
    failure: r.get(5)?,
    provider: r.get(6)?,
    model: r.get(7)?,
    language: r.get(8)?,
    raw_text: r.get(9)?,
    formatted_text: r.get(10)?,
    final_text: r.get(11)?,
    refine_status: r.get(12)?,
    delivery_status: r.get(13)?,
    target_app: r.get(14)?,
    duration_ms: r.get(15)?,
    word_count: r.get(16)?,
    retry_of: r.get(17)?,
    delivery_reason: r.get(18)?,
  })
}

fn word_count(text: &str) -> i64 {
  text.split_whitespace().count() as i64
}

impl Db {
  pub fn create_session(&self, s: &NewSession<'_>) -> rusqlite::Result<()> {
    self.conn().execute(
      "INSERT INTO sessions (id, created_at, trigger, status, provider, model, language, retry_of)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
      params![
        s.id,
        now_ms(),
        s.trigger,
        status::IN_PROGRESS,
        s.provider,
        s.model,
        s.language,
        s.retry_of
      ],
    )?;
    Ok(())
  }

  pub fn start_attempt(
    &self,
    session_id: &str,
    kind: &str,
    provider: &str,
    model: &str,
    language: &str,
  ) -> rusqlite::Result<i64> {
    let conn = self.conn();
    conn.execute(
      "INSERT INTO attempts (session_id, kind, provider, model, language, started_at, status)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'running')",
      params![session_id, kind, provider, model, language, now_ms()],
    )?;
    Ok(conn.last_insert_rowid())
  }

  /// The running attempt for a session, if any (the most recent one).
  pub fn current_attempt(&self, session_id: &str) -> rusqlite::Result<Option<i64>> {
    self
      .conn()
      .query_row(
        "SELECT id FROM attempts WHERE session_id = ?1 AND status = 'running' ORDER BY id DESC LIMIT 1",
        [session_id],
        |r| r.get(0),
      )
      .optional()
  }

  pub fn add_segment(&self, attempt_id: i64, text: &str) -> rusqlite::Result<()> {
    self.conn().execute(
      "INSERT INTO segments (attempt_id, seq, text)
       VALUES (?1, (SELECT COALESCE(MAX(seq), -1) + 1 FROM segments WHERE attempt_id = ?1), ?2)",
      params![attempt_id, text],
    )?;
    Ok(())
  }

  pub fn segments(&self, attempt_id: i64) -> rusqlite::Result<Vec<String>> {
    let conn = self.conn();
    let mut stmt = conn.prepare("SELECT text FROM segments WHERE attempt_id = ?1 ORDER BY seq")?;
    let rows = stmt.query_map([attempt_id], |r| r.get(0))?;
    rows.collect()
  }

  pub fn finish_attempt(
    &self,
    attempt_id: i64,
    status: &str,
    error: Option<&str>,
    text: Option<&str>,
  ) -> rusqlite::Result<()> {
    self.conn().execute(
      "UPDATE attempts SET status = ?2, error = ?3, text = COALESCE(?4, text), ended_at = ?5 WHERE id = ?1",
      params![attempt_id, status, error, text, now_ms()],
    )?;
    Ok(())
  }

  /// Stores the transcript at every stage. Called before delivery so a crash during delivery
  /// never loses the final text.
  pub fn save_processing(&self, session_id: &str, p: &Processing<'_>) -> rusqlite::Result<()> {
    self.conn().execute(
      "UPDATE sessions SET raw_text = ?2, formatted_text = ?3, final_text = ?4, refine_provider = ?5,
         refine_model = ?6, refine_status = ?7, word_count = ?8, delivery_status = 'pending'
       WHERE id = ?1",
      params![
        session_id,
        p.raw_text,
        p.formatted_text,
        p.final_text,
        p.refine_provider,
        p.refine_model,
        p.refine_status,
        word_count(p.final_text)
      ],
    )?;
    Ok(())
  }

  pub fn set_target(&self, session_id: &str, app: &str, title: &str, process: &str) -> rusqlite::Result<()> {
    self.conn().execute(
      "UPDATE sessions SET target_app = ?2, target_title = ?3, target_process = ?4 WHERE id = ?1",
      params![session_id, app, title, process],
    )?;
    Ok(())
  }

  pub fn set_duration(&self, session_id: &str, duration_ms: i64) -> rusqlite::Result<()> {
    self.conn().execute(
      "UPDATE sessions SET duration_ms = ?2 WHERE id = ?1",
      params![session_id, duration_ms],
    )?;
    Ok(())
  }

  /// Raw transcript only (cancelled or failed sessions keep what was heard).
  pub fn save_raw(&self, session_id: &str, raw: &str) -> rusqlite::Result<()> {
    self.conn().execute(
      "UPDATE sessions SET raw_text = ?2, word_count = COALESCE(word_count, ?3) WHERE id = ?1 AND raw_text IS NULL",
      params![session_id, raw, word_count(raw)],
    )?;
    Ok(())
  }

  pub fn finalize(&self, session_id: &str, o: &Outcome<'_>) -> rusqlite::Result<()> {
    let conn = self.conn();
    conn.execute(
      "UPDATE sessions SET status = ?2, failure = ?3, delivery_status = COALESCE(?4, delivery_status),
         duration_ms = COALESCE(?5, duration_ms), ended_at = ?6, delivery_reason = COALESCE(?7, delivery_reason)
       WHERE id = ?1",
      params![
        session_id,
        o.status,
        o.failure,
        o.delivery,
        o.duration_ms,
        now_ms(),
        o.delivery_reason
      ],
    )?;
    // Close any attempt still running (cancel, failure, shutdown).
    let attempt_status = match o.status {
      status::COMPLETED | status::NO_SPEECH => "ok",
      status::CANCELLED => "cancelled",
      status::INTERRUPTED => "interrupted",
      _ => "failed",
    };
    conn.execute(
      "UPDATE attempts SET status = ?2, error = COALESCE(error, ?3), ended_at = ?4 WHERE session_id = ?1 AND status = 'running'",
      params![session_id, attempt_status, o.failure, now_ms()],
    )?;
    Ok(())
  }

  /// Marks sessions a crash or kill left in progress. Their segments stay so text is not lost.
  /// Returns how many rows were reconciled.
  pub fn reconcile_interrupted(&self) -> rusqlite::Result<usize> {
    let conn = self.conn();
    let tx = conn.unchecked_transaction()?;
    // Rebuild raw text from stored segments where processing never ran.
    tx.execute(
      "UPDATE sessions SET raw_text = (
         SELECT group_concat(s.text, ' ') FROM (
           SELECT seg.text FROM segments seg JOIN attempts a ON a.id = seg.attempt_id
           WHERE a.session_id = sessions.id ORDER BY a.id, seg.seq) s)
       WHERE status = 'in_progress' AND raw_text IS NULL",
      [],
    )?;
    let n = tx.execute(
      "UPDATE sessions SET status = 'interrupted', failure = COALESCE(failure, 'app_closed'), ended_at = COALESCE(ended_at, created_at)
       WHERE status = 'in_progress'",
      [],
    )?;
    tx.execute(
      "UPDATE attempts SET status = 'interrupted', ended_at = COALESCE(ended_at, started_at) WHERE status = 'running'",
      [],
    )?;
    tx.commit()?;
    Ok(n)
  }

  pub fn session(&self, id: &str) -> rusqlite::Result<Option<SessionRow>> {
    let sql = format!("SELECT {SESSION_COLS} FROM sessions WHERE id = ?1");
    self.conn().query_row(&sql, [id], map_session).optional()
  }

  pub fn recent_sessions(&self, limit: i64) -> rusqlite::Result<Vec<SessionRow>> {
    list(&self.conn(), limit)
  }

  /// Most recent delivered or copied transcript, for "Paste last transcript".
  pub fn last_final_text(&self) -> rusqlite::Result<Option<String>> {
    self
      .conn()
      .query_row(
        "SELECT final_text FROM sessions WHERE status = 'completed' AND final_text IS NOT NULL AND final_text != ''
         ORDER BY created_at DESC LIMIT 1",
        [],
        |r| r.get(0),
      )
      .optional()
  }
}

fn list(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<SessionRow>> {
  let sql = format!("SELECT {SESSION_COLS} FROM sessions ORDER BY created_at DESC LIMIT ?1");
  let mut stmt = conn.prepare(&sql)?;
  let rows = stmt.query_map([limit], map_session)?;
  rows.collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn new<'a>(id: &'a str) -> NewSession<'a> {
    NewSession {
      id,
      trigger: "hotkey",
      provider: "deepgram",
      model: "nova-3",
      language: "en",
      retry_of: None,
    }
  }

  #[test]
  fn full_session_lifecycle() {
    let db = Db::open_in_memory().unwrap();
    db.create_session(&new("s1")).unwrap();
    let a = db.start_attempt("s1", "live", "deepgram", "nova-3", "en").unwrap();
    assert_eq!(db.current_attempt("s1").unwrap(), Some(a));
    db.add_segment(a, "hello").unwrap();
    db.add_segment(a, "world").unwrap();
    assert_eq!(db.segments(a).unwrap(), vec!["hello", "world"]);
    db.finish_attempt(a, "ok", None, Some("hello world")).unwrap();
    db.save_processing(
      "s1",
      &Processing {
        raw_text: "hello world",
        formatted_text: "hello world",
        final_text: "Hello world.",
        refine_status: "ok",
        ..Default::default()
      },
    )
    .unwrap();
    // Final text is durable before delivery finishes.
    let mid = db.session("s1").unwrap().unwrap();
    assert_eq!(mid.final_text.as_deref(), Some("Hello world."));
    assert_eq!(mid.delivery_status.as_deref(), Some("pending"));
    assert_eq!(mid.status, status::IN_PROGRESS);
    db.finalize(
      "s1",
      &Outcome {
        status: status::COMPLETED,
        failure: None,
        delivery: Some("pasted"),
        delivery_reason: None,
        duration_ms: Some(2_500),
      },
    )
    .unwrap();
    let row = db.session("s1").unwrap().unwrap();
    assert_eq!(row.status, status::COMPLETED);
    assert_eq!(row.delivery_status.as_deref(), Some("pasted"));
    assert_eq!(row.word_count, Some(2));
    assert!(row.ended_at.is_some());
    assert_eq!(db.last_final_text().unwrap().as_deref(), Some("Hello world."));
  }

  #[test]
  fn cancel_closes_running_attempt_and_keeps_raw() {
    let db = Db::open_in_memory().unwrap();
    db.create_session(&new("s2")).unwrap();
    let a = db.start_attempt("s2", "live", "deepgram", "nova-3", "en").unwrap();
    db.add_segment(a, "partial words").unwrap();
    db.save_raw("s2", "partial words").unwrap();
    db.finalize(
      "s2",
      &Outcome {
        status: status::CANCELLED,
        failure: None,
        delivery: None,
        delivery_reason: None,
        duration_ms: None,
      },
    )
    .unwrap();
    let row = db.session("s2").unwrap().unwrap();
    assert_eq!(row.status, status::CANCELLED);
    assert_eq!(row.raw_text.as_deref(), Some("partial words"));
    assert_eq!(db.current_attempt("s2").unwrap(), None);
    assert_eq!(
      db.last_final_text().unwrap(),
      None,
      "cancelled sessions are not pasteable"
    );
  }

  #[test]
  fn reconcile_marks_interrupted_and_rebuilds_text() {
    let dir = std::env::temp_dir().join(format!("dh-recon-{}", uuid::Uuid::new_v4()));
    let path = dir.join("h.db");
    {
      // First run: crash mid-session (no finalize).
      let db = Db::open(&path).unwrap();
      db.create_session(&new("crashed")).unwrap();
      let a = db.start_attempt("crashed", "live", "deepgram", "nova-3", "en").unwrap();
      db.add_segment(a, "important").unwrap();
      db.add_segment(a, "words").unwrap();
      db.create_session(&new("done")).unwrap();
      db.finalize(
        "done",
        &Outcome {
          status: status::COMPLETED,
          failure: None,
          delivery: Some("pasted"),
          delivery_reason: None,
          duration_ms: None,
        },
      )
      .unwrap();
    }
    // Restart.
    let db = Db::open(&path).unwrap();
    assert_eq!(db.reconcile_interrupted().unwrap(), 1);
    let row = db.session("crashed").unwrap().unwrap();
    assert_eq!(row.status, status::INTERRUPTED);
    assert_eq!(row.failure.as_deref(), Some("app_closed"));
    assert_eq!(row.raw_text.as_deref(), Some("important words"));
    assert_eq!(db.session("done").unwrap().unwrap().status, status::COMPLETED);
    // Idempotent.
    assert_eq!(db.reconcile_interrupted().unwrap(), 0);
    drop(db);
    let _ = std::fs::remove_dir_all(dir);
  }

  #[test]
  fn retry_lineage_and_listing_order() {
    let db = Db::open_in_memory().unwrap();
    db.create_session(&new("orig")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(3));
    db.create_session(&NewSession {
      retry_of: Some("orig"),
      ..new("retry")
    })
    .unwrap();
    let rows = db.recent_sessions(10).unwrap();
    assert_eq!(rows[0].id, "retry");
    assert_eq!(rows[0].retry_of.as_deref(), Some("orig"));
    assert_eq!(rows[1].id, "orig");
  }

  #[test]
  fn unicode_text_round_trips() {
    let db = Db::open_in_memory().unwrap();
    db.create_session(&new("bn")).unwrap();
    let bengali = "আমি বাংলায় কথা বলছি।";
    db.save_processing(
      "bn",
      &Processing {
        raw_text: bengali,
        formatted_text: bengali,
        final_text: bengali,
        refine_status: "off",
        ..Default::default()
      },
    )
    .unwrap();
    assert_eq!(db.session("bn").unwrap().unwrap().final_text.as_deref(), Some(bengali));
  }
}
