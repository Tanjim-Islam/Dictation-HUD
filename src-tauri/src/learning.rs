//! Correction learning: when the user fixes a word right after a paste, offer it for the
//! dictionary.
//!
//! The field is read (ephemerally, through UI Automation) right after the paste and again a
//! little later. Only small substitutions inside the pasted text that look like
//! misrecognitions are kept: 1 to 3 words replaced by 1 to 3 words with similar spelling
//! ("wack v" -> "WACV", "tan jim" -> "Tanjim"), or a casing fix ("citibank" -> "Citibank").
//! Rewrites ("good" -> "great") and edits outside the pasted text are ignored. Secure fields
//! are never read.

use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
  Off,
  /// Add as suggestions the user accepts in Dictionary.
  #[default]
  Suggest,
  /// Add directly as active dictionary entries.
  Auto,
}

impl Mode {
  pub fn parse(s: &str) -> Option<Mode> {
    match s {
      "off" => Some(Mode::Off),
      "suggest" => Some(Mode::Suggest),
      "auto" => Some(Mode::Auto),
      _ => None,
    }
  }
}

/// A learned entry: `heard` is `None` for a casing fix (vocabulary only).
#[derive(Debug, Clone, PartialEq)]
pub struct Learned {
  pub phrase: String,
  pub heard: Option<String>,
}

const MAX_RUN: usize = 3;
const MAX_LEARNED: usize = 3;
pub const MAX_FIELD_CHARS: usize = 20_000;
/// When the field is read again after the paste.
pub const CHECKS: &[Duration] = &[
  Duration::from_secs(6),
  Duration::from_secs(12),
  Duration::from_secs(20),
  Duration::from_secs(30),
];

fn word_key(w: &str) -> String {
  w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

fn bare(w: &str) -> &str {
  w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-' && c != '.')
    .trim_end_matches('.')
}

fn levenshtein(a: &str, b: &str) -> usize {
  let a: Vec<char> = a.chars().collect();
  let b: Vec<char> = b.chars().collect();
  let mut prev: Vec<usize> = (0..=b.len()).collect();
  for (i, ca) in a.iter().enumerate() {
    let mut cur = vec![i + 1; b.len() + 1];
    for (j, cb) in b.iter().enumerate() {
      cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
    }
    prev = cur;
  }
  prev[b.len()]
}

/// Spelling similarity 0..1 ignoring case and spaces.
fn similarity(a: &str, b: &str) -> f64 {
  let a: String = a.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect();
  let b: String = b.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect();
  let n = a.chars().count().max(b.chars().count());
  if n == 0 {
    return 0.0;
  }
  1.0 - levenshtein(&a, &b) as f64 / n as f64
}

/// Word alignment by longest common subsequence; returns replaced runs (old, new).
fn replaced_runs<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(Vec<&'a str>, Vec<&'a str>)> {
  let (n, m) = (old.len(), new.len());
  let mut lcs = vec![vec![0usize; m + 1]; n + 1];
  for i in (0..n).rev() {
    for j in (0..m).rev() {
      lcs[i][j] = if word_key(old[i]) == word_key(new[j]) && old[i] == new[j] {
        lcs[i + 1][j + 1] + 1
      } else {
        lcs[i + 1][j].max(lcs[i][j + 1])
      };
    }
  }
  let (mut i, mut j) = (0, 0);
  let mut runs = Vec::new();
  let (mut ro, mut rn): (Vec<&str>, Vec<&str>) = (Vec::new(), Vec::new());
  let flush = |ro: &mut Vec<&'a str>, rn: &mut Vec<&'a str>, runs: &mut Vec<(Vec<&'a str>, Vec<&'a str>)>| {
    if !ro.is_empty() || !rn.is_empty() {
      runs.push((std::mem::take(ro), std::mem::take(rn)));
    }
  };
  while i < n || j < m {
    if i < n && j < m && old[i] == new[j] {
      flush(&mut ro, &mut rn, &mut runs);
      i += 1;
      j += 1;
    } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
      rn.push(new[j]);
      j += 1;
    } else {
      ro.push(old[i]);
      i += 1;
    }
  }
  flush(&mut ro, &mut rn, &mut runs);
  runs
}

