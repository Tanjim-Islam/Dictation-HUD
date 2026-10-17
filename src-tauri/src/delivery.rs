//! Safe delivery of the final text.
//!
//! The text always goes to the clipboard first. A paste keystroke is sent only when the
//! session's target window still exists, belongs to the same process, and is (or can safely be
//! brought back to) the foreground. If the user moved to another application, the target
//! closed, or input would be blocked, the text stays on the clipboard and the reason is
//! reported. After a successful paste the previous clipboard text is restored.

use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::target::{self, Foreground, Target, TargetKind, Validation};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyReason {
  AutoPasteOff,
  NoTarget,
  TargetClosed,
  TargetChanged,
  SwitchedApp,
  ElevatedTarget,
  FocusFailed,
  KeystrokeFailed,
}

impl CopyReason {
  pub fn code(self) -> &'static str {
    match self {
      CopyReason::AutoPasteOff => "auto_paste_off",
      CopyReason::NoTarget => "no_target",
      CopyReason::TargetClosed => "target_closed",
      CopyReason::TargetChanged => "target_changed",
      CopyReason::SwitchedApp => "switched_app",
      CopyReason::ElevatedTarget => "elevated_target",
      CopyReason::FocusFailed => "focus_failed",
      CopyReason::KeystrokeFailed => "keystroke_failed",
    }
  }

  /// Short explanation shown in the Flow Bar notice.
  pub fn notice(self) -> Option<&'static str> {
    match self {
      CopyReason::AutoPasteOff => None,
      CopyReason::NoTarget => Some("Copied. No text field was active when you started."),
      CopyReason::TargetClosed | CopyReason::TargetChanged => Some("Copied. The original window was closed."),
      CopyReason::SwitchedApp => Some("Copied. You switched to another app."),
      CopyReason::ElevatedTarget => Some("Copied. That app runs as administrator, so it can't be typed into."),
      CopyReason::FocusFailed => Some("Copied. The original window could not be focused."),
      CopyReason::KeystrokeFailed => Some("Copied. The paste keystroke was blocked."),
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Delivered {
  Pasted,
  Copied(CopyReason),
  /// Even the clipboard write failed.
  Failed,
}

/// Decides whether a paste is safe. Pure, so the rules are unit tested.
pub fn decide(
  auto_paste: bool,
  target: Option<&Target>,
  validation: Option<Validation>,
  fg: Option<Foreground>,
) -> Result<(), CopyReason> {
  if !auto_paste {
    return Err(CopyReason::AutoPasteOff);
  }
  let Some(t) = target else {
    return Err(CopyReason::NoTarget);
  };
  if t.kind != TargetKind::App {
    return Err(CopyReason::NoTarget);
  }
  match validation {
    Some(Validation::Valid) => {}
    Some(Validation::Replaced) => return Err(CopyReason::TargetChanged),
    _ => return Err(CopyReason::TargetClosed),
  }
  if t.elevated_mismatch {
    return Err(CopyReason::ElevatedTarget);
  }
  if fg == Some(Foreground::OtherApp) {
    return Err(CopyReason::SwitchedApp);
  }
  Ok(())
}

fn write_clipboard(app: &AppHandle, text: &str) -> bool {
  // Another app may hold the clipboard open briefly; retry instead of failing.
  for attempt in 0..6 {
    if app.clipboard().write_text(text.to_string()).is_ok() {
      return true;
    }
    std::thread::sleep(Duration::from_millis(30 * (attempt + 1)));
  }
  false
}

#[cfg(windows)]
mod keys {
  use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
  };

  fn held(vk: VIRTUAL_KEY) -> bool {
    (unsafe { GetAsyncKeyState(vk.0 as i32) } as u16 & 0x8000) != 0
  }

  /// Waits (bounded) for the user to release shortcut modifiers so Ctrl+V is not read as
  /// Ctrl+Shift+V or Ctrl+Alt+V.
  pub fn wait_modifiers_released(max: std::time::Duration) {
    let deadline = std::time::Instant::now() + max;
    while std::time::Instant::now() < deadline {
      if ![VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN, VK_CONTROL]
        .iter()
        .any(|k| held(*k))
      {
        return;
      }
      std::thread::sleep(std::time::Duration::from_millis(20));
    }
  }

  pub fn send_paste() -> bool {
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
      r#type: INPUT_KEYBOARD,
      Anonymous: INPUT_0 {
        ki: KEYBDINPUT {
          wVk: vk,
          wScan: 0,
          dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
          time: 0,
          dwExtraInfo: 0,
        },
      },
    };
    const VK_V: VIRTUAL_KEY = VIRTUAL_KEY(0x56);
    let inputs = [
      key(VK_CONTROL, false),
      key(VK_V, false),
      key(VK_V, true),
      key(VK_CONTROL, true),
    ];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    sent as usize == inputs.len()
  }
}

#[cfg(not(windows))]
mod keys {
  pub fn wait_modifiers_released(_: std::time::Duration) {}
  pub fn send_paste() -> bool {
    false
  }
}

