//! Platform-neutral focus types and rules.

/// Selected tab inside a tabbed window (Windows 11 Notepad, browsers): tabs share one HWND,
/// so a tab switch is invisible to window-level checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabPos {
  pub index: usize,
  pub count: usize,
  /// UI Automation runtime id of the selected tab item (stable while the tab exists; empty if
  /// unknown). Preferred over the index, which shifts when an app adds transient tab items.
  pub id: Vec<i32>,
}

/// The focused control when a session starts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FocusSnapshot {
  /// UI Automation runtime id of the focused element (empty if unknown).
  pub runtime_id: Vec<i32>,
  /// Password, PIN, or other credential field: never read for context or learning.
  pub secure: bool,
  pub tab: Option<TabPos>,
}

/// Name, automation id, or class name that marks a credential field even when the control
/// does not set the password flag.
pub fn is_secure_hint(name: &str, automation_id: &str, class_name: &str) -> bool {
  // Substrings, and short tokens that only count as whole words ("otp" is inside "footprint").
  const PARTS: &[&str] = &[
    "password",
    "passwd",
    "passcode",
    "pincode",
    "one-time",
    "one time code",
    "security code",
    "secret",
    "credential",
  ];
  const WORDS: &[&str] = &["pin", "otp", "cvv", "cvc", "ssn"];
  let hay = format!("{name} {automation_id} {class_name}").to_lowercase();
  PARTS.iter().any(|h| hay.contains(h)) || hay.split(|c: char| !c.is_alphanumeric()).any(|w| WORDS.contains(&w))
}

/// Whether a different tab is selected than at start. Unknown either way counts as unchanged
/// (only a positive mismatch blocks a paste). A tab opened or closed before the selected one
/// also shifts the index; copying instead of pasting is the safe outcome then.
pub fn tab_changed(at_start: Option<&TabPos>, now: Option<&TabPos>) -> bool {
  match (at_start, now) {
    (Some(a), Some(b)) if !a.id.is_empty() && !b.id.is_empty() => a.id != b.id,
    (Some(a), Some(b)) => a.index != b.index,
    _ => false,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn secure_hints() {
    assert!(is_secure_hint("Password", "", ""));
    assert!(
      !is_secure_hint("", "txtPin", ""),
      "camelCase pin is not a word boundary"
    );
    assert!(is_secure_hint("Enter PIN", "", ""));
    assert!(is_secure_hint("", "otp-input", ""));
    assert!(is_secure_hint("Card security code", "", ""));
    assert!(!is_secure_hint("Message", "composer", "RichEditD2DPT"));
    assert!(!is_secure_hint("Pinterest search", "", ""), "pin inside a word is fine");
    assert!(!is_secure_hint("Carbon footprint notes", "", ""));
  }

  #[test]
  fn tab_change_rules() {
    let t = |index, count| TabPos {
      index,
      count,
      id: Vec::new(),
    };
    let id = |index, n: i32| TabPos {
      index,
      count: 5,
      id: vec![42, n],
    };
    assert!(!tab_changed(Some(&t(2, 5)), Some(&t(2, 5))));
    assert!(tab_changed(Some(&t(2, 5)), Some(&t(3, 5))));
    assert!(
      !tab_changed(Some(&t(2, 5)), Some(&t(2, 6))),
      "a new tab opened elsewhere"
    );
    assert!(!tab_changed(None, Some(&t(1, 2))));
    assert!(!tab_changed(Some(&t(1, 2)), None), "unknown now does not block");
    // With runtime ids, an index shift from transient items is not a switch; a new id is.
    assert!(!tab_changed(Some(&id(3, 7)), Some(&id(1, 7))));
    assert!(tab_changed(Some(&id(1, 7)), Some(&id(1, 8))));
  }
}
