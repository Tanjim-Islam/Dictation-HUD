//! Local SQLite store, owned by Rust. The WebView never sees SQL; it calls typed commands.
//!
//! One connection behind a mutex: writes are small (a handful per dictation) and never happen
//! inside audio callbacks. Schema changes are ordered migrations tracked by `user_version`.

pub mod audio;
pub mod sessions;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;

const MIGRATIONS: &[&str] = &[
  // v1: session spine.
  r#"
  CREATE TABLE sessions (
    id              TEXT PRIMARY KEY,
    created_at      INTEGER NOT NULL,
    ended_at        INTEGER,
    trigger         TEXT NOT NULL,
    status          TEXT NOT NULL,
    failure         TEXT,
    provider        TEXT NOT NULL,
    model           TEXT NOT NULL,
    language        TEXT NOT NULL,
    raw_text        TEXT,
    formatted_text  TEXT,
    final_text      TEXT,
    refine_provider TEXT,
    refine_model    TEXT,
    refine_status   TEXT,
    delivery_status TEXT,
    target_app      TEXT,
    target_title    TEXT,
    target_process  TEXT,
    duration_ms     INTEGER,
    word_count      INTEGER,
    retry_of        TEXT REFERENCES sessions(id) ON DELETE SET NULL
  );
  CREATE INDEX sessions_created ON sessions(created_at DESC);
  CREATE INDEX sessions_status ON sessions(status);

  CREATE TABLE attempts (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id   TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL,
    provider     TEXT NOT NULL,
    model        TEXT NOT NULL,
    language     TEXT NOT NULL,
    started_at   INTEGER NOT NULL,
    ended_at     INTEGER,
    status       TEXT NOT NULL,
    error        TEXT,
    text         TEXT
  );
  CREATE INDEX attempts_session ON attempts(session_id);

  CREATE TABLE segments (
    attempt_id INTEGER NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    text       TEXT NOT NULL,
    PRIMARY KEY (attempt_id, seq)
  );
  "#,
  // v2: local audio for each session (staging while recording, Ogg Opus after).
  r#"
  CREATE TABLE audio (
    session_id  TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    state       TEXT NOT NULL,
    path        TEXT NOT NULL,
    bytes       INTEGER,
    duration_ms INTEGER,
    codec       TEXT,
    error       TEXT,
    created_at  INTEGER NOT NULL,
    encoded_at  INTEGER
  );
  CREATE INDEX audio_state ON audio(state);
  "#,
  // v3: why delivery copied instead of pasting.
  r#"
  ALTER TABLE sessions ADD COLUMN delivery_reason TEXT;
  "#,
];

pub struct Db {
  conn: Mutex<Connection>,
}

impl Db {
  pub fn open(path: &Path) -> rusqlite::Result<Db> {
    if let Some(dir) = path.parent() {
      let _ = std::fs::create_dir_all(dir);
    }
    Self::init(Connection::open(path)?)
  }

  pub fn open_in_memory() -> rusqlite::Result<Db> {
    Self::init(Connection::open_in_memory()?)
  }

  fn init(conn: Connection) -> rusqlite::Result<Db> {
    conn.execute_batch(
      "PRAGMA journal_mode = WAL;
       PRAGMA synchronous = NORMAL;
       PRAGMA foreign_keys = ON;
       PRAGMA busy_timeout = 3000;",
    )?;
    migrate(&conn)?;
    Ok(Db { conn: Mutex::new(conn) })
  }

  pub fn conn(&self) -> MutexGuard<'_, Connection> {
    self.conn.lock().unwrap_or_else(|p| p.into_inner())
  }
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
  let current: usize = conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize;
  for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(sql)?;
    tx.execute_batch(&format!("PRAGMA user_version = {}", i + 1))?;
    tx.commit()?;
    tracing::info!(version = i + 1, "database migrated");
  }
  Ok(())
}

pub fn now_ms() -> i64 {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_millis() as i64)
    .unwrap_or(0)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn migrations_apply_once_and_are_idempotent() {
    let dir = std::env::temp_dir().join(format!("dh-db-{}", uuid::Uuid::new_v4()));
    let path = dir.join("test.db");
    {
      let db = Db::open(&path).unwrap();
      let v: i64 = db.conn().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
      assert_eq!(v as usize, MIGRATIONS.len());
    }
    // Reopen: no re-run, no error.
    let db = Db::open(&path).unwrap();
    let n: i64 = db
      .conn()
      .query_row("SELECT count(*) FROM sqlite_master WHERE name='sessions'", [], |r| {
        r.get(0)
      })
      .unwrap();
    assert_eq!(n, 1);
    drop(db);
    let _ = std::fs::remove_dir_all(dir);
  }
}
