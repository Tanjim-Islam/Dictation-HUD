//! Personal dictionary (vocabulary and corrections) and snippets.
//!
//! A dictionary row with only `phrase` is vocabulary: a preferred spelling sent to the speech
//! provider as a hint, given to AI refinement, and used to restore casing. A row with `heard`
//! is also a correction: that text is replaced by `phrase`. `status = 'suggested'` rows come
//! from correction learning and are not applied until accepted.

use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::{now_ms, Db};
use crate::textproc::{Correction, Lexicon, Snippet};

pub const MAX_PHRASE: usize = 80;
pub const MAX_TRIGGER: usize = 60;
pub const MAX_EXPANSION: usize = 4_000;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Term {
  pub id: i64,
  pub phrase: String,
  pub heard: Option<String>,
  /// manual | learned | import
  pub source: String,
  /// active | suggested
  pub status: String,
  pub uses: i64,
  pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SnippetRow {
  pub id: i64,
  pub trigger: String,
  pub expansion: String,
  pub uses: i64,
  pub created_at: i64,
}

#[derive(Debug)]
pub enum LexError {
  Invalid(&'static str),
  Duplicate,
  NotFound,
  Db(rusqlite::Error),
}

impl std::fmt::Display for LexError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      LexError::Invalid(why) => write!(f, "{why}"),
      LexError::Duplicate => write!(f, "That entry already exists."),
      LexError::NotFound => write!(f, "That entry no longer exists."),
      LexError::Db(_) => write!(f, "The dictionary is unavailable right now."),
    }
  }
}

impl From<rusqlite::Error> for LexError {
  fn from(e: rusqlite::Error) -> Self {
    LexError::Db(e)
  }
}

