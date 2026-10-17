//! Dictionary and snippet commands. Storage and validation live in `db::lexicon`; the text
//! pipeline reads the active entries on every dictation, so changes apply immediately.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::db::lexicon::{LexError, SnippetRow, Term};
use crate::db::Db;

pub const LEXICON_EVENT: &str = "lexicon://changed";

fn changed(app: &AppHandle) {
  let _ = app.emit(LEXICON_EVENT, ());
}

fn err(e: LexError) -> String {
  if let LexError::Db(inner) = &e {
    tracing::warn!(error = %inner, "dictionary query failed");
  }
  e.to_string()
}

fn db_err(e: rusqlite::Error) -> String {
  err(LexError::Db(e))
}

#[derive(Deserialize)]
pub struct TermInput {
  pub phrase: String,
  #[serde(default)]
  pub heard: Option<String>,
}

#[derive(Deserialize)]
pub struct SnippetInput {
  pub trigger: String,
  pub expansion: String,
}

#[derive(Serialize, Debug, PartialEq, Default)]
pub struct ImportResult {
  pub added: usize,
  pub skipped: usize,
}

/// One import line: `word` (vocabulary) or `misheard -> word` (correction).
pub fn parse_import_line(line: &str) -> Option<(String, Option<String>)> {
  let line = line.trim();
  if line.is_empty() || line.starts_with('#') {
    return None;
  }
  for sep in ["->", "\u{2192}", "=>"] {
    if let Some((heard, phrase)) = line.split_once(sep) {
      return Some((phrase.trim().to_string(), Some(heard.trim().to_string())));
    }
  }
  Some((line.to_string(), None))
}

#[tauri::command]
pub async fn dictionary_list(db: State<'_, Db>) -> Result<Vec<Term>, String> {
  db.terms().map_err(db_err)
}

#[tauri::command]
pub async fn dictionary_add(app: AppHandle, db: State<'_, Db>, input: TermInput) -> Result<Term, String> {
  let t = db
    .add_term(&input.phrase, input.heard.as_deref(), "manual", "active")
    .map_err(err)?;
  changed(&app);
  Ok(t)
}

#[tauri::command]
pub async fn dictionary_update(app: AppHandle, db: State<'_, Db>, id: i64, input: TermInput) -> Result<Term, String> {
  let t = db.update_term(id, &input.phrase, input.heard.as_deref()).map_err(err)?;
  changed(&app);
  Ok(t)
}

#[tauri::command]
pub async fn dictionary_delete(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
  db.delete_term(id).map_err(db_err)?;
  changed(&app);
  Ok(())
}

/// Accepts a suggested entry (from correction learning) so it is applied.
#[tauri::command]
pub async fn dictionary_accept(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
  db.set_term_status(id, "active").map_err(err)?;
  changed(&app);
  Ok(())
}

/// Adds many entries from text, one per line. Invalid lines and duplicates are skipped.
#[tauri::command]
pub async fn dictionary_import(app: AppHandle, db: State<'_, Db>, text: String) -> Result<ImportResult, String> {
  let mut r = ImportResult::default();
  for line in text.lines().take(2_000) {
    let Some((phrase, heard)) = parse_import_line(line) else {
      continue;
    };
    match db.add_term(&phrase, heard.as_deref(), "import", "active") {
      Ok(_) => r.added += 1,
      Err(LexError::Db(e)) => return Err(db_err(e)),
      Err(_) => r.skipped += 1,
    }
  }
  tracing::info!(added = r.added, skipped = r.skipped, "dictionary import");
  changed(&app);
  Ok(r)
}

#[tauri::command]
pub async fn snippets_list(db: State<'_, Db>) -> Result<Vec<SnippetRow>, String> {
  db.snippets().map_err(db_err)
}

#[tauri::command]
pub async fn snippets_add(app: AppHandle, db: State<'_, Db>, input: SnippetInput) -> Result<SnippetRow, String> {
  let s = db.add_snippet(&input.trigger, &input.expansion).map_err(err)?;
  changed(&app);
  Ok(s)
}

#[tauri::command]
pub async fn snippets_update(
  app: AppHandle,
  db: State<'_, Db>,
  id: i64,
  input: SnippetInput,
) -> Result<SnippetRow, String> {
  let s = db.update_snippet(id, &input.trigger, &input.expansion).map_err(err)?;
  changed(&app);
  Ok(s)
}

#[tauri::command]
pub async fn snippets_delete(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
  db.delete_snippet(id).map_err(db_err)?;
  changed(&app);
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn import_lines() {
    assert_eq!(parse_import_line("  WACV "), Some(("WACV".into(), None)));
    assert_eq!(
      parse_import_line("wack v -> WACV"),
      Some(("WACV".into(), Some("wack v".into())))
    );
    assert_eq!(
      parse_import_line("city bank \u{2192} Citibank"),
      Some(("Citibank".into(), Some("city bank".into())))
    );
    assert_eq!(parse_import_line("# comment"), None);
    assert_eq!(parse_import_line("   "), None);
  }
}
