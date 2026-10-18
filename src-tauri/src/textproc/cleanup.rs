//! Smart formatting: small, safe fixes applied without AI.
//!
//! Only changes that cannot alter meaning: collapse repeated spaces, remove spaces before
//! punctuation, standalone "i" to "I", and capitalize the start of the text and of sentences
//! that a spoken command started. Provider punctuation and casing are otherwise left alone.

fn capitalize_at(s: &mut String, idx: usize) {
  let Some(c) = s[idx..].chars().next() else {
    return;
  };
  if c.is_lowercase() {
    let up: String = c.to_uppercase().collect();
    s.replace_range(idx..idx + c.len_utf8(), &up);
  }
}

/// Fixes standalone lowercase "i" and its contractions.
fn fix_pronoun_i(text: &str) -> String {
  let mut out = String::with_capacity(text.len());
  let chars: Vec<(usize, char)> = text.char_indices().collect();
  for (k, &(_, c)) in chars.iter().enumerate() {
    let prev = k.checked_sub(1).map(|j| chars[j].1);
    let next = chars.get(k + 1).map(|x| x.1);
    let alone = c == 'i'
      && !prev.is_some_and(|p| p.is_alphanumeric() || p == '\'' || p == '-' || p == '.' || p == '/')
      && match next {
        None => true,
        Some('\'') => chars
          .get(k + 2)
          .is_some_and(|x| matches!(x.1, 'm' | 'l' | 'v' | 'd' | 's')),
        Some(n) => !(n.is_alphanumeric() || n == '-' || n == '.' || n == '/' || n == '@' || n == '_'),
      };
    out.push(if alone { 'I' } else { c });
  }
  out
}

/// `sentence_starts` are byte offsets (in `text`) where a spoken command began a sentence.
pub fn smart_format(text: &str, sentence_starts: &[usize]) -> String {
  // Capitalize command-started sentences first, while offsets are still valid.
  let mut s = text.to_string();
  for &i in sentence_starts {
    if i < s.len() && s.is_char_boundary(i) {
      capitalize_at(&mut s, i);
    }
  }
  // Collapse runs of spaces and tabs (line breaks are kept).
  let mut out = String::with_capacity(s.len());
  let mut last_space = false;
  for c in s.chars() {
    if c == ' ' || c == '\t' {
      if !last_space {
        out.push(' ');
      }
      last_space = true;
    } else {
      last_space = false;
      out.push(c);
    }
  }
  // No space before closing punctuation; trim spaces at line edges.
  let mut fixed = String::with_capacity(out.len());
  let chars: Vec<char> = out.chars().collect();
  for (k, &c) in chars.iter().enumerate() {
    if c == ' ' {
      let next = chars.get(k + 1).copied();
      if matches!(next, Some(',' | '.' | '?' | '!' | ';' | ':' | '\n') | None) || fixed.ends_with('\n') {
        continue;
      }
    }
    fixed.push(c);
  }
  let mut fixed = fix_pronoun_i(fixed.trim());
  // First letter of the text.
  if let Some((i, _)) = fixed.char_indices().find(|(_, c)| c.is_alphabetic()) {
    if !fixed[..i].contains(|c: char| c.is_alphanumeric()) {
      capitalize_at(&mut fixed, i);
    }
  }
  fixed
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn spacing_and_pronoun() {
    assert_eq!(
      smart_format("so  i think i'm   right , ok", &[]),
      "So I think I'm right, ok"
    );
    assert_eq!(
      smart_format("pi is not i.e. and wi-fi i/o", &[]),
      "Pi is not i.e. and wi-fi i/o"
    );
    assert_eq!(smart_format("line one \n line two", &[]), "Line one\nline two");
  }

  #[test]
  fn capitalizes_only_command_sentence_starts() {
    let text = "hi? how are you\nnext. ok";
    // Offsets of "how" and "next" come from the command step; "ok" was provider text.
    let starts = [text.find("how").unwrap(), text.find("next").unwrap()];
    assert_eq!(smart_format(text, &starts), "Hi? How are you\nNext. ok");
  }

  #[test]
  fn leaves_other_scripts_and_symbols() {
    assert_eq!(smart_format("আমি ভালো আছি", &[]), "আমি ভালো আছি");
    assert_eq!(
      smart_format("\u{27E6}1\u{27E7} is my email", &[]),
      "\u{27E6}1\u{27E7} is my email"
    );
  }
}
