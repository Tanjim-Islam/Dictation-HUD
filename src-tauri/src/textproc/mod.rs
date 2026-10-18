//! Deterministic text pipeline between the speech provider and delivery.
//!
//! Order: dictionary corrections, vocabulary casing, snippet triggers to placeholders, spoken
//! commands, smart formatting. Optional AI refinement then sees placeholders instead of
//! snippet text, and [`finish`] restores the exact expansions (or reports that the model
//! broke a placeholder, so the caller falls back to the deterministic text).

pub mod cleanup;
pub mod coding;
pub mod commands;
pub mod lexicon;
pub mod matcher;

pub use coding::Target;
pub use lexicon::{Correction, Lexicon, Snippet};

#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
  /// Formatted text with snippet placeholders.
  pub text: String,
  /// Exact expansion per placeholder.
  pub expansions: Vec<String>,
  /// Dictionary rows applied as corrections (one id per replacement).
  pub corrections: Vec<i64>,
  /// Snippet rows expanded (one id per expansion).
  pub snippets: Vec<i64>,
}

impl Prepared {
  /// Whether anything besides snippet placeholders is left for AI refinement to improve.
  pub fn has_prose(&self) -> bool {
    let mut rest = self.text.clone();
    for i in 0..self.expansions.len() {
      rest = rest.replace(&lexicon::placeholder(i), " ");
    }
    rest.chars().filter(|c| c.is_alphabetic()).count() >= 2
  }
}

pub fn prepare(raw: &str, lex: &Lexicon, smart_formatting: bool, target: Target) -> Prepared {
  let (text, corrections) = lexicon::apply_corrections(raw, &lex.corrections);
  let text = lexicon::apply_vocabulary_case(&text, &lex.vocabulary);
  let (text, mut expansions, snippets) = lexicon::protect_snippets(&text, &lex.snippets);
  let text = if target.is_code() {
    // Identifiers from casing commands are protected exactly like snippets.
    let (text, idents) = coding::apply_casing(&text, expansions.len(), lexicon::placeholder);
    expansions.extend(idents);
    coding::apply_code_symbols(&text)
  } else {
    text
  };
  let applied = commands::apply(&text);
  let mut text = if smart_formatting {
    cleanup::smart_format(&applied.text, &applied.sentence_starts)
  } else {
    applied.text.trim().to_string()
  };
  if target == Target::Terminal {
    // A shell command: no capital first word (unless it is deliberately cased, like
    // `Get-ChildItem` or `SELECT`) and no trailing period.
    text = coding::lowercase_plain_first_word(&text);
    text = coding::terminal_finish(&text);
  }
  Prepared {
    text,
    expansions,
    corrections,
    snippets,
  }
}

/// Restores snippet expansions in `text` (the prepared text or an AI rewrite of it).
pub fn finish(text: &str, expansions: &[String]) -> Result<String, usize> {
  lexicon::restore_snippets(text, expansions)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn lex() -> Lexicon {
    Lexicon {
      vocabulary: vec!["Citibank".into(), "WACV".into()],
      corrections: vec![Correction {
        id: 7,
        heard: "city bank".into(),
        phrase: "Citibank".into(),
      }],
      snippets: vec![Snippet {
        id: 9,
        trigger: "my email".into(),
        expansion: "tanjim@Example.com".into(),
      }],
    }
  }

  #[test]
  fn full_pipeline() {
    let p = prepare(
      "send the wacv report to city bank new line and cc my email period",
      &lex(),
      true,
      Target::Prose,
    );
    assert_eq!(p.corrections, vec![7]);
    assert_eq!(p.snippets, vec![9]);
    assert_eq!(p.text, "Send the WACV report to Citibank\nAnd cc \u{27E6}1\u{27E7}.");
    assert_eq!(
      finish(&p.text, &p.expansions).unwrap(),
      "Send the WACV report to Citibank\nAnd cc tanjim@Example.com."
    );
    assert!(p.has_prose());
  }

  #[test]
  fn snippet_only_has_no_prose() {
    let p = prepare("my email", &lex(), true, Target::Prose);
    assert_eq!(finish(&p.text, &p.expansions).unwrap(), "tanjim@Example.com");
    assert!(!p.has_prose(), "nothing for AI to refine");
  }

  #[test]
  fn smart_formatting_off_keeps_provider_text() {
    let p = prepare("so  i think", &Lexicon::default(), false, Target::Prose);
    assert_eq!(p.text, "so  i think");
  }

  #[test]
  fn coding_targets() {
    let lex = Lexicon::default();
    let p = prepare(
      "rename camel case get user name to snake case user name",
      &lex,
      true,
      Target::Editor,
    );
    assert_eq!(
      finish(&p.text, &p.expansions).unwrap(),
      "Rename getUserName to user_name"
    );
    assert!(p.has_prose());
    let t = prepare("Git status.", &lex, true, Target::Terminal);
    assert_eq!(finish(&t.text, &t.expansions).unwrap(), "git status");
    let t = prepare("cd camel case my project.", &lex, true, Target::Terminal);
    assert_eq!(finish(&t.text, &t.expansions).unwrap(), "cd myProject");
    // Prose targets never apply coding rules.
    let p = prepare("camel case get user", &lex, true, Target::Prose);
    assert_eq!(p.text, "Camel case get user");
  }
}
