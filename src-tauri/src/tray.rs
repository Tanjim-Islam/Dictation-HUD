//! System tray: Open / Check for updates / Paste last transcript / Shortcuts / Microphone >
//! / Languages > / Exit.
//!
//! The Microphone and Languages submenus reflect current settings and devices; the menu is
//! rebuilt when settings change and when the pointer reaches the tray icon (so a newly plugged
//! microphone shows up before the menu opens).

use std::sync::atomic::{AtomicU64, Ordering};

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::delivery::{self, Delivered};
use crate::services::Services;
use crate::settings::SettingsPatch;
use crate::{commands, flowbar, settings, target};

const TRAY_ID: &str = "main";
pub const OPEN_SETTINGS_EVENT: &str = "app://open-settings";
pub const CHECK_UPDATES_EVENT: &str = "app://check-updates";

/// Languages offered in the tray (the full list is in Settings), filtered by what the selected
/// model supports.
const TRAY_LANGUAGES: &[(&str, &str)] = &[
  ("multi", "Auto-detect"),
  ("en", "English"),
  ("bn", "Bengali"),
  ("hi", "Hindi"),
  ("es", "Spanish"),
  ("fr", "French"),
  ("de", "German"),
  ("pt", "Portuguese"),
  ("ar", "Arabic"),
  ("ja", "Japanese"),
];

fn show_main(app: &AppHandle) {
  if let Some(w) = app.get_webview_window("main") {
    let _ = w.unminimize();
    let _ = w.show();
    let _ = w.set_focus();
  }
}

fn languages_for(app: &AppHandle) -> Vec<(String, String)> {
  let s = settings::load(app);
  let supported: Vec<String> = app
    .state::<Services>()
    .registry
    .get(&s.stt_provider)
    .and_then(|a| {
      let d = a.descriptor();
      d.model(s.stt_model())
        .or_else(|| d.model(&d.default_model))
        .map(|m| m.languages.clone())
    })
    .unwrap_or_default();
  let mut out: Vec<(String, String)> = TRAY_LANGUAGES
    .iter()
    .filter(|(code, _)| supported.iter().any(|l| l == code))
    .map(|(c, n)| (c.to_string(), n.to_string()))
    .collect();
  if !out.iter().any(|(c, _)| *c == s.language) {
    out.push((s.language.clone(), s.language.clone()));
  }
  out
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
  let s = settings::load(app);
  let sep = || PredefinedMenuItem::separator(app);

  let mic = Submenu::with_id(app, "mic", "Microphone", true)?;
  mic.append(&CheckMenuItem::with_id(
    app,
    "mic:",
    "System default",
    true,
    s.input_device.is_none(),
    None::<&str>,
  )?)?;
  for d in crate::audio::list_input_devices() {
    let checked = s.input_device.as_deref() == Some(d.id.as_str());
    mic.append(&CheckMenuItem::with_id(
      app,
      format!("mic:{}", d.id),
      &d.name,
      true,
      checked,
      None::<&str>,
    )?)?;
  }

  let lang = Submenu::with_id(app, "lang", "Languages", true)?;
  for (code, name) in languages_for(app) {
    lang.append(&CheckMenuItem::with_id(
      app,
      format!("lang:{code}"),
      &name,
      true,
      code == s.language,
      None::<&str>,
    )?)?;
  }

  let menu = Menu::new(app)?;
  menu.append(&MenuItem::with_id(
    app,
    "open",
    "Open Dictation HUD",
    true,
    None::<&str>,
  )?)?;
  menu.append(&MenuItem::with_id(
    app,
    "updates",
    "Check for updates...",
    true,
    None::<&str>,
  )?)?;
  menu.append(&sep()?)?;
  menu.append(&MenuItem::with_id(
    app,
    "paste_last",
    "Paste last transcript",
    true,
    None::<&str>,
  )?)?;
  menu.append(&MenuItem::with_id(app, "shortcuts", "Shortcuts", true, None::<&str>)?)?;
  menu.append(&sep()?)?;
  menu.append(&mic)?;
  menu.append(&lang)?;
  menu.append(&sep()?)?;
  menu.append(&MenuItem::with_id(app, "quit", "Exit", true, None::<&str>)?)?;
  Ok(menu)
}

