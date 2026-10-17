//! Spoken formatting commands ("new line", "question mark", "comma").
//!
//! Conservative by design: only phrases that are unambiguous in dictation are always
//! replaced. Words with an everyday meaning ("period", "colon") are replaced only at the end
//! of the dictation or right before a line-break command, and words like "enter", "dot",
//! "plus", "star", "dollar", or "less than" are never treated as commands, so "a period of
//! time" or "less than ten" stay as spoken.

use super::matcher::{find_longest, Rule};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
  /// Line or paragraph break: trims spaces and commas around it.
  Break,
  /// `, . ? ! ; : …`: attaches to the previous word.
  Punct,
  /// `— –`: joins the words on both sides.
  Join,
  /// `( [ { "` opening: attaches to the next word.
  Open,
  /// `) ] } "` closing: attaches to the previous word.
  Close,
  /// `@`: joins both sides (emails).
  Glue,
  /// `$`: attaches to the next word.
  Prefix,
  /// `%`: attaches to the previous word.
  Suffix,
  /// Other signs: spaced like a word.
  Word,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum When {
  Always,
  /// Only at the end of the dictation or before a break command.
  AtEnd,
}

struct Command {
  phrase: &'static str,
  out: &'static str,
  kind: Kind,
  when: When,
}

const fn c(phrase: &'static str, out: &'static str, kind: Kind) -> Command {
  Command {
    phrase,
    out,
    kind,
    when: When::Always,
  }
}

const fn end(phrase: &'static str, out: &'static str, kind: Kind) -> Command {
  Command {
    phrase,
    out,
    kind,
    when: When::AtEnd,
  }
}

const COMMANDS: &[Command] = &[
  c("new paragraph", "\n\n", Kind::Break),
  c("paragraph break", "\n\n", Kind::Break),
  c("new line", "\n", Kind::Break),
  c("newline", "\n", Kind::Break),
  c("line break", "\n", Kind::Break),
  c("question mark", "?", Kind::Punct),
  c("exclamation mark", "!", Kind::Punct),
  c("exclamation point", "!", Kind::Punct),
  c("full stop", ".", Kind::Punct),
  c("comma", ",", Kind::Punct),
  c("semicolon", ";", Kind::Punct),
  c("semi colon", ";", Kind::Punct),
  c("ellipsis", "...", Kind::Punct),
  end("period", ".", Kind::Punct),
  end("colon", ":", Kind::Punct),
  c("em dash", "—", Kind::Join),
  c("en dash", "–", Kind::Join),
  c("open quote", "\"", Kind::Open),
  c("close quote", "\"", Kind::Close),
  c("open paren", "(", Kind::Open),
  c("close paren", ")", Kind::Close),
  c("open parenthesis", "(", Kind::Open),
  c("close parenthesis", ")", Kind::Close),
  c("open bracket", "[", Kind::Open),
  c("close bracket", "]", Kind::Close),
  c("open brace", "{", Kind::Open),
  c("close brace", "}", Kind::Close),
  c("at sign", "@", Kind::Glue),
  c("at symbol", "@", Kind::Glue),
  c("dollar sign", "$", Kind::Prefix),
  c("percent sign", "%", Kind::Suffix),
  c("plus sign", "+", Kind::Word),
  c("minus sign", "-", Kind::Word),
  c("equals sign", "=", Kind::Word),
  c("equal sign", "=", Kind::Word),
  c("pound sign", "#", Kind::Word),
  c("hash sign", "#", Kind::Word),
  c("ampersand", "&", Kind::Word),
  c("asterisk", "*", Kind::Word),
  c("forward slash", "/", Kind::Word),
  c("backslash", "\\", Kind::Word),
  c("back slash", "\\", Kind::Word),
];

/// Result of applying commands: the text and byte offsets where a new sentence starts because
/// of a command (after `? ! .` commands and breaks), for capitalization.
pub struct Applied {
  pub text: String,
  pub sentence_starts: Vec<usize>,
}

fn trim_end_chars<'a>(s: &'a str, chars: &[char]) -> &'a str {
  s.trim_end_matches(|c: char| chars.contains(&c))
}

/// True when only provider punctuation, whitespace, or a break command follows.
fn at_end(rest: &str) -> bool {
  let r = rest.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '.' | ',' | '!' | '?'));
  if r.is_empty() {
    return true;
  }
  let lower = r.to_lowercase();
  ["new line", "newline", "new paragraph", "line break", "paragraph break"]
    .iter()
    .any(|b| lower.starts_with(b))
}

