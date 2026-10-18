//! Coding awareness: spoken casing commands and code symbols, only for code editors and
//! terminals.
//!
//! "camel case get user name" -> `getUserName`. A casing command takes the words that follow it
//! up to the next punctuation mark, line break, or 8 words. The identifier is protected like a
//! snippet so AI refinement cannot change it.

use super::matcher::{find_longest, Rule};

/// What kind of app the text goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Target {
  #[default]
  Prose,
  /// Code editors and IDEs: casing commands and code symbols on; prose formatting otherwise.
  Editor,
  /// Shells: also no capitalized first word and no trailing period.
  Terminal,
}

impl Target {
  pub fn is_code(self) -> bool {
    self != Target::Prose
  }

  /// Classifies an executable name (`Code.exe`, `WindowsTerminal.exe`).
  pub fn from_process(process: &str) -> Target {
    let p = process.to_ascii_lowercase();
    let p = p.trim_end_matches(".exe");
    const TERMINALS: &[&str] = &[
      "windowsterminal",
      "powershell",
      "pwsh",
      "cmd",
      "conhost",
      "wezterm-gui",
      "alacritty",
      "mintty",
      "wsl",
      "kitty",
    ];
    const EDITORS: &[&str] = &[
      "code",
      "code - insiders",
      "cursor",
      "windsurf",
      "devenv",
      "idea64",
      "pycharm64",
      "webstorm64",
      "rider64",
      "clion64",
      "goland64",
      "rustrover64",
      "datagrip64",
      "studio64",
      "sublime_text",
      "notepad++",
      "zed",
      "nvim-qt",
      "gvim",
      "fleet",
    ];
    if TERMINALS.contains(&p) {
      Target::Terminal
    } else if EDITORS.contains(&p) {
      Target::Editor
    } else {
      Target::Prose
    }
  }
}

#[derive(Clone, Copy)]
enum Case {
  Camel,
  Pascal,
  Snake,
  Kebab,
  Constant,
}

const CASINGS: &[(&str, Case)] = &[
  ("camel case", Case::Camel),
  ("pascal case", Case::Pascal),
  ("snake case", Case::Snake),
  ("kebab case", Case::Kebab),
  ("constant case", Case::Constant),
  ("screaming snake case", Case::Constant),
];

const MAX_WORDS: usize = 8;
/// Words that end an identifier ("rename camel case user name to ..."), except as its first word.
const STOP_WORDS: &[&str] = &["to", "then", "and", "or", "with", "from", "into", "as", "equals"];

fn cap(w: &str) -> String {
  let mut c = w.chars();
  c.next()
    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
    .unwrap_or_default()
}

fn join(case: Case, words: &[String]) -> String {
  match case {
    Case::Camel => words
      .iter()
      .enumerate()
      .map(|(i, w)| if i == 0 { w.clone() } else { cap(w) })
      .collect(),
    Case::Pascal => words.iter().map(|w| cap(w)).collect(),
    Case::Snake => words.join("_"),
    Case::Kebab => words.join("-"),
    Case::Constant => words.iter().map(|w| w.to_uppercase()).collect::<Vec<_>>().join("_"),
  }
}

/// Applies casing commands. Each identifier is replaced by `placeholder(start + n)`; returns
/// the text and the identifiers in placeholder order.
pub fn apply_casing(text: &str, start: usize, placeholder: impl Fn(usize) -> String) -> (String, Vec<String>) {
  let rules: Vec<Rule<'_>> = CASINGS
    .iter()
    .enumerate()
    .map(|(id, (p, _))| Rule { find: p, id })
    .collect();
  let mut out = String::with_capacity(text.len());
  let mut idents = Vec::new();
  let mut pos = 0;
  for (s, e, id) in find_longest(text, &rules) {
    if s < pos {
      continue;
    }
    // Words after the command: letters and digits separated by spaces (a provider comma right
    // after the command is skipped).
    let rest = &text[e..];
    let rest_trim = rest.trim_start_matches([',', ' ']);
    let mut words = Vec::new();
    let mut consumed = rest.len() - rest_trim.len();
    let mut cursor = rest_trim;
    while words.len() < MAX_WORDS {
      let word: String = cursor.chars().take_while(|c| c.is_alphanumeric()).collect();
      if word.is_empty() {
        break;
      }
      words.push(word.to_lowercase());
      consumed += word.len();
      cursor = &cursor[word.len()..];
      let spaces = cursor.len() - cursor.trim_start_matches(' ').len();
      if spaces == 0 || !cursor[spaces..].chars().next().is_some_and(|c| c.is_alphanumeric()) {
        break;
      }
      // Stop at the word limit, before a following casing command, or at a connective word.
      let next = cursor[spaces..].to_lowercase();
      let next_word: String = next.chars().take_while(|c| c.is_alphanumeric()).collect();
      if words.len() == MAX_WORDS
        || CASINGS.iter().any(|(p, _)| next.starts_with(p))
        || STOP_WORDS.contains(&next_word.as_str())
      {
        break;
      }
      consumed += spaces;
      cursor = &cursor[spaces..];
    }
    if words.is_empty() {
      continue;
    }
    out.push_str(&text[pos..s]);
    out.push_str(&placeholder(start + idents.len()));
    idents.push(join(CASINGS[id].1, &words));
    pos = e + consumed;
  }
  out.push_str(&text[pos..]);
  (out, idents)
}

