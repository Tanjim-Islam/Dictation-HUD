//! History queries and edits: listing with search and filters, attempt lineage, text updates
//! from retries and recovery, deletion, retention candidates, storage totals.
//!
//! Retries never change an earlier attempt. Each retry is a new attempt whose `parent_id`
//! points at the attempt it replaces, and the session's text follows the newest one.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::sessions::{map_session, SessionRow, SESSION_COLS};
use super::{now_ms, Db};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
  #[default]
  All,
  /// Interrupted or failed sessions, and transcripts that could not be delivered.
  NeedsAttention,
  /// Sessions that still have a local recording.
  WithAudio,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct HistoryQuery {
  pub search: Option<String>,
  pub filter: Filter,
  /// Cursor: only sessions created before this time (ms).
  pub before: Option<i64>,
  pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HistoryItem {
  #[serde(flatten)]
  pub session: SessionRow,
  /// `encoded`, `staging`, `failed`, `deleted`, or none.
  pub audio_state: Option<String>,
  pub audio_ms: Option<i64>,
  pub audio_bytes: Option<i64>,
  /// Status before a retry or recovery fixed the session.
  pub recovered_from: Option<String>,
  pub attempts: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AttemptRow {
  pub id: i64,
  pub kind: String,
  pub provider: String,
  pub model: String,
  pub language: String,
  pub started_at: i64,
  pub ended_at: Option<i64>,
  pub status: String,
  pub error: Option<String>,
  pub text: Option<String>,
  pub parent_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct StorageStats {
  pub sessions: i64,
  pub audio_files: i64,
  pub audio_bytes: i64,
}

/// Text produced by a retry, recovery, or refinement retry.
#[derive(Debug, Clone, Default)]
pub struct TextUpdate<'a> {
  pub raw_text: &'a str,
  pub formatted_text: &'a str,
  pub final_text: &'a str,
  pub refine_provider: Option<&'a str>,
  pub refine_model: Option<&'a str>,
  pub refine_status: &'a str,
}

const MAX_PAGE: i64 = 200;

fn prefixed_session_cols() -> String {
  SESSION_COLS
    .split(',')
    .map(|c| format!("s.{}", c.trim()))
    .collect::<Vec<_>>()
    .join(", ")
}

/// `LIKE` pattern matching `term` literally (escapes `%`, `_`, and the escape char).
fn like_pattern(term: &str) -> String {
  let mut out = String::with_capacity(term.len() + 2);
  out.push('%');
  for c in term.chars() {
    if matches!(c, '%' | '_' | '\\') {
      out.push('\\');
    }
    out.push(c);
  }
  out.push('%');
  out
}

impl Db {
  pub fn history(&self, q: &HistoryQuery) -> rusqlite::Result<Vec<HistoryItem>> {
    let n = prefixed_session_cols().split(", ").count();
    let mut sql = format!(
      "SELECT {}, a.state, a.duration_ms, a.bytes, s.recovered_from,
         (SELECT count(*) FROM attempts t WHERE t.session_id = s.id)
       FROM sessions s LEFT JOIN audio a ON a.session_id = s.id WHERE s.created_at < ?1",
      prefixed_session_cols()
    );
    match q.filter {
      Filter::All => {}
      Filter::NeedsAttention => {
        sql.push_str(" AND (s.status IN ('interrupted', 'failed') OR s.delivery_status = 'failed')")
      }
      Filter::WithAudio => sql.push_str(" AND a.state = 'encoded'"),
    }
    let search = q
      .search
      .as_deref()
      .map(str::trim)
      .filter(|t| !t.is_empty())
      .map(like_pattern);
    if search.is_some() {
      sql.push_str(
        " AND (s.final_text LIKE ?3 ESCAPE '\\' OR s.raw_text LIKE ?3 ESCAPE '\\' OR s.target_app LIKE ?3 ESCAPE '\\')",
      );
    } else {
      sql.push_str(" AND ?3 IS NULL");
    }
    sql.push_str(" ORDER BY s.created_at DESC LIMIT ?2");
    let conn = self.conn();
    let mut stmt = conn.prepare(&sql)?;
    let limit = q.limit.unwrap_or(60).clamp(1, MAX_PAGE);
    let before = q.before.unwrap_or(i64::MAX);
    let rows = stmt.query_map(params![before, limit, search], |r| {
      Ok(HistoryItem {
        session: map_session(r)?,
        audio_state: r.get(n)?,
        audio_ms: r.get(n + 1)?,
        audio_bytes: r.get(n + 2)?,
        recovered_from: r.get(n + 3)?,
        attempts: r.get(n + 4)?,
      })
    })?;
    rows.collect()
  }

  pub fn history_item(&self, id: &str) -> rusqlite::Result<Option<HistoryItem>> {
    let n = prefixed_session_cols().split(", ").count();
    let sql = format!(
      "SELECT {}, a.state, a.duration_ms, a.bytes, s.recovered_from,
         (SELECT count(*) FROM attempts t WHERE t.session_id = s.id)
       FROM sessions s LEFT JOIN audio a ON a.session_id = s.id WHERE s.id = ?1",
      prefixed_session_cols()
    );
    self
      .conn()
      .query_row(&sql, [id], |r| {
        Ok(HistoryItem {
          session: map_session(r)?,
          audio_state: r.get(n)?,
          audio_ms: r.get(n + 1)?,
          audio_bytes: r.get(n + 2)?,
          recovered_from: r.get(n + 3)?,
          attempts: r.get(n + 4)?,
        })
      })
      .optional()
  }

  pub fn attempts(&self, session_id: &str) -> rusqlite::Result<Vec<AttemptRow>> {
    let conn = self.conn();
    let mut stmt = conn.prepare(
      "SELECT id, kind, provider, model, language, started_at, ended_at, status, error, text, parent_id
       FROM attempts WHERE session_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([session_id], |r| {
      Ok(AttemptRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        provider: r.get(2)?,
        model: r.get(3)?,
        language: r.get(4)?,
        started_at: r.get(5)?,
        ended_at: r.get(6)?,
        status: r.get(7)?,
        error: r.get(8)?,
        text: r.get(9)?,
        parent_id: r.get(10)?,
      })
    })?;
    rows.collect()
  }

  /// A new attempt that replaces the session's latest one (lineage via `parent_id`).
  pub fn start_retry_attempt(
    &self,
    session_id: &str,
    provider: &str,
    model: &str,
    language: &str,
  ) -> rusqlite::Result<i64> {
    let conn = self.conn();
    let parent: Option<i64> = conn
      .query_row(
        "SELECT id FROM attempts WHERE session_id = ?1 ORDER BY id DESC LIMIT 1",
        [session_id],
        |r| r.get(0),
      )
      .optional()?;
    conn.execute(
      "INSERT INTO attempts (session_id, kind, provider, model, language, started_at, status, parent_id)
       VALUES (?1, 'retry', ?2, ?3, ?4, ?5, 'running', ?6)",
      params![session_id, provider, model, language, now_ms(), parent],
    )?;
    Ok(conn.last_insert_rowid())
  }

  /// Replaces the session's current text (retry, recovery, refinement retry). Delivery state
  /// is left alone: nothing is pasted from History.
  pub fn update_text(&self, session_id: &str, t: &TextUpdate<'_>) -> rusqlite::Result<()> {
    self.conn().execute(
      "UPDATE sessions SET raw_text = ?2, formatted_text = ?3, final_text = ?4, refine_provider = ?5,
         refine_model = ?6, refine_status = ?7, word_count = ?8
       WHERE id = ?1",
      params![
        session_id,
        t.raw_text,
        t.formatted_text,
        t.final_text,
        t.refine_provider,
        t.refine_model,
        t.refine_status,
        t.final_text.split_whitespace().count() as i64
      ],
    )?;
    Ok(())
  }

  /// After a successful retry or recovery: the session now has usable text. Remembers what it
  /// was before so History can say "Recovered".
  pub fn mark_recovered(&self, session_id: &str) -> rusqlite::Result<bool> {
    let n = self.conn().execute(
      "UPDATE sessions SET recovered_from = status, status = 'completed',
         delivery_status = CASE WHEN delivery_status IS NULL OR delivery_status = 'pending' THEN 'none'
                                ELSE delivery_status END
       WHERE id = ?1 AND status IN ('interrupted', 'failed', 'no_speech', 'cancelled')",
      [session_id],
    )?;
    Ok(n > 0)
  }

  /// Deletes a session with its attempts, segments, and audio row (cascade). The audio file
  /// is removed by the caller first.
  pub fn delete_session(&self, session_id: &str) -> rusqlite::Result<bool> {
    let n = self
      .conn()
      .execute("DELETE FROM sessions WHERE id = ?1", [session_id])?;
    Ok(n > 0)
  }

  /// Retained recordings created before `cutoff_ms` (encoded, or failed with raw audio kept).
  /// Staging rows are excluded: they belong to a live recording or a pending recovery.
  pub fn audio_expired(&self, cutoff_ms: i64) -> rusqlite::Result<Vec<String>> {
    let conn = self.conn();
    let mut stmt = conn.prepare(
      "SELECT session_id FROM audio WHERE state IN ('encoded', 'failed') AND created_at < ?1 ORDER BY created_at",
    )?;
    let rows = stmt.query_map([cutoff_ms], |r| r.get(0))?;
    rows.collect()
  }

  pub fn storage_stats(&self) -> rusqlite::Result<StorageStats> {
    self.conn().query_row(
      "SELECT (SELECT count(*) FROM sessions),
              (SELECT count(*) FROM audio WHERE state IN ('encoded', 'failed')),
              (SELECT COALESCE(SUM(bytes), 0) FROM audio WHERE state = 'encoded')",
      [],
      |r| {
        Ok(StorageStats {
          sessions: r.get(0)?,
          audio_files: r.get(1)?,
          audio_bytes: r.get(2)?,
        })
      },
    )
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::sessions::{status, NewSession, Outcome, Processing};

  fn session(db: &Db, id: &str, created_at: i64) {
    db.create_session(&NewSession {
      id,
      trigger: "hotkey",
      provider: "deepgram",
      model: "nova-3",
      language: "en",
      retry_of: None,
    })
    .unwrap();
    db.conn()
      .execute(
        "UPDATE sessions SET created_at = ?2 WHERE id = ?1",
        params![id, created_at],
      )
      .unwrap();
  }

  fn finish(db: &Db, id: &str, st: &str, text: &str, app: &str) {
    db.save_processing(
      id,
      &Processing {
        raw_text: text,
        formatted_text: text,
        final_text: text,
        refine_status: "off",
        ..Default::default()
      },
    )
    .unwrap();
    db.set_target(id, app, "", "").unwrap();
    db.finalize(
      id,
      &Outcome {
        status: st,
        failure: (st == status::FAILED).then_some("network"),
        delivery: (st == status::COMPLETED).then_some("pasted"),
        delivery_reason: None,
        duration_ms: Some(1_000),
      },
    )
    .unwrap();
  }

  #[test]
  fn search_filters_and_cursor() {
    let db = Db::open_in_memory().unwrap();
    session(&db, "a", 1_000);
    finish(&db, "a", status::COMPLETED, "hello world", "Notepad");
    session(&db, "b", 2_000);
    finish(&db, "b", status::FAILED, "fifty_percent% done", "VS Code");
    session(&db, "c", 3_000);
    finish(&db, "c", status::COMPLETED, "another note", "Slack");
    db.audio_encoded("c", "p", 10, 10).unwrap();

    let all = db.history(&HistoryQuery::default()).unwrap();
    assert_eq!(
      all.iter().map(|i| i.session.id.as_str()).collect::<Vec<_>>(),
      ["c", "b", "a"]
    );
    assert_eq!(all[0].audio_state.as_deref(), Some("encoded"));
    assert_eq!(all[0].attempts, 0);

    let q = |search: &str| HistoryQuery {
      search: Some(search.into()),
      ..Default::default()
    };
    assert_eq!(db.history(&q("WORLD")).unwrap().len(), 1, "case-insensitive");
    assert_eq!(db.history(&q("slack")).unwrap()[0].session.id, "c", "matches app");
    // `%` and `_` are literal, not wildcards.
    assert_eq!(db.history(&q("%")).unwrap().len(), 1);
    assert_eq!(db.history(&q("y_p")).unwrap().len(), 1);
    assert_eq!(db.history(&q("o_w")).unwrap().len(), 0);
    assert_eq!(db.history(&q("   ")).unwrap().len(), 3, "blank search lists all");

    let attention = db
      .history(&HistoryQuery {
        filter: Filter::NeedsAttention,
        ..Default::default()
      })
      .unwrap();
    assert_eq!(attention.len(), 1);
    assert_eq!(attention[0].session.id, "b");
    let audio = db
      .history(&HistoryQuery {
        filter: Filter::WithAudio,
        ..Default::default()
      })
      .unwrap();
    assert_eq!(audio.len(), 1);

    let page = db
      .history(&HistoryQuery {
        before: Some(3_000),
        limit: Some(1),
        ..Default::default()
      })
      .unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].session.id, "b");
  }

  #[test]
  fn retry_adds_attempt_with_lineage_and_keeps_the_old_one() {
    let db = Db::open_in_memory().unwrap();
    session(&db, "s", 1);
    let live = db.start_attempt("s", "live", "deepgram", "nova-3", "en").unwrap();
    db.finish_attempt(live, "failed", Some("network"), Some("helo wrld"))
      .unwrap();
    finish(&db, "s", status::FAILED, "helo wrld", "Notepad");

    let retry = db.start_retry_attempt("s", "elevenlabs", "scribe", "en").unwrap();
    db.finish_attempt(retry, "ok", None, Some("hello world")).unwrap();
    db.update_text(
      "s",
      &TextUpdate {
        raw_text: "hello world",
        formatted_text: "hello world",
        final_text: "Hello world.",
        refine_status: "off",
        ..Default::default()
      },
    )
    .unwrap();
    assert!(db.mark_recovered("s").unwrap());
    assert!(!db.mark_recovered("s").unwrap(), "only once");

    let attempts = db.attempts("s").unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].text.as_deref(), Some("helo wrld"), "old attempt untouched");
    assert_eq!(attempts[0].status, "failed");
    assert_eq!(attempts[1].kind, "retry");
    assert_eq!(attempts[1].parent_id, Some(live));
    let item = db.history_item("s").unwrap().unwrap();
    assert_eq!(item.session.status, status::COMPLETED);
    assert_eq!(item.recovered_from.as_deref(), Some(status::FAILED));
    assert_eq!(item.session.final_text.as_deref(), Some("Hello world."));
    assert_eq!(item.session.word_count, Some(2));
    assert_eq!(item.attempts, 2);
  }

  #[test]
  fn retention_candidates_and_deletion() {
    let db = Db::open_in_memory().unwrap();
    for (id, at) in [("old", 1_000), ("new", 9_000), ("rec", 500)] {
      session(&db, id, at);
    }
    db.audio_encoded("old", "p", 5, 5).unwrap();
    db.audio_encoded("new", "p", 7, 5).unwrap();
    db.audio_staging("rec", "p").unwrap();
    for (id, at) in [("old", 1_000), ("new", 9_000), ("rec", 500)] {
      db.conn()
        .execute(
          "UPDATE audio SET created_at = ?2 WHERE session_id = ?1",
          params![id, at],
        )
        .unwrap();
    }
    assert_eq!(db.audio_expired(5_000).unwrap(), vec!["old".to_string()]);
    let stats = db.storage_stats().unwrap();
    assert_eq!(stats.audio_files, 2);
    assert_eq!(stats.audio_bytes, 12);
    assert!(db.delete_session("old").unwrap());
    assert!(db.audio_for("old").unwrap().is_none());
    assert_eq!(db.storage_stats().unwrap().sessions, 2);
  }
}
