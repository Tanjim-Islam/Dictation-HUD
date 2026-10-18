//! Where recordings live and how leftovers are recovered.
//!
//! `<root>/staging/<session>.pcm` while recording, `<root>/archive/<session>.opus` after.
//! All paths are derived from a validated session id and checked to stay inside `root`.

use std::path::{Path, PathBuf};

use crate::db::Db;

use super::recording::{finalize_staging, RecordingError};

#[derive(Debug, Clone)]
pub struct RecordingsDir {
  root: PathBuf,
}

/// Session ids are UUIDs; anything else is rejected before touching the file system.
fn valid_id(id: &str) -> bool {
  uuid::Uuid::parse_str(id).is_ok()
}

impl RecordingsDir {
  pub fn new(root: impl Into<PathBuf>) -> Self {
    Self { root: root.into() }
  }

  pub fn root(&self) -> &Path {
    &self.root
  }

  pub fn staging_path(&self, id: &str) -> Option<PathBuf> {
    valid_id(id).then(|| self.root.join("staging").join(format!("{id}.pcm")))
  }

  pub fn archive_path(&self, id: &str) -> Option<PathBuf> {
    valid_id(id).then(|| self.root.join("archive").join(format!("{id}.opus")))
  }

  /// True only for an existing path strictly inside the recordings root.
  pub fn contains(&self, path: &Path) -> bool {
    let (Ok(root), Ok(p)) = (self.root.canonicalize(), path.canonicalize()) else {
      return false;
    };
    p.starts_with(&root) && p != root
  }

  /// Encodes one staging file and records the result. The staging file stays on failure.
  pub fn finalize(&self, db: &Db, id: &str) -> Result<i64, RecordingError> {
    let staging = self.staging_path(id).ok_or(RecordingError::Empty)?;
    let archive = self.archive_path(id).ok_or(RecordingError::Empty)?;
    match finalize_staging(&staging, &archive) {
      Ok(info) => {
        let _ = db.audio_encoded(id, &archive.to_string_lossy(), info.bytes as i64, info.duration_ms());
        tracing::info!(
          session = id,
          bytes = info.bytes,
          ms = info.duration_ms(),
          "recording encoded"
        );
        Ok(info.duration_ms())
      }
      Err(RecordingError::Empty) => {
        let _ = db.audio_failed(id, "empty");
        Err(RecordingError::Empty)
      }
      Err(e) => {
        let _ = db.audio_failed(id, &e.to_string());
        tracing::error!(session = id, error = %e, "recording encode failed; raw audio kept");
        Err(e)
      }
    }
  }