/// What the user changed inside the pasted text. `before` is the field right after the paste,
/// `after` a later read. `None` when the pasted text cannot be located or the user edited
/// around it.
fn edited_region<'a>(inserted: &str, before: &str, after: &'a str) -> Option<&'a str> {
  let start = before.rfind(inserted)?;
  let prefix = &before[..start];
  let suffix = &before[start + inserted.len()..];
  if !after.starts_with(prefix) || !after.ends_with(suffix) || after.len() < prefix.len() + suffix.len() {
    return None;
  }
  Some(&after[prefix.len()..after.len() - suffix.len()])
}

pub fn learn(inserted: &str, before: &str, after: &str) -> Vec<Learned> {
  let Some(edited) = edited_region(inserted, before, after) else {
    return Vec::new();
  };
  if edited == inserted {
    return Vec::new();
  }
  let old: Vec<&str> = inserted.split_whitespace().collect();
  let new: Vec<&str> = edited.split_whitespace().collect();
  let mut out: Vec<Learned> = Vec::new();
  for (ro, rn) in replaced_runs(&old, &new) {
    if ro.is_empty() || rn.is_empty() || ro.len() > MAX_RUN || rn.len() > MAX_RUN {
      continue;
    }
    let heard = ro.iter().map(|w| bare(w)).collect::<Vec<_>>().join(" ");
    let phrase = rn.iter().map(|w| bare(w)).collect::<Vec<_>>().join(" ");
    if phrase.chars().filter(|c| c.is_alphanumeric()).count() < 2 || heard.trim().is_empty() {
      continue;
    }
    let learned = if heard.to_lowercase() == phrase.to_lowercase() {
      // Only a casing fix: remember the spelling.
      if heard == phrase || !phrase.chars().any(char::is_uppercase) {
        continue;
      }
      Learned { phrase, heard: None }
    } else if similarity(&heard, &phrase) >= 0.5 {
      Learned {
        phrase,
        heard: Some(heard),
      }
    } else {
      continue;
    };
    if !out.contains(&learned) {
      out.push(learned);
    }
    if out.len() == MAX_LEARNED {
      break;
    }
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  fn corr(phrase: &str, heard: &str) -> Learned {
    Learned {
      phrase: phrase.into(),
      heard: Some(heard.into()),
    }
  }

  #[test]
  fn learns_misrecognitions_inside_the_paste() {
    let ins = "The wack v deadline is Friday.";
    let before = format!("Notes: {ins} Thanks");
    let after = "Notes: The WACV deadline is Friday. Thanks";
    assert_eq!(learn(ins, &before, after), vec![corr("WACV", "wack v")]);
    let ins = "Ask tan jim about it.";
    assert_eq!(learn(ins, ins, "Ask Tanjim about it."), vec![corr("Tanjim", "tan jim")]);
  }

  #[test]
  fn casing_fix_becomes_vocabulary() {
    let ins = "send it to citibank today";
    assert_eq!(
      learn(ins, ins, "send it to Citibank today"),
      vec![Learned {
        phrase: "Citibank".into(),
        heard: None
      }]
    );
  }

  #[test]
  fn ignores_rewrites_edits_elsewhere_and_unchanged() {
    let ins = "This is a good plan.";
    assert!(learn(ins, ins, "This is a great plan.").is_empty(), "style edit");
    assert!(learn(ins, ins, ins).is_empty());
    assert!(learn(ins, ins, "").is_empty(), "deleted");
    // Text around the paste changed: alignment is unknown, learn nothing.
    let before = format!("A {ins} B");
    assert!(learn(ins, &before, "X This is a gud plan. B").is_empty());
    // Long rewrites are not misrecognitions.
    assert!(learn(
      "one two three four five",
      "one two three four five",
      "uno dos tres cuatro five"
    )
    .is_empty());
    // Pasted text not found.
    assert!(learn("missing", "other text", "other text!").is_empty());
  }

  #[test]
  fn punctuation_is_not_learned_into_entries() {
    let ins = "Meet at deraz, then lunch.";
    assert_eq!(
      learn(ins, ins, "Meet at Daraz, then lunch."),
      vec![corr("Daraz", "deraz")]
    );
  }

  #[test]
  fn modes_parse() {
    assert_eq!(Mode::parse("auto"), Some(Mode::Auto));
    assert_eq!(Mode::parse("x"), None);
    assert_eq!(Mode::default(), Mode::Suggest);
  }
}
