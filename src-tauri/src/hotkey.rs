//! Global dictation shortcut, registered and handled in Rust.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tauri_plugin_store::StoreExt;

use crate::session::{Input, SessionHandle, Trigger};

pub const DEFAULT_HOTKEY: &str = "Ctrl+Shift+Alt+H";

/// A press counts only after this much quiet since the previous press (accepted or not), so
/// a burst of presses acts once.
const PRESS_GUARD: Duration = Duration::from_millis(400);
static LAST_PRESS: Mutex<Option<Instant>> = Mutex::new(None);

fn accept_press(now: Instant) -> bool {
  let mut last = LAST_PRESS.lock().unwrap();
  let accept = last.is_none_or(|prev| now.saturating_duration_since(prev) >= PRESS_GUARD);
  *last = Some(now);
  accept
}

/// The shortcut swallows its main key, so the focused app sees Alt (or Win) pressed and
/// released with nothing in between. Windows treats that as "open the menu bar" (Alt) or
/// "open Start" (Win), which steals keyboard focus from the text field and breaks the later
/// paste. Injecting an unassigned virtual key while the modifier is still down cancels that,
/// the same "menu mask key" technique AutoHotkey uses.
#[cfg(windows)]
fn mask_modifier_release() {
  use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY,
  };
  const VK_UNASSIGNED: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);
  let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
    r#type: INPUT_KEYBOARD,
    Anonymous: INPUT_0 {
      ki: KEYBDINPUT {
        wVk: VK_UNASSIGNED,
        wScan: 0,
        dwFlags: flags,
        time: 0,
        dwExtraInfo: 0,
      },
    },
  };
  let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
  let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
  tracing::debug!(sent, "modifier release masked");
}

#[cfg(not(windows))]
fn mask_modifier_release() {}

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
  tauri_plugin_global_shortcut::Builder::new()
    .with_handler(|app, shortcut, event| {
      use tauri_plugin_global_shortcut::Modifiers;
      tracing::debug!(state = ?event.state(), mods = ?shortcut.mods, "shortcut event");
      if event.state() == ShortcutState::Pressed && shortcut.mods.intersects(Modifiers::ALT | Modifiers::SUPER) {
        mask_modifier_release();
      }
      // The OS reports both press and release; only a press is an intent.
      if event.state() == ShortcutState::Pressed && accept_press(Instant::now()) {
        if let Some(session) = app.try_state::<SessionHandle>() {
          session.try_send(Input::Toggle(Trigger::Hotkey));
        }
      }
    })
    .build()
}

pub fn get_hotkey(app: &AppHandle) -> String {
  app
    .store("prefs.json")
    .ok()
    .and_then(|s| s.get("hotkey"))
    .and_then(|v| v.as_str().map(str::to_string))
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| DEFAULT_HOTKEY.into())
}

fn parse(combo: &str) -> Result<Shortcut, String> {
  combo.parse::<Shortcut>().map_err(|e| format!("Invalid shortcut: {e}"))
}

fn register_only(app: &AppHandle, combo: &str) -> Result<(), String> {
  let shortcut = parse(combo)?;
  let gs = app.global_shortcut();
  gs.unregister_all().map_err(|e| e.to_string())?;
  gs.register(shortcut)
    .map_err(|e| format!("Could not register shortcut: {e}"))
}

/// Registers the stored shortcut at startup, falling back to the default.
pub fn register_stored(app: &AppHandle) {
  let combo = get_hotkey(app);
  if let Err(e) = register_only(app, &combo) {
    tracing::warn!(error = %e, "stored shortcut failed; trying default");
    if let Err(e) = register_only(app, DEFAULT_HOTKEY) {
      tracing::error!(error = %e, "default shortcut failed");
    }
  }
}

/// Registers a new shortcut and persists it only if registration succeeded. On failure the
/// previous shortcut is restored.
pub fn set_hotkey(app: &AppHandle, combo: &str) -> Result<(), String> {
  let previous = get_hotkey(app);
  parse(combo)?;
  if let Err(e) = register_only(app, combo) {
    let _ = register_only(app, &previous);
    return Err(e);
  }
  let store = app.store("prefs.json").map_err(|e| e.to_string())?;
  store.set("hotkey", combo);
  store.save().map_err(|e| e.to_string())?;
  tracing::info!(combo, "shortcut registered");
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_and_recorder_formats_parse() {
    for combo in [DEFAULT_HOTKEY, "Ctrl+Space", "Alt+F9", "Ctrl+Shift+D"] {
      assert!(parse(combo).is_ok(), "{combo}");
    }
    assert!(parse("NotAKey+++").is_err());
  }

  #[test]
  fn press_guard_drops_chatter() {
    let t0 = Instant::now() + Duration::from_secs(3600);
    assert!(accept_press(t0));
    assert!(!accept_press(t0 + Duration::from_millis(250)));
    assert!(
      !accept_press(t0 + Duration::from_millis(500)),
      "burst keeps resetting the guard"
    );
    assert!(accept_press(t0 + Duration::from_millis(950)));
  }
}