  /// Deletes a session's recording files and marks its audio row deleted. Only files derived
  /// from the validated id, or the stored path when it is inside the root, are removed.
  /// Returns the bytes freed.
  pub fn remove(&self, db: &Db, id: &str) -> std::io::Result<u64> {
    let (Some(staging), Some(archive)) = (self.staging_path(id), self.archive_path(id)) else {
      return Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "invalid session id",
      ));
    };
    let mut paths = vec![archive, staging];
    if let Ok(Some(row)) = db.audio_for(id) {
      let stored = PathBuf::from(&row.path);
      if !paths.contains(&stored) {
        paths.push(stored);
      }
    }
    let mut freed = 0;
    for p in paths {
      if !p.exists() {
        continue;
      }
      if !self.contains(&p) {
        tracing::warn!(
          session = id,
          "refusing to delete a recording path outside the recordings folder"
        );
        continue;
      }
      freed += std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
      std::fs::remove_file(&p)?;
    }
    let _ = db.audio_deleted(id);
    Ok(freed)
  }

  /// Startup sweep: every staging file left behind (crash, kill, power loss) is encoded and
  /// attached to its session, creating a recovered session row if none exists. Returns the
  /// number of recordings recovered.
  pub fn recover(&self, db: &Db) -> usize {
    let Ok(entries) = std::fs::read_dir(self.root.join("staging")) else {
      return 0;
    };
    let mut recovered = 0;
    for entry in entries.flatten() {
      let path = entry.path();
      if path.extension().and_then(|e| e.to_str()) != Some("pcm") {
        continue;
      }
      let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
        continue;
      };
      if !valid_id(&id) {
        continue;
      }
      let created = entry
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or_else(crate::db::now_ms);
      if !db.session_exists(&id).unwrap_or(false) {
        let _ = db.create_recovered_session(&id, created);
      }
      let _ = db.audio_staging(&id, &path.to_string_lossy());
      match self.finalize(db, &id) {
        Ok(ms) => {
          let _ = db.set_duration(&id, ms);
          recovered += 1;
        }
        Err(RecordingError::Empty) => {}
        Err(_) => {}
      }
    }
    recovered
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::db::sessions::NewSession;

  fn root() -> PathBuf {
    std::env::temp_dir().join(format!("dh-store-{}", uuid::Uuid::new_v4()))
  }

  #[test]
  fn paths_reject_non_uuid_ids() {
    let d = RecordingsDir::new(root());
    assert!(d.staging_path("../../Windows/system32").is_none());
    assert!(d.archive_path("abc").is_none());
    let id = uuid::Uuid::new_v4().to_string();
    assert!(d
      .archive_path(&id)
      .unwrap()
      .ends_with(format!("archive/{id}.opus").replace('/', std::path::MAIN_SEPARATOR_STR)));
  }

  #[test]
  fn contains_blocks_paths_outside_root() {
    let r = root();
    let d = RecordingsDir::new(&r);
    std::fs::create_dir_all(r.join("archive")).unwrap();
    let inside = r.join("archive").join("x.opus");
    std::fs::write(&inside, b"x").unwrap();
    assert!(d.contains(&inside));
    let outside = std::env::temp_dir().join(format!("dh-outside-{}", uuid::Uuid::new_v4()));
    std::fs::write(&outside, b"x").unwrap();
    assert!(!d.contains(&outside));
    assert!(!d.contains(
      &r.join("archive")
        .join("..")
        .join("..")
        .join(outside.file_name().unwrap())
    ));
    assert!(!d.contains(&r), "the root itself is never a deletable item");
    let _ = std::fs::remove_file(outside);
    let _ = std::fs::remove_dir_all(r);
  }

  #[test]
  fn remove_deletes_only_inside_root() {
    let r = root();
    let d = RecordingsDir::new(&r);
    let db = Db::open_in_memory().unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    db.create_recovered_session(&id, 1).unwrap();
    std::fs::create_dir_all(r.join("archive")).unwrap();
    let archive = d.archive_path(&id).unwrap();
    std::fs::write(&archive, b"opus").unwrap();
    // A tampered row pointing outside the recordings folder must not delete that file.
    let outside = std::env::temp_dir().join(format!("dh-keep-{}", uuid::Uuid::new_v4()));
    std::fs::write(&outside, b"important").unwrap();
    db.audio_encoded(&id, &outside.to_string_lossy(), 4, 1).unwrap();
    assert_eq!(d.remove(&db, &id).unwrap(), 4);
    assert!(!archive.exists());
    assert!(outside.exists(), "file outside the root survives");
    assert_eq!(db.audio_for(&id).unwrap().unwrap().state, "deleted");
    assert!(d.remove(&db, "../../etc").is_err());
    let _ = std::fs::remove_file(outside);
    let _ = std::fs::remove_dir_all(r);
  }

  #[test]
  fn recover_encodes_orphans_and_known_sessions() {
    let r = root();
    let d = RecordingsDir::new(&r);
    let db = Db::open_in_memory().unwrap();
    let known = uuid::Uuid::new_v4().to_string();
    let orphan = uuid::Uuid::new_v4().to_string();
    db.create_session(&NewSession {
      id: &known,
      trigger: "hotkey",
      provider: "deepgram",
      model: "nova-3",
      language: "en",
      retry_of: None,
    })
    .unwrap();
    std::fs::create_dir_all(r.join("staging")).unwrap();
    let pcm: Vec<u8> = (0..16_000)
      .flat_map(|i| ((i % 200) as i16 * 50).to_le_bytes())
      .collect();
    std::fs::write(d.staging_path(&known).unwrap(), &pcm).unwrap();
    std::fs::write(d.staging_path(&orphan).unwrap(), &pcm).unwrap();
    std::fs::write(r.join("staging").join("not-a-session.pcm"), &pcm).unwrap();
    assert_eq!(d.recover(&db), 2);
    for id in [&known, &orphan] {
      let a = db.audio_for(id).unwrap().unwrap();
      assert_eq!(a.state, "encoded");
      assert!(d.archive_path(id).unwrap().exists());
      assert!(!d.staging_path(id).unwrap().exists());
    }
    assert_eq!(
      db.session(&orphan).unwrap().unwrap().failure.as_deref(),
      Some("recovered_audio")
    );
    assert!(
      r.join("staging").join("not-a-session.pcm").exists(),
      "unknown files are left alone"
    );
    let _ = std::fs::remove_dir_all(r);
  }
}
