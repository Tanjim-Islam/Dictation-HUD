//! Case-insensitive whole-phrase matching on Unicode text.
//!
//! A phrase matches when its characters match the text case-insensitively, a space in the
//! phrase matches any run of whitespace, and the match does not start or end inside a word
//! (a boundary is only required on a side where the phrase itself starts or ends with a letter
//! or digit, so phrases like `@home` still work).

fn is_word(c: char) -> bool {
  c.is_alphanumeric() || c == '\''
}

fn same(a: char, b: char) -> bool {
  a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// Normalizes a phrase for matching: trimmed, single spaces.
pub fn normalize(phrase: &str) -> String {
  phrase.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Byte range of the match of `phrase` starting exactly at byte `start`, if any.
fn match_at(text: &str, start: usize, phrase: &[char]) -> Option<usize> {
  let mut chars = text[start..].char_indices().peekable();
  let mut pi = 0;
  while pi < phrase.len() {
    let p = phrase[pi];
    if p == ' ' {
      let mut any = false;
      while let Some(&(_, c)) = chars.peek() {
        if c.is_whitespace() {
          chars.next();
          any = true;
        } else {
          break;
        }
      }
      if !any {
        return None;
      }
    } else {
      let (_, c) = chars.next()?;
      if !same(c, p) {
        return None;
      }
    }
    pi += 1;
  }
  Some(chars.peek().map(|&(i, _)| start + i).unwrap_or(text.len()))
}

/// All non-overlapping matches of `phrase`, left to right, as byte ranges.
pub fn find_all(text: &str, phrase: &str) -> Vec<(usize, usize)> {
  let norm = normalize(phrase);
  let p: Vec<char> = norm.chars().collect();
  if p.is_empty() {
    return Vec::new();
  }
  let need_left = is_word(p[0]);
  let need_right = is_word(p[p.len() - 1]);
  let mut out = Vec::new();
  let mut prev: Option<char> = None;
  let mut skip_until = 0;
  for (i, c) in text.char_indices() {
    if i >= skip_until && (!need_left || !prev.is_some_and(is_word)) {
      if let Some(end) = match_at(text, i, &p) {
        let next = text[end..].chars().next();
        if !need_right || !next.is_some_and(is_word) {
          out.push((i, end));
          skip_until = end;
        }
      }
    }
    prev = Some(c);
  }
  out
}

/// One replacement candidate: phrase to find and what replaces it.
pub struct Rule<'a> {
  pub find: &'a str,
  pub id: usize,
}

/// Non-overlapping matches for many phrases. Longer phrases win where matches overlap, then
/// the earlier match. Returns (start, end, rule id), sorted by position.
pub fn find_longest(text: &str, rules: &[Rule<'_>]) -> Vec<(usize, usize, usize)> {
  let mut all: Vec<(usize, usize, usize)> = Vec::new();
  for r in rules {
    for (s, e) in find_all(text, r.find) {
      all.push((s, e, r.id));
    }
  }
  // Longest first, then leftmost.
  all.sort_by(|a, b| (b.1 - b.0).cmp(&(a.1 - a.0)).then(a.0.cmp(&b.0)));
  let mut chosen: Vec<(usize, usize, usize)> = Vec::new();
  for m in all {
    if chosen.iter().all(|c| m.1 <= c.0 || m.0 >= c.1) {
      chosen.push(m);
    }
  }
  chosen.sort_by_key(|m| m.0);
  chosen
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn whole_words_case_insensitive() {
    assert_eq!(find_all("Send to Citibank now", "citibank"), vec![(8, 16)]);
    assert!(find_all("citibanks", "citibank").is_empty());
    assert!(find_all("acitibank", "citibank").is_empty());
    assert_eq!(find_all("a  new\tline", "new line"), vec![(3, 11)]);
  }

  #[test]
  fn symbols_at_edges_need_no_boundary() {
    assert_eq!(find_all("mail@home", "@home"), vec![(4, 9)]);
  }

  #[test]
  fn unicode_text_is_safe() {
    assert_eq!(find_all("আমি Citibank যাব", "citibank").len(), 1);
    assert_eq!(find_all("ÉCOLE école", "école").len(), 2);
  }

  #[test]
  fn longest_match_wins() {
    let rules = [
      Rule {
        find: "my address",
        id: 0,
      },
      Rule {
        find: "my address line two",
        id: 1,
      },
    ];
    let m = find_longest("use my address line two please", &rules);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].2, 1);
    let m = find_longest("my address and my address line two", &rules);
    assert_eq!(m.iter().map(|x| x.2).collect::<Vec<_>>(), vec![0, 1]);
  }
}