fn collapse(s: &str) -> String {
  s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Trimmed, single-spaced phrase with at least one letter or digit.
pub fn clean_phrase(s: &str, what: &'static str) -> Result<String, LexError> {
  let p = collapse(s);
  if p.is_empty() || !p.chars().any(char::is_alphanumeric) {
    return Err(LexError::Invalid(what));
  }
  if p.chars().count() > MAX_PHRASE {
    return Err(LexError::Invalid("Keep entries under 80 characters."));
  }
  Ok(p)
}

fn clean_heard(heard: Option<&str>, phrase: &str) -> Result<Option<String>, LexError> {
  let Some(h) = heard.map(collapse).filter(|h| !h.is_empty()) else {
    return Ok(None);
  };
  let h = clean_phrase(&h, "Enter what you hear instead.")?;
  if h.to_lowercase() == phrase.to_lowercase() {
    return Err(LexError::Invalid("The misheard text must differ from the word."));
  }
  Ok(Some(h))
}

pub fn clean_trigger(s: &str) -> Result<String, LexError> {
  let t = collapse(s);
  if t.chars().filter(|c| c.is_alphabetic()).count() < 2 {
    return Err(LexError::Invalid("A trigger needs at least two letters."));
  }
  if t.chars().count() > MAX_TRIGGER {
    return Err(LexError::Invalid("Keep triggers under 60 characters."));
  }
  Ok(t)
}

pub fn clean_expansion(s: &str) -> Result<String, LexError> {
  // Exact text: only surrounding blank lines are removed.
  let e = s.trim_matches(|c| c == '\n' || c == '\r');
  if e.trim().is_empty() {
    return Err(LexError::Invalid("Enter the text to insert."));
  }
  if e.chars().count() > MAX_EXPANSION {
    return Err(LexError::Invalid("Keep snippets under 4,000 characters."));
  }
  Ok(e.replace("\r\n", "\n"))
}

fn map_term(r: &rusqlite::Row<'_>) -> rusqlite::Result<Term> {
  Ok(Term {
    id: r.get(0)?,
    phrase: r.get(1)?,
    heard: r.get(2)?,
    source: r.get(3)?,
    status: r.get(4)?,
    uses: r.get(5)?,
    created_at: r.get(6)?,
  })
}

fn map_snippet(r: &rusqlite::Row<'_>) -> rusqlite::Result<SnippetRow> {
  Ok(SnippetRow {
    id: r.get(0)?,
    trigger: r.get(1)?,
    expansion: r.get(2)?,
    uses: r.get(3)?,
    created_at: r.get(4)?,
  })
}

const TERM_COLS: &str = "id, phrase, heard, source, status, uses, created_at";
const SNIPPET_COLS: &str = "id, trigger, expansion, uses, created_at";

impl Db {
  pub fn terms(&self) -> rusqlite::Result<Vec<Term>> {
    let conn = self.conn();
    let mut stmt = conn.prepare(&format!(
      "SELECT {TERM_COLS} FROM dictionary ORDER BY status = 'suggested' DESC, created_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map([], map_term)?;
    rows.collect()
  }

  pub fn term(&self, id: i64) -> rusqlite::Result<Option<Term>> {
    self
      .conn()
      .query_row(
        &format!("SELECT {TERM_COLS} FROM dictionary WHERE id = ?1"),
        [id],
        map_term,
      )
      .optional()
  }

  /// Unicode-aware duplicate check (SQLite's lower() only folds ASCII).
  fn term_exists(&self, phrase: &str, heard: Option<&str>, except: Option<i64>) -> rusqlite::Result<bool> {
    let key = (phrase.to_lowercase(), heard.map(str::to_lowercase));
    Ok(
      self
        .terms()?
        .iter()
        .any(|t| Some(t.id) != except && (t.phrase.to_lowercase(), t.heard.as_deref().map(str::to_lowercase)) == key),
    )
  }

  pub fn add_term(&self, phrase: &str, heard: Option<&str>, source: &str, status: &str) -> Result<Term, LexError> {
    let phrase = clean_phrase(phrase, "Enter a word or phrase.")?;
    let heard = clean_heard(heard, &phrase)?;
    if self.term_exists(&phrase, heard.as_deref(), None)? {
      return Err(LexError::Duplicate);
    }
    let now = now_ms();
    let id = {
      let conn = self.conn();
      conn.execute(
        "INSERT INTO dictionary (phrase, heard, source, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![phrase, heard, source, status, now],
      )?;
      conn.last_insert_rowid()
    };
    self.term(id)?.ok_or(LexError::NotFound)
  }

  pub fn update_term(&self, id: i64, phrase: &str, heard: Option<&str>) -> Result<Term, LexError> {
    let phrase = clean_phrase(phrase, "Enter a word or phrase.")?;
    let heard = clean_heard(heard, &phrase)?;
    if self.term_exists(&phrase, heard.as_deref(), Some(id))? {
      return Err(LexError::Duplicate);
    }
    let n = self.conn().execute(
      "UPDATE dictionary SET phrase = ?2, heard = ?3, updated_at = ?4 WHERE id = ?1",
      params![id, phrase, heard, now_ms()],
    )?;
    if n == 0 {
      return Err(LexError::NotFound);
    }
    self.term(id)?.ok_or(LexError::NotFound)
  }

  pub fn set_term_status(&self, id: i64, status: &str) -> Result<(), LexError> {
    let n = self.conn().execute(
      "UPDATE dictionary SET status = ?2, updated_at = ?3 WHERE id = ?1",
      params![id, status, now_ms()],
    )?;
    if n == 0 {
      return Err(LexError::NotFound);
    }
    Ok(())
  }

  pub fn delete_term(&self, id: i64) -> rusqlite::Result<bool> {
    Ok(self.conn().execute("DELETE FROM dictionary WHERE id = ?1", [id])? > 0)
  }

  pub fn snippets(&self) -> rusqlite::Result<Vec<SnippetRow>> {
    let conn = self.conn();
    let mut stmt = conn.prepare(&format!(
      "SELECT {SNIPPET_COLS} FROM snippets ORDER BY created_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map([], map_snippet)?;
    rows.collect()
  }

  fn snippet(&self, id: i64) -> rusqlite::Result<Option<SnippetRow>> {
    self
      .conn()
      .query_row(
        &format!("SELECT {SNIPPET_COLS} FROM snippets WHERE id = ?1"),
        [id],
        map_snippet,
      )
      .optional()
  }

  fn trigger_exists(&self, trigger: &str, except: Option<i64>) -> rusqlite::Result<bool> {
    let key = trigger.to_lowercase();
    Ok(
      self
        .snippets()?
        .iter()
        .any(|s| Some(s.id) != except && s.trigger.to_lowercase() == key),
    )
  }

  pub fn add_snippet(&self, trigger: &str, expansion: &str) -> Result<SnippetRow, LexError> {
    let trigger = clean_trigger(trigger)?;
    let expansion = clean_expansion(expansion)?;
    if self.trigger_exists(&trigger, None)? {
      return Err(LexError::Duplicate);
    }
    let now = now_ms();
    let id = {
      let conn = self.conn();
      conn.execute(
        "INSERT INTO snippets (trigger, expansion, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![trigger, expansion, now],
      )?;
      conn.last_insert_rowid()
    };
    self.snippet(id)?.ok_or(LexError::NotFound)
  }

  pub fn update_snippet(&self, id: i64, trigger: &str, expansion: &str) -> Result<SnippetRow, LexError> {
    let trigger = clean_trigger(trigger)?;
    let expansion = clean_expansion(expansion)?;
    if self.trigger_exists(&trigger, Some(id))? {
      return Err(LexError::Duplicate);
    }
    let n = self.conn().execute(
      "UPDATE snippets SET trigger = ?2, expansion = ?3, updated_at = ?4 WHERE id = ?1",
      params![id, trigger, expansion, now_ms()],
    )?;
    if n == 0 {
      return Err(LexError::NotFound);
    }
    self.snippet(id)?.ok_or(LexError::NotFound)
  }

  pub fn delete_snippet(&self, id: i64) -> rusqlite::Result<bool> {
    Ok(self.conn().execute("DELETE FROM snippets WHERE id = ?1", [id])? > 0)
  }

  /// Counts uses of applied corrections and snippets (for Insights and sorting).
  pub fn count_uses(&self, term_ids: &[i64], snippet_ids: &[i64]) -> rusqlite::Result<()> {
    let conn = self.conn();
    for id in term_ids {
      conn.execute("UPDATE dictionary SET uses = uses + 1 WHERE id = ?1", [id])?;
    }
    for id in snippet_ids {
      conn.execute("UPDATE snippets SET uses = uses + 1 WHERE id = ?1", [id])?;
    }
    Ok(())
  }

  /// Active dictionary and snippets in the form the text pipeline uses.
  pub fn lexicon(&self) -> rusqlite::Result<Lexicon> {
    let mut lex = Lexicon::default();
    for t in self.terms()?.into_iter().filter(|t| t.status == "active") {
      if !lex.vocabulary.iter().any(|v| v.eq_ignore_ascii_case(&t.phrase)) {
        lex.vocabulary.push(t.phrase.clone());
      }
      if let Some(heard) = t.heard {
        lex.corrections.push(Correction {
          id: t.id,
          heard,
          phrase: t.phrase,
        });
      }
    }
    lex.snippets = self
      .snippets()?
      .into_iter()
      .map(|s| Snippet {
        id: s.id,
        trigger: s.trigger,
        expansion: s.expansion,
      })
      .collect();
    Ok(lex)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn terms_validate_and_reject_duplicates() {
    let db = Db::open_in_memory().unwrap();
    let t = db.add_term("  Tanjim   Riju ", None, "manual", "active").unwrap();
    assert_eq!(t.phrase, "Tanjim Riju");
    assert!(matches!(
      db.add_term("tanjim riju", None, "manual", "active"),
      Err(LexError::Duplicate)
    ));
    // Same phrase with a correction is a different entry.
    db.add_term("Tanjim Riju", Some("tan jim"), "manual", "active").unwrap();
    assert!(matches!(
      db.add_term("   ", None, "manual", "active"),
      Err(LexError::Invalid(_))
    ));
    assert!(matches!(
      db.add_term("!!", None, "manual", "active"),
      Err(LexError::Invalid(_))
    ));
    assert!(matches!(
      db.add_term("WACV", Some("wacv"), "manual", "active"),
      Err(LexError::Invalid(_))
    ));
    assert!(matches!(
      db.add_term(&"x".repeat(81), None, "manual", "active"),
      Err(LexError::Invalid(_))
    ));
    // Unicode duplicate check (SQLite lower() is ASCII-only).
    db.add_term("École", None, "manual", "active").unwrap();
    assert!(matches!(
      db.add_term("ÉCOLE", None, "manual", "active"),
      Err(LexError::Duplicate)
    ));
  }

  #[test]
  fn update_delete_and_status() {
    let db = Db::open_in_memory().unwrap();
    let a = db.add_term("Daraz", None, "manual", "active").unwrap();
    let b = db.add_term("WACV", Some("wack v"), "learned", "suggested").unwrap();
    assert!(matches!(
      db.update_term(a.id, "wacv", Some("wack v")),
      Err(LexError::Duplicate)
    ));
    let a2 = db.update_term(a.id, "Daraz.com", None).unwrap();
    assert_eq!(a2.phrase, "Daraz.com");
    // Suggested entries are listed first and not applied.
    assert_eq!(db.terms().unwrap()[0].id, b.id);
    assert!(db.lexicon().unwrap().corrections.is_empty());
    db.set_term_status(b.id, "active").unwrap();
    let lex = db.lexicon().unwrap();
    assert_eq!(lex.corrections.len(), 1);
    assert_eq!(lex.vocabulary, vec!["WACV".to_string(), "Daraz.com".into()]);
    assert!(db.delete_term(a.id).unwrap());
    assert!(!db.delete_term(a.id).unwrap());
    assert!(matches!(db.update_term(a.id, "x1", None), Err(LexError::NotFound)));
  }

  #[test]
  fn snippets_validate_keep_exact_text_and_count_uses() {
    let db = Db::open_in_memory().unwrap();
    let s = db.add_snippet(" my  email ", "\r\nTanjim@Example.com  \r\n").unwrap();
    assert_eq!(s.trigger, "my email");
    assert_eq!(s.expansion, "Tanjim@Example.com  ", "inner text and spacing kept");
    assert!(matches!(db.add_snippet("MY EMAIL", "x"), Err(LexError::Duplicate)));
    assert!(matches!(db.add_snippet("a", "x"), Err(LexError::Invalid(_))));
    assert!(matches!(db.add_snippet("sign off", "  \n "), Err(LexError::Invalid(_))));
    let multi = db.add_snippet("sign off", "Best,\r\nTanjim").unwrap();
    assert_eq!(multi.expansion, "Best,\nTanjim");
    db.count_uses(&[], &[s.id, s.id]).unwrap();
    assert_eq!(db.snippets().unwrap().iter().find(|x| x.id == s.id).unwrap().uses, 2);
    let lex = db.lexicon().unwrap();
    assert_eq!(lex.snippets.len(), 2);
    assert!(db.delete_snippet(multi.id).unwrap());
  }
}