/// Code symbols that join the words around them ("console dot log" -> "console.log").
const JOINERS: &[(&str, &str)] = &[
  ("dot", "."),
  ("underscore", "_"),
  ("double colon", "::"),
  ("open angle bracket", "<"),
  ("close angle bracket", ">"),
  ("backtick", "`"),
];

/// "dot" is also an everyday word ("the dot product"), so it only joins words in short
/// utterances, which in a code editor are far more likely to be code than prose.
const DOT_MAX_WORDS: usize = 8;

pub fn apply_code_symbols(text: &str) -> String {
  let short = text.split_whitespace().count() <= DOT_MAX_WORDS;
  let rules: Vec<Rule<'_>> = JOINERS
    .iter()
    .enumerate()
    .filter(|(_, (p, _))| short || *p != "dot")
    .map(|(id, (p, _))| Rule { find: p, id })
    .collect();
  let mut out = String::with_capacity(text.len());
  let mut pos = 0;
  for (s, e, id) in find_longest(text, &rules) {
    if s < pos {
      continue;
    }
    out.push_str(text[pos..s].trim_end_matches([' ', ',']));
    out.push_str(JOINERS[id].1);
    let rest = &text[e..];
    pos = e + (rest.len() - rest.trim_start_matches([' ', ',']).len());
  }
  out.push_str(&text[pos..]);
  out
}

/// Lowercases the first word when it is plainly capitalized ("Git" -> "git"); words with other
/// capitals (`Get-ChildItem`, `SELECT`, `McFly`) are kept.
pub fn lowercase_plain_first_word(text: &str) -> String {
  let word: String = text.chars().take_while(|c| !c.is_whitespace()).collect();
  let mut chars = word.chars();
  let plain = chars.next().is_some_and(char::is_uppercase) && chars.all(|c| !c.is_uppercase());
  if !plain {
    return text.to_string();
  }
  let first = text.chars().next().expect("non-empty");
  first.to_lowercase().collect::<String>() + &text[first.len_utf8()..]
}

/// Terminal text: no trailing period.
pub fn terminal_finish(text: &str) -> String {
  let t = text.trim_end();
  let t = if t.ends_with('.') && !t.ends_with("..") {
    &t[..t.len() - 1]
  } else {
    t
  };
  t.to_string()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ph(i: usize) -> String {
    format!("<{i}>")
  }

  fn cased(text: &str) -> (String, Vec<String>) {
    apply_casing(text, 0, ph)
  }

  #[test]
  fn casing_commands() {
    assert_eq!(
      cased("call camel case get user name."),
      ("call <0>.".into(), vec!["getUserName".into()])
    );
    assert_eq!(
      cased("Snake case, max retry count"),
      ("<0>".into(), vec!["max_retry_count".into()])
    );
    assert_eq!(
      cased("pascal case http client"),
      ("<0>".into(), vec!["HttpClient".into()])
    );
    assert_eq!(
      cased("constant case api base url"),
      ("<0>".into(), vec!["API_BASE_URL".into()])
    );
    assert_eq!(
      cased("kebab case main nav bar"),
      ("<0>".into(), vec!["main-nav-bar".into()])
    );
    // Stops at punctuation and at the next command; placeholders continue after snippets.
    let (t, ids) = apply_casing("camel case user id, then snake case user id", 2, ph);
    assert_eq!(t, "<2>, then <3>");
    assert_eq!(ids, vec!["userId", "user_id"]);
    assert_eq!(cased("camel case"), ("camel case".into(), vec![]), "nothing to case");
    assert_eq!(
      cased("camel case to do list").1,
      vec!["toDoList"],
      "first word may be a stop word"
    );
    assert_eq!(cased("set camel case retry count to three").0, "set <0> to three");
  }

  #[test]
  fn casing_takes_at_most_eight_words() {
    let (t, ids) = cased("snake case a b c d e f g h i j");
    assert_eq!(ids, vec!["a_b_c_d_e_f_g_h"]);
    assert_eq!(t, "<0> i j");
  }

  #[test]
  fn code_symbols_join() {
    assert_eq!(apply_code_symbols("console dot log"), "console.log");
    assert_eq!(apply_code_symbols("std double colon env"), "std::env");
    assert_eq!(apply_code_symbols("my underscore var"), "my_var");
    assert_eq!(
      apply_code_symbols("Vec open angle bracket u8 close angle bracket"),
      "Vec<u8>"
    );
    let prose = "we should compute the dot product of both vectors before normalizing them";
    assert_eq!(apply_code_symbols(prose), prose, "long prose keeps the word");
  }

  #[test]
  fn targets_and_terminal() {
    assert_eq!(Target::from_process("Code.exe"), Target::Editor);
    assert_eq!(Target::from_process("WindowsTerminal.exe"), Target::Terminal);
    assert_eq!(Target::from_process("slack.exe"), Target::Prose);
    assert_eq!(terminal_finish("git status."), "git status");
    assert_eq!(lowercase_plain_first_word("Git status"), "git status");
    assert_eq!(
      lowercase_plain_first_word("Get-ChildItem -Path x"),
      "Get-ChildItem -Path x"
    );
    assert_eq!(lowercase_plain_first_word("SELECT * FROM t"), "SELECT * FROM t");
    assert_eq!(terminal_finish("wait..."), "wait...");
  }
}
