//! Dictionary corrections, vocabulary casing, and snippet placeholders.

use super::matcher::{find_longest, Rule};

/// A correction: what the recognizer writes (`heard`) and what the user means (`phrase`).
#[derive(Debug, Clone, PartialEq)]
pub struct Correction {
  /// Dictionary row id (0 in tests).
  pub id: i64,
  pub heard: String,
  pub phrase: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snippet {
  /// Snippet row id (0 in tests).
  pub id: i64,
  pub trigger: String,
  pub expansion: String,
}

/// Everything the text pipeline needs from the dictionary and snippets.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lexicon {
  /// Preferred spellings (provider hints, AI hints, and casing).
  pub vocabulary: Vec<String>,
  pub corrections: Vec<Correction>,
  pub snippets: Vec<Snippet>,
}

fn replace_matches(text: &str, matches: &[(usize, usize, usize)], with: impl Fn(usize) -> String) -> String {
  let mut out = String::with_capacity(text.len());
  let mut pos = 0;
  for &(s, e, id) in matches {
    out.push_str(&text[pos..s]);
    out.push_str(&with(id));
    pos = e;
  }
  out.push_str(&text[pos..]);
  out
}

/// Replaces misheard forms with the user's phrase (whole words, longest first). Returns the
/// text and the ids of the corrections applied (one per replacement).
pub fn apply_corrections(text: &str, corrections: &[Correction]) -> (String, Vec<i64>) {
  let rules: Vec<Rule<'_>> = corrections
    .iter()
    .enumerate()
    .filter(|(_, c)| !c.heard.trim().is_empty())
    .map(|(id, c)| Rule { find: &c.heard, id })
    .collect();
  let m = find_longest(text, &rules);
  let used = m.iter().map(|&(_, _, i)| corrections[i].id).collect();
  (replace_matches(text, &m, |id| corrections[id].phrase.clone()), used)
}

/// Restores the exact casing of vocabulary terms that carry deliberate casing ("WACV",
/// "iPhone", "Tanjim Riju"). All-lowercase terms are left alone: casing them would be a guess.
pub fn apply_vocabulary_case(text: &str, vocabulary: &[String]) -> String {
  let cased: Vec<&String> = vocabulary
    .iter()
    .filter(|v| v.chars().any(|c| c.is_uppercase()) && v.chars().filter(|c| c.is_alphanumeric()).count() >= 2)
    .collect();
  let rules: Vec<Rule<'_>> = cased
    .iter()
    .enumerate()
    .map(|(id, v)| Rule { find: v.as_str(), id })
    .collect();
  let m = find_longest(text, &rules);
  replace_matches(text, &m, |id| super::matcher::normalize(cased[id]))
}

/// Placeholder for snippet `i`. Private-use brackets that never occur in dictated text.
pub fn placeholder(i: usize) -> String {
  format!("\u{27E6}{}\u{27E7}", i + 1)
}

/// Replaces snippet triggers with placeholders so later steps (cleanup, AI refinement) cannot
/// alter the expansion. Returns the text, the exact expansion per placeholder, and the ids of
/// the snippets used.
pub fn protect_snippets(text: &str, snippets: &[Snippet]) -> (String, Vec<String>, Vec<i64>) {
  let rules: Vec<Rule<'_>> = snippets
    .iter()
    .enumerate()
    .filter(|(_, s)| !s.trigger.trim().is_empty())
    .map(|(id, s)| Rule { find: &s.trigger, id })
    .collect();
  let m = find_longest(text, &rules);
  let mut expansions = Vec::with_capacity(m.len());
  let mut ids = Vec::with_capacity(m.len());
  let mut out = String::with_capacity(text.len());
  let mut pos = 0;
  for &(s, e, id) in &m {
    out.push_str(&text[pos..s]);
    out.push_str(&placeholder(expansions.len()));
    expansions.push(snippets[id].expansion.clone());
    ids.push(snippets[id].id);
    pos = e;
  }
  out.push_str(&text[pos..]);
  (out, expansions, ids)
}

/// Puts the exact expansions back. Fails if any placeholder is missing or repeated (an AI
/// model dropped or duplicated it), so the caller can fall back to deterministic text.
pub fn restore_snippets(text: &str, expansions: &[String]) -> Result<String, usize> {
  let mut out = text.to_string();
  for (i, exp) in expansions.iter().enumerate() {
    let ph = placeholder(i);
    if out.matches(&ph).count() != 1 {
      return Err(i);
    }
    out = out.replacen(&ph, exp, 1);
  }
  // A stray placeholder the model invented.
  if out.contains('\u{27E6}') {
    return Err(expansions.len());
  }
  Ok(out)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn corr(heard: &str, phrase: &str) -> Correction {
    Correction {
      id: 0,
      heard: heard.into(),
      phrase: phrase.into(),
    }
  }

  #[test]
  fn corrections_whole_word_longest_first() {
    let c = [
      corr("tan jim", "Tanjim"),
      corr("tan jim ri ju", "Tanjim Riju"),
      corr("wack v", "WACV"),
    ];
    let (t, used) = apply_corrections("hi Tan Jim Ri Ju, see wack v", &c);
    assert_eq!(t, "hi Tanjim Riju, see WACV");
    assert_eq!(used.len(), 2);
    assert!(apply_corrections("tanjimx", &c).1.is_empty());
  }

  #[test]
  fn vocabulary_casing_only_for_deliberate_case() {
    let v = vec![
      "WACV".to_string(),
      "iPhone".into(),
      "daraz".into(),
      "Tanjim  Riju".into(),
    ];
    assert_eq!(
      apply_vocabulary_case("the wacv deadline on my iphone, daraz order from tanjim riju", &v),
      "the WACV deadline on my iPhone, daraz order from Tanjim Riju"
    );
  }

  #[test]
  fn snippets_round_trip_exactly() {
    let s = [
      Snippet {
        id: 1,
        trigger: "my email".into(),
        expansion: "tanjim@example.com".into(),
      },
      Snippet {
        id: 2,
        trigger: "my email signature".into(),
        expansion: "Best,\nTanjim".into(),
      },
    ];
    let (t, exp, ids) = protect_snippets("send my email and my email signature.", &s);
    assert_eq!(ids, vec![1, 2]);
    assert_eq!(t, "send \u{27E6}1\u{27E7} and \u{27E6}2\u{27E7}.");
    assert_eq!(
      restore_snippets(&t, &exp).unwrap(),
      "send tanjim@example.com and Best,\nTanjim."
    );
    // A model that dropped or duplicated a placeholder is detected.
    assert_eq!(restore_snippets("send it.", &exp), Err(0));
    assert_eq!(
      restore_snippets("\u{27E6}1\u{27E7} \u{27E6}1\u{27E7} \u{27E6}2\u{27E7}", &exp),
      Err(0)
    );
    assert!(restore_snippets("\u{27E6}9\u{27E7}", &[]).is_err());
  }
}