/// Delivers `text` for a session. Blocking (focus changes and key timing); call from a
/// blocking-capable task.
pub fn deliver(app: &AppHandle, text: &str, target: Option<&Target>, auto_paste: bool) -> Delivered {
  let previous = app.clipboard().read_text().ok();
  if !write_clipboard(app, text) {
    tracing::error!("clipboard write failed");
    return Delivered::Failed;
  }
  let validation = target.map(target::validate);
  let fg = target.map(target::foreground_relative_to);
  if let Err(reason) = decide(auto_paste, target, validation, fg) {
    // When a window closes, focus jumps to another app before the handle disappears; report
    // the real cause.
    if reason == CopyReason::SwitchedApp {
      std::thread::sleep(Duration::from_millis(200));
      if target.map(target::validate) == Some(Validation::Closed) {
        return Delivered::Copied(CopyReason::TargetClosed);
      }
    }
    return Delivered::Copied(reason);
  }
  let t = target.expect("decide checked");
  keys::wait_modifiers_released(Duration::from_millis(1500));
  // Bring the target back (if a neutral window was in front) and restore the focused control.
  // This also cancels a menu/access-key mode the shortcut's modifier release may have opened.
  if !target::activate(t) {
    // The window may have been closing when it was first validated.
    return Delivered::Copied(match target::validate(t) {
      Validation::Closed => CopyReason::TargetClosed,
      Validation::Replaced => CopyReason::TargetChanged,
      Validation::Valid => CopyReason::FocusFailed,
    });
  }
  // Last check right before the keystroke: still the same window in front.
  if target::foreground_relative_to(t) != Foreground::Target {
    return Delivered::Copied(CopyReason::FocusFailed);
  }
  std::thread::sleep(Duration::from_millis(40));
  if !keys::send_paste() {
    return Delivered::Copied(CopyReason::KeystrokeFailed);
  }
  restore_clipboard_later(app.clone(), text.to_string(), previous);
  Delivered::Pasted
}

/// Puts the previous clipboard text back once the target had time to read the paste, unless
/// something else changed the clipboard in the meantime.
fn restore_clipboard_later(app: AppHandle, ours: String, previous: Option<String>) {
  let Some(previous) = previous.filter(|p| !p.is_empty() && *p != ours) else {
    return;
  };
  std::thread::spawn(move || {
    let started = Instant::now();
    std::thread::sleep(Duration::from_millis(900));
    if app.clipboard().read_text().ok().as_deref() == Some(ours.as_str()) {
      let _ = write_clipboard(&app, &previous);
      tracing::debug!(ms = started.elapsed().as_millis() as u64, "clipboard restored");
    }
  });
}

#[cfg(test)]
mod tests {
  use super::*;

  fn t(kind: TargetKind, elevated: bool) -> Target {
    Target {
      hwnd: 1,
      focus_hwnd: 2,
      pid: 3,
      thread_id: 4,
      process: "notepad.exe".into(),
      title: "x".into(),
      class_name: "Notepad".into(),
      kind,
      elevated_mismatch: elevated,
    }
  }

  #[test]
  fn paste_only_when_safe() {
    let app = t(TargetKind::App, false);
    let ok = decide(true, Some(&app), Some(Validation::Valid), Some(Foreground::Target));
    assert_eq!(ok, Ok(()));
    // Tray click or the Flow Bar in front: target is brought back.
    assert_eq!(
      decide(true, Some(&app), Some(Validation::Valid), Some(Foreground::Neutral)),
      Ok(())
    );
  }

  #[test]
  fn never_pastes_into_another_app() {
    let app = t(TargetKind::App, false);
    assert_eq!(
      decide(true, Some(&app), Some(Validation::Valid), Some(Foreground::OtherApp)),
      Err(CopyReason::SwitchedApp)
    );
  }

  #[test]
  fn closed_or_reused_window_copies() {
    let app = t(TargetKind::App, false);
    assert_eq!(
      decide(true, Some(&app), Some(Validation::Closed), Some(Foreground::Neutral)),
      Err(CopyReason::TargetClosed)
    );
    assert_eq!(
      decide(true, Some(&app), Some(Validation::Replaced), Some(Foreground::Neutral)),
      Err(CopyReason::TargetChanged)
    );
  }

  #[test]
  fn shell_own_app_elevated_and_off_copy() {
    assert_eq!(decide(true, None, None, None), Err(CopyReason::NoTarget));
    let shell = t(TargetKind::Shell, false);
    assert_eq!(
      decide(true, Some(&shell), Some(Validation::Valid), Some(Foreground::Target)),
      Err(CopyReason::NoTarget)
    );
    let own = t(TargetKind::OwnApp, false);
    assert_eq!(
      decide(true, Some(&own), Some(Validation::Valid), Some(Foreground::Target)),
      Err(CopyReason::NoTarget)
    );
    let admin = t(TargetKind::App, true);
    assert_eq!(
      decide(true, Some(&admin), Some(Validation::Valid), Some(Foreground::Target)),
      Err(CopyReason::ElevatedTarget)
    );
    let app = t(TargetKind::App, false);
    assert_eq!(
      decide(false, Some(&app), Some(Validation::Valid), Some(Foreground::Target)),
      Err(CopyReason::AutoPasteOff)
    );
  }

  #[test]
  fn every_visible_reason_has_a_notice() {
    for r in [
      CopyReason::NoTarget,
      CopyReason::TargetClosed,
      CopyReason::TargetChanged,
      CopyReason::SwitchedApp,
      CopyReason::ElevatedTarget,
      CopyReason::FocusFailed,
      CopyReason::KeystrokeFailed,
    ] {
      assert!(r.notice().is_some(), "{r:?}");
    }
    assert!(CopyReason::AutoPasteOff.notice().is_none());
  }
}
