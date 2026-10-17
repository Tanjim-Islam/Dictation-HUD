//! The window dictation is meant for.
//!
//! A [`Target`] is captured when a session starts and revalidated at delivery so text never
//! goes to whatever window happens to be in front later. Platform code lives in `windows.rs`;
//! other platforms get conservative stubs (no target, so delivery copies to the clipboard).

#[cfg(windows)]
mod windows;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
  /// An ordinary application window.
  App,
  /// Taskbar, desktop, Start, tray overflow: never a text target.
  Shell,
  /// One of Dictation HUD's own windows.
  OwnApp,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Target {
  /// Top-level window handle (as an integer, for logging and comparison).
  pub hwnd: isize,
  /// Focused control inside the target's GUI thread at capture time (0 if unknown).
  pub focus_hwnd: isize,
  pub pid: u32,
  pub thread_id: u32,
  /// Executable file name, e.g. `notepad.exe`.
  pub process: String,
  pub title: String,
  pub class_name: String,
  pub kind: TargetKind,
  /// The target runs elevated while we do not: synthetic input would be blocked (UIPI).
  pub elevated_mismatch: bool,
}

impl Target {
  /// Friendly app label for History, from the executable name.
  pub fn app_label(&self) -> String {
    let base = self.process.trim_end_matches(".exe").trim_end_matches(".EXE");
    match base.to_ascii_lowercase().as_str() {
      "notepad" => "Notepad".into(),
      "code" => "VS Code".into(),
      "cursor" => "Cursor".into(),
      "windsurf" => "Windsurf".into(),
      "chrome" => "Chrome".into(),
      "msedge" => "Edge".into(),
      "brave" => "Brave".into(),
      "firefox" => "Firefox".into(),
      "windowsterminal" => "Windows Terminal".into(),
      "powershell" | "pwsh" => "PowerShell".into(),
      "cmd" => "Command Prompt".into(),
      "slack" => "Slack".into(),
      "discord" => "Discord".into(),
      "winword" => "Word".into(),
      "outlook" | "olk" => "Outlook".into(),
      "chatgpt" => "ChatGPT".into(),
      "claude" => "Claude".into(),
      _ => {
        let mut c = base.chars();
        c.next()
          .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
          .unwrap_or_default()
      }
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Validation {
  /// Same window, same process, still exists.
  Valid,
  /// The window no longer exists.
  Closed,
  /// The handle was reused by a different process.
  Replaced,
}

/// What is in front right now, relative to the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Foreground {
  /// The target window itself.
  Target,
  /// Shell, our own windows, or nothing: safe to bring the target back.
  Neutral,
  /// The user moved to a different application.
  OtherApp,
}

#[cfg(windows)]
pub use self::windows::{
  activate, capture, foreground_relative_to, last_app_target, start_foreground_tracker, validate, window_center,
};

#[cfg(not(windows))]
mod stubs {
  use super::*;
  pub fn capture() -> Option<Target> {
    None
  }
  pub fn last_app_target() -> Option<Target> {
    None
  }
  pub fn validate(_: &Target) -> Validation {
    Validation::Closed
  }
  pub fn foreground_relative_to(_: &Target) -> Foreground {
    Foreground::OtherApp
  }
  pub fn activate(_: &Target) -> bool {
    false
  }
  pub fn start_foreground_tracker() {}
  pub fn window_center(_: isize) -> Option<(i32, i32)> {
    None
  }
}

#[cfg(not(windows))]
pub use stubs::*;

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn app_labels() {
    let t = |p: &str| Target {
      hwnd: 1,
      focus_hwnd: 0,
      pid: 1,
      thread_id: 1,
      process: p.into(),
      title: String::new(),
      class_name: String::new(),
      kind: TargetKind::App,
      elevated_mismatch: false,
    };
    assert_eq!(t("Code.exe").app_label(), "VS Code");
    assert_eq!(t("notepad.exe").app_label(), "Notepad");
    assert_eq!(t("obsidian.exe").app_label(), "Obsidian");
  }
}