/// Rebuilds the menu so checks and devices are current.
pub fn refresh(app: &AppHandle) {
  let Some(tray) = app.tray_by_id(TRAY_ID) else {
    return;
  };
  match build_menu(app) {
    Ok(menu) => {
      let _ = tray.set_menu(Some(menu));
    }
    Err(e) => tracing::warn!(error = %e, "tray menu rebuild failed"),
  }
}

static LAST_HOVER_REFRESH: AtomicU64 = AtomicU64::new(0);

fn refresh_on_hover(app: &AppHandle) {
  let now = crate::db::now_ms() as u64;
  if now.saturating_sub(LAST_HOVER_REFRESH.swap(now, Ordering::SeqCst)) > 3_000 {
    refresh(app);
  }
}

fn set(app: &AppHandle, patch: SettingsPatch) {
  if let Err(e) = commands::apply_settings_patch(app, patch) {
    tracing::warn!(error = %e, "tray setting change failed");
  }
}

/// Pastes the most recent completed transcript into the last application window, with the
/// same safety rules as a dictation (copy only if that window is gone or blocked).
fn paste_last(app: &AppHandle) {
  let app = app.clone();
  tauri::async_runtime::spawn_blocking(move || {
    let text = match app.state::<crate::db::Db>().last_final_text() {
      Ok(Some(t)) => t,
      _ => {
        flowbar::notice(&app, "No transcript to paste yet.");
        return;
      }
    };
    let target = target::last_app_target();
    let result = delivery::deliver(&app, &text, target.as_ref(), None, true);
    match result {
      Delivered::Pasted => tracing::info!("pasted last transcript"),
      Delivered::Copied(reason) => {
        tracing::info!(reason = reason.code(), "last transcript copied");
        if let Some(msg) = reason.notice() {
          flowbar::notice(&app, msg);
        }
      }
      Delivered::Failed => flowbar::notice(&app, "Could not copy or paste the text."),
    }
  });
}

pub fn build(app: &tauri::App, on_quit: fn(&AppHandle)) -> tauri::Result<()> {
  let menu = build_menu(app.handle())?;
  TrayIconBuilder::with_id(TRAY_ID)
    .tooltip("Dictation HUD")
    .icon(app.default_window_icon().cloned().expect("app icon"))
    .menu(&menu)
    .show_menu_on_left_click(false)
    .on_tray_icon_event(|tray, event| match event {
      TrayIconEvent::Enter { .. } => refresh_on_hover(tray.app_handle()),
      TrayIconEvent::DoubleClick { .. } => show_main(tray.app_handle()),
      _ => {}
    })
    .on_menu_event(move |app, event| {
      let id = event.id.as_ref();
      match id {
        "open" => show_main(app),
        "updates" => {
          show_main(app);
          let _ = app.emit(OPEN_SETTINGS_EVENT, "about");
          let _ = app.emit(CHECK_UPDATES_EVENT, ());
        }
        "paste_last" => paste_last(app),
        "shortcuts" => {
          show_main(app);
          let _ = app.emit(OPEN_SETTINGS_EVENT, "general");
        }
        "quit" => on_quit(app),
        _ if id.starts_with("mic:") => set(
          app,
          SettingsPatch {
            input_device: Some(id["mic:".len()..].to_string()),
            ..Default::default()
          },
        ),
        _ if id.starts_with("lang:") => set(
          app,
          SettingsPatch {
            language: Some(id["lang:".len()..].to_string()),
            ..Default::default()
          },
        ),
        _ => {}
      }
    })
    .build(app)?;
  Ok(())
}