pub fn apply(text: &str) -> Applied {
  let rules: Vec<Rule<'_>> = COMMANDS
    .iter()
    .enumerate()
    .map(|(id, cmd)| Rule { find: cmd.phrase, id })
    .collect();
  let matches = find_longest(text, &rules);
  let mut out = String::with_capacity(text.len());
  let mut starts = Vec::new();
  let mut pos = 0;
  for (s, e, id) in matches {
    let cmd = &COMMANDS[id];
    if s < pos || (cmd.when == When::AtEnd && !at_end(&text[e..])) {
      continue;
    }
    let mut before = &text[pos..s];
    // Providers often wrap spoken commands in their own punctuation ("hello, comma, world").
    before = match cmd.kind {
      Kind::Break => trim_end_chars(before, &[' ', '\t', ',']),
      Kind::Punct | Kind::Close | Kind::Suffix => trim_end_chars(before, &[' ', '\t', ',']),
      Kind::Join | Kind::Glue => trim_end_chars(before, &[' ', '\t']),
      _ => before,
    };
    out.push_str(before);
    out.push_str(cmd.out);
    let mut after = e;
    let rest = &text[e..];
    let skip = match cmd.kind {
      Kind::Break => rest.len() - rest.trim_start_matches([' ', '\t', ',', '.']).len(),
      // The provider often punctuates the spoken command itself ("question mark?").
      Kind::Punct => rest.len() - rest.trim_start_matches([',', '.', '?', '!', ';', ':']).len(),
      Kind::Join | Kind::Glue | Kind::Open | Kind::Prefix => {
        rest.len() - rest.trim_start_matches([' ', '\t', ',']).len()
      }
      Kind::Close | Kind::Suffix => rest.len() - rest.trim_start_matches(',').len(),
      Kind::Word => 0,
    };
    after += skip;
    if cmd.kind == Kind::Break || (cmd.kind == Kind::Punct && matches!(cmd.out, "." | "?" | "!")) {
      // The next word starts a sentence (position in `out` after any following spaces).
      let lead = text[after..].len() - text[after..].trim_start_matches([' ', '\t']).len();
      starts.push(out.len() + lead);
    }
    pos = after;
  }
  out.push_str(&text[pos..]);
  Applied {
    text: out,
    sentence_starts: starts,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn t(s: &str) -> String {
    apply(s).text
  }

  #[test]
  fn breaks_and_punctuation() {
    assert_eq!(t("hello new line world"), "hello\nworld");
    assert_eq!(t("Hello, new line. World"), "Hello\nWorld");
    assert_eq!(t("one new paragraph two"), "one\n\ntwo");
    assert_eq!(t("is it done question mark yes"), "is it done? yes");
    assert_eq!(t("hello comma world"), "hello, world");
    assert_eq!(t("hello, comma, world"), "hello, world");
    assert_eq!(t("wait exclamation point"), "wait!");
    assert_eq!(t("Is that everything question mark? We"), "Is that everything? We");
    assert_eq!(
      t("Milk and eggs. New line. Bread."),
      "Milk and eggs.
Bread."
    );
  }

  #[test]
  fn everyday_words_stay() {
    assert_eq!(t("a period of time"), "a period of time");
    assert_eq!(t("enter the room"), "enter the room");
    assert_eq!(t("less than ten dollars"), "less than ten dollars");
    assert_eq!(t("five dollar bill and a star"), "five dollar bill and a star");
    assert_eq!(t("colon cancer screening"), "colon cancer screening");
    assert_eq!(t("dot the i"), "dot the i");
    assert_eq!(t("plus one"), "plus one");
  }

  #[test]
  fn period_and_colon_only_at_end_or_before_a_break() {
    assert_eq!(t("I am home period"), "I am home.");
    assert_eq!(t("I am home period."), "I am home.");
    assert_eq!(t("groceries colon new line milk"), "groceries:\nmilk");
    assert_eq!(t("done period new line next"), "done.\nnext");
  }

  #[test]
  fn signs_and_brackets() {
    assert_eq!(t("tanjim at sign example.com"), "tanjim@example.com");
    assert_eq!(t("costs dollar sign 5"), "costs $5");
    assert_eq!(t("50 percent sign off"), "50% off");
    assert_eq!(t("open paren aside close paren ok"), "(aside) ok");
    assert_eq!(t("he said open quote hi close quote"), "he said \"hi\"");
    assert_eq!(t("word em dash word"), "word—word");
  }

  #[test]
  fn sentence_starts_after_commands() {
    let a = apply("hi question mark how are you new line next");
    assert_eq!(a.text, "hi? how are you\nnext");
    let words: Vec<&str> = a.sentence_starts.iter().map(|&i| &a.text[i..i + 3]).collect();
    assert_eq!(words, vec!["how", "nex"]);
  }

  #[test]
  fn unicode_untouched() {
    assert_eq!(t("আমি ভালো আছি"), "আমি ভালো আছি");
  }
}
