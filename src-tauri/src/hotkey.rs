use tauri::AppHandle;
use tauri_plugin_store::StoreExt;
// shortcut registration is handled on the frontend via the JS plugin

pub fn ensure_default_hotkey(app: tauri::AppHandle) -> Result<(), String> {
  let store = app.store("prefs.json").map_err(|e| e.to_string())?;
  let default = if cfg!(target_os = "macos") { "Control+Shift+Alt+H" } else { "Ctrl+Shift+Alt+H" };
  let combo = store.get("hotkey").and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or(default.into());
  set_hotkey(&app, &combo)?;
  Ok(())
}

pub fn set_hotkey(app: &AppHandle, combo: &str) -> Result<(), String> {
  let store = app.store("prefs.json").map_err(|e| e.to_string())?; store.set("hotkey", combo); store.save().map_err(|e| e.to_string())?; Ok(())
}

pub fn get_hotkey(app: &AppHandle) -> String {
  let store = match app.store("prefs.json") { Ok(s) => s, Err(_) => return if cfg!(target_os = "macos") {"Control+Shift+Alt+H".into()} else {"Ctrl+Shift+Alt+H".into()} };
  store.get("hotkey").and_then(|v| v.as_str().map(|s| s.to_string())).unwrap_or_else(|| if cfg!(target_os = "macos") {"Control+Shift+Alt+H".into()} else {"Ctrl+Shift+Alt+H".into()})
}
