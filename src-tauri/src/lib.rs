pub mod config;
pub mod hotkey;
pub mod logging;
pub mod paste;
pub mod prompt;
pub mod refine;
pub mod symbols;

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{
  menu::{Menu, MenuItem},
  tray::{TrayIconBuilder, TrayIconEvent},
  AppHandle, Emitter, Manager,
};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_store::StoreExt;

use logging::text_shape;

// Helper for choosing which monitor the HUD should appear on.
#[cfg(all(target_os = "windows", feature = "windows-monitor"))]
mod focused_monitor {
  use windows::Win32::Foundation::{POINT, RECT};
  use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
  use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

  /// Returns (left, top, width, height) of the work area of the monitor under the cursor.
  pub fn work_area_for_foreground_monitor() -> Option<(i32, i32, u32, u32)> {
    unsafe {
      let mut pt = POINT { x: 0, y: 0 };
      if GetCursorPos(&mut pt).is_err() {
        return None;
      }
      let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
      let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
      };
      if !GetMonitorInfoW(hmon, &mut info).as_bool() {
        return None;
      }
      let RECT {
        left,
        top,
        right,
        bottom,
      } = info.rcWork;
      Some((left, top, (right - left) as u32, (bottom - top) as u32))
    }
  }
}

#[cfg(not(all(target_os = "windows", feature = "windows-monitor")))]
mod focused_monitor {
  pub fn work_area_for_foreground_monitor() -> Option<(i32, i32, u32, u32)> {
    None
  }
}

pub const BEHAVIOR_VERSION: u32 = 1;

/// Behavior preferences. Every field has a default so an older or partial stored object
/// keeps the fields it has instead of resetting everything.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BehaviorPrefs {
  pub version: u32,
  pub auto_paste: bool,
  pub autostart: bool,
  pub ai_refine: bool,
  pub ai_provider: String,  // "openrouter" | "megallm"
  pub stt_provider: String, // "deepgram" | "elevenlabs"
  pub echo_cancellation: bool,
  pub noise_suppression: bool,
}

impl Default for BehaviorPrefs {
  fn default() -> Self {
    Self {
      version: BEHAVIOR_VERSION,
      auto_paste: true,
      autostart: false,
      ai_refine: true,
      ai_provider: "openrouter".into(),
      stt_provider: "deepgram".into(),
      echo_cancellation: true,
      noise_suppression: true,
    }
  }
}

impl BehaviorPrefs {
  /// Parses a stored value. Unknown providers fall back to defaults; missing fields keep
  /// their defaults; a non-object value yields defaults.
  pub fn from_stored(value: serde_json::Value) -> Self {
    let mut prefs: BehaviorPrefs = serde_json::from_value(value).unwrap_or_default();
    if prefs.ai_provider != "openrouter" && prefs.ai_provider != "megallm" {
      prefs.ai_provider = "openrouter".into();
    }
    if prefs.stt_provider != "deepgram" && prefs.stt_provider != "elevenlabs" {
      prefs.stt_provider = "deepgram".into();
    }
    prefs.version = BEHAVIOR_VERSION;
    prefs
  }
}

fn load_behavior(app: &AppHandle) -> BehaviorPrefs {
  match app.store("prefs.json") {
    Ok(store) => store
      .get("behavior")
      .map(BehaviorPrefs::from_stored)
      .unwrap_or_default(),
    Err(_) => BehaviorPrefs::default(),
  }
}

fn save_behavior(app: &AppHandle, prefs: &BehaviorPrefs) -> Result<(), String> {
  let store = app.store("prefs.json").map_err(|e| e.to_string())?;
  store.set("behavior", serde_json::to_value(prefs).map_err(|e| e.to_string())?);
  store.save().map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum DictationState {
  Inactive,
  Starting,
  Recording,
  Stopping,
}

struct RecordingState {
  state: DictationState,
  start_time: Option<Instant>,
}

static RECORDING_STATE: Mutex<RecordingState> = Mutex::new(RecordingState {
  state: DictationState::Inactive,
  start_time: None,
});

#[tauri::command]
async fn start_dictation(app: AppHandle) -> Result<(), String> {
  {
    let mut state = RECORDING_STATE.lock().unwrap();
    match state.state {
      DictationState::Starting => return Err("already-starting".into()),
      DictationState::Recording => return Err("already-recording".into()),
      DictationState::Stopping => return Err("currently-stopping".into()),
      DictationState::Inactive => state.state = DictationState::Starting,
    }
  }
  tracing::info!("dictation starting");

  let Some(win) = app.get_webview_window("hud") else {
    RECORDING_STATE.lock().unwrap().state = DictationState::Inactive;
    return Err("hud-window-not-found".into());
  };

  if let Some((left, top, width, height)) = focused_monitor::work_area_for_foreground_monitor() {
    let hud_width = 600;
    let hud_height = 60;
    let x = left + ((width as i32 - hud_width) / 2);
    let y = top + (height as i32 - hud_height - 60);
    let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
  } else if let Ok(Some(monitor)) = win.primary_monitor() {
    let size = monitor.size();
    let x = (size.width as i32 - 600) / 2;
    let y = size.height as i32 - 120 - 60;
    let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
  }

  let _ = win.show();
  let _ = win.set_always_on_top(true);
  // Do not focus the HUD; focus must stay in the user's text field.
  app.emit_to("hud", "dictation-start", ()).ok();
  Ok(())
}

#[tauri::command]
async fn stop_dictation(app: AppHandle) -> Result<(), String> {
  if let Some(win) = app.get_webview_window("hud") {
    let _ = win.hide();
  }
  Ok(())
}

#[tauri::command]
fn is_dictation_active() -> Result<bool, String> {
  let state = RECORDING_STATE.lock().unwrap();
  Ok(!matches!(state.state, DictationState::Inactive))
}

#[tauri::command]
fn set_recording_active(new_state: String) -> Result<(), String> {
  let mut state = RECORDING_STATE.lock().unwrap();
  match new_state.as_str() {
    "recording" => {
      state.state = DictationState::Recording;
      state.start_time = Some(Instant::now());
    }
    "stopping" => state.state = DictationState::Stopping,
    "inactive" => {
      if let Some(start) = state.start_time.take() {
        tracing::info!(secs = start.elapsed().as_secs_f32(), "dictation ended");
      }
      state.state = DictationState::Inactive;
    }
    _ => return Err(format!("Invalid state: {new_state}")),
  }
  tracing::debug!(state = %new_state, "recording state set");
  Ok(())
}

#[tauri::command]
async fn trigger_stop_dictation(app: AppHandle) -> Result<(), String> {
  app.emit_to("hud", "dictation-stop", ()).ok();
  Ok(())
}

/// Deterministic formatting, then optional AI refinement. Always returns usable text when
/// the input has words: AI failures fall back to the deterministic result.
#[tauri::command]
async fn refine_text(raw_text: String, app: AppHandle) -> Result<String, String> {
  let with_symbols = symbols::replace_symbols(&raw_text);
  let behavior = load_behavior(&app);
  if !behavior.ai_refine {
    tracing::info!(input = %text_shape(&with_symbols), "refinement off; deterministic text used");
    return Ok(with_symbols);
  }
  match refine::refine_with_provider(&app, &behavior.ai_provider, &with_symbols).await {
    Ok(refined) => Ok(refined),
    Err(_) => Ok(refine::basic_punctuation_cleanup(&with_symbols)),
  }
}

#[tauri::command]
async fn save_keys_secure(
  app: AppHandle,
  openrouter: String,
  deepgram: String,
  megallm: String,
  elevenlabs: String,
) -> Result<(), String> {
  if !openrouter.is_empty() {
    config::set_openrouter_key(&app, &openrouter)
      .await
      .map_err(|e| e.to_string())?;
  }
  if !deepgram.is_empty() {
    config::set_deepgram_key(&app, &deepgram)
      .await
      .map_err(|e| e.to_string())?;
  }
  if !megallm.is_empty() {
    config::set_megallm_key(&app, &megallm)
      .await
      .map_err(|e| e.to_string())?;
  }
  if !elevenlabs.is_empty() {
    config::set_elevenlabs_key(&app, &elevenlabs)
      .await
      .map_err(|e| e.to_string())?;
  }
  Ok(())
}

#[tauri::command]
async fn get_keys_secure(app: AppHandle) -> Result<(bool, bool, bool, bool), String> {
  Ok((
    config::get_openrouter_key(&app).await.is_some(),
    config::get_deepgram_key(&app).await.is_some(),
    config::get_megallm_key(&app).await.is_some(),
    config::get_elevenlabs_key(&app).await.is_some(),
  ))
}

#[tauri::command]
async fn set_hotkey(app: AppHandle, combo: String) -> Result<(), String> {
  hotkey::set_hotkey(&app, &combo)
}

#[tauri::command]
async fn get_hotkey(app: AppHandle) -> Result<String, String> {
  Ok(hotkey::get_hotkey(&app))
}

#[tauri::command]
async fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
  let autolaunch = app.autolaunch();
  if enabled {
    autolaunch.enable().map_err(|e| e.to_string())?;
  } else {
    autolaunch.disable().map_err(|e| e.to_string())?;
  }
  let mut prefs = load_behavior(&app);
  prefs.autostart = enabled;
  save_behavior(&app, &prefs)?;
  tracing::info!(enabled, "autostart updated");
  Ok(())
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> Result<bool, String> {
  app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct BehaviorPatch {
  auto_paste: Option<bool>,
  ai_refine: Option<bool>,
  ai_provider: Option<String>,
  stt_provider: Option<String>,
  echo_cancellation: Option<bool>,
  noise_suppression: Option<bool>,
}

#[tauri::command]
async fn set_behavior(app: AppHandle, patch: BehaviorPatch) -> Result<BehaviorPrefs, String> {
  let mut prefs = load_behavior(&app);
  if let Some(v) = patch.auto_paste {
    prefs.auto_paste = v;
  }
  if let Some(v) = patch.ai_refine {
    prefs.ai_refine = v;
  }
  if let Some(v) = patch.ai_provider.map(|s| s.to_lowercase()) {
    if v == "openrouter" || v == "megallm" {
      prefs.ai_provider = v;
    }
  }
  if let Some(v) = patch.stt_provider.map(|s| s.to_lowercase()) {
    if v == "deepgram" || v == "elevenlabs" {
      prefs.stt_provider = v;
    }
  }
  if let Some(v) = patch.echo_cancellation {
    prefs.echo_cancellation = v;
  }
  if let Some(v) = patch.noise_suppression {
    prefs.noise_suppression = v;
  }
  save_behavior(&app, &prefs)?;
  tracing::info!(?prefs, "behavior saved");
  Ok(prefs)
}

#[tauri::command]
async fn get_behavior(app: AppHandle) -> Result<BehaviorPrefs, String> {
  let mut prefs = load_behavior(&app);
  // The OS is authoritative for autostart.
  if let Ok(os_enabled) = app.autolaunch().is_enabled() {
    prefs.autostart = os_enabled;
  }
  Ok(prefs)
}

#[tauri::command]
async fn set_model(app: AppHandle, name: String) -> Result<(), String> {
  config::set_model(&app, &name).await.map_err(|e| e.to_string())
}
#[tauri::command]
async fn get_model(app: AppHandle) -> Result<String, String> {
  Ok(
    config::get_model(&app)
      .await
      .unwrap_or_else(|| "openai/gpt-oss-20b:free".into()),
  )
}
#[tauri::command]
async fn set_megallm_model(app: AppHandle, name: String) -> Result<(), String> {
  config::set_megallm_model(&app, &name).await.map_err(|e| e.to_string())
}
#[tauri::command]
async fn get_megallm_model(app: AppHandle) -> Result<String, String> {
  Ok(config::get_megallm_model(&app).await.unwrap_or_else(|| "gpt-4".into()))
}
#[tauri::command]
async fn set_language(app: AppHandle, code: String) -> Result<(), String> {
  config::set_language(&app, &code).await.map_err(|e| e.to_string())
}
#[tauri::command]
async fn get_language(app: AppHandle) -> Result<String, String> {
  Ok(config::get_language(&app).await.unwrap_or_else(|| "en-US".into()))
}

#[tauri::command]
async fn test_openrouter(app: AppHandle) -> Result<(), String> {
  refine::refine_with_provider(&app, "openrouter", "this is a connection test")
    .await
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Cheap Deepgram key validation (no audio, no credits).
#[tauri::command]
async fn test_deepgram(app: AppHandle) -> Result<(), String> {
  let key = config::get_deepgram_key(&app).await.ok_or("Missing Deepgram key")?;
  let resp = http_client()?
    .get("https://api.deepgram.com/v1/auth/token")
    .header("authorization", format!("Token {key}"))
    .send()
    .await
    .map_err(|_| "Deepgram network error".to_string())?;
  match resp.status().as_u16() {
    200..=299 => Ok(()),
    401 | 403 => Err("Deepgram rejected the key".into()),
    code => Err(format!("Deepgram HTTP {code}")),
  }
}

#[tauri::command]
async fn test_megallm(app: AppHandle) -> Result<(), String> {
  list_megallm_models(app).await.map(|_| ())
}

fn http_client() -> Result<reqwest::Client, String> {
  reqwest::Client::builder()
    .timeout(Duration::from_secs(5))
    .build()
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_elevenlabs_token(app: AppHandle) -> Result<String, String> {
  let key = config::get_elevenlabs_key(&app).await.ok_or("Missing ElevenLabs key")?;
  let resp = http_client()?
    .post("https://api.elevenlabs.io/v1/single-use-token/realtime_scribe")
    .header("xi-api-key", key)
    .header("content-length", "0")
    .body("")
    .send()
    .await
    .map_err(|_| "ElevenLabs network error".to_string())?;
  let status = resp.status();
  if !status.is_success() {
    return Err(format!("ElevenLabs HTTP {}", status.as_u16()));
  }
  let v: serde_json::Value = resp
    .json()
    .await
    .map_err(|_| "ElevenLabs malformed response".to_string())?;
  v.get("token")
    .and_then(|t| t.as_str())
    .map(str::to_string)
    .ok_or_else(|| "Missing token in ElevenLabs response".into())
}

#[tauri::command]
async fn test_elevenlabs(app: AppHandle) -> Result<(), String> {
  create_elevenlabs_token(app).await.map(|_| ())
}

#[tauri::command]
async fn list_megallm_models(app: AppHandle) -> Result<Vec<String>, String> {
  let key = config::get_megallm_key(&app).await.ok_or("Missing MegaLLM key")?;
  let resp = http_client()?
    .get("https://ai.megallm.io/v1/models")
    .bearer_auth(key)
    .send()
    .await
    .map_err(|_| "MegaLLM network error".to_string())?;
  let status = resp.status();
  if !status.is_success() {
    return Err(format!("MegaLLM HTTP {}", status.as_u16()));
  }
  let v: serde_json::Value = resp
    .json()
    .await
    .map_err(|_| "MegaLLM malformed response".to_string())?;
  Ok(
    v["data"]
      .as_array()
      .map(|arr| {
        arr
          .iter()
          .filter_map(|m| m["id"].as_str().map(str::to_string))
          .collect()
      })
      .unwrap_or_default(),
  )
}

#[tauri::command]
async fn insert_text(app: AppHandle, text: String) -> Result<bool, String> {
  let auto_paste = load_behavior(&app).auto_paste;
  let pasted = paste::copy_and_paste(&app, &text, auto_paste).await?;
  tracing::info!(text = %text_shape(&text), pasted, auto_paste, "text delivered");
  Ok(pasted)
}

/// Temporary: the WebView still runs provider sockets until Phase 2 moves them to Rust.
#[tauri::command]
async fn runtime_keys(
  app: AppHandle,
) -> Result<(Option<String>, Option<String>, Option<String>, Option<String>), String> {
  Ok((
    config::get_openrouter_key(&app).await,
    config::get_deepgram_key(&app).await,
    config::get_megallm_key(&app).await,
    config::get_elevenlabs_key(&app).await,
  ))
}

/// Frontend diagnostics. Messages must be content-free; they are length-capped here.
#[tauri::command]
fn log_to_terminal(message: String) {
  let capped: String = message.chars().take(300).collect();
  tracing::info!(target: "frontend", "{capped}");
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
  let menu = Menu::new(app)?;
  let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
  let start = MenuItem::with_id(app, "start", "Start Dictation", true, None::<&str>)?;
  let stop = MenuItem::with_id(app, "stop", "Stop Dictation", true, None::<&str>)?;
  let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
  menu.append(&settings)?;
  menu.append(&start)?;
  menu.append(&stop)?;
  menu.append(&quit)?;
  let _tray = TrayIconBuilder::with_id("main")
    .tooltip("Dictation HUD")
    .icon(app.default_window_icon().unwrap().clone())
    .menu(&menu)
    .on_menu_event(|app, event| {
      tracing::debug!(id = event.id.as_ref(), "tray menu event");
      match event.id.as_ref() {
        "settings" => {
          if let Some(w) = app.get_webview_window("settings") {
            let _ = w.show();
            let _ = w.set_focus();
          }
        }
        "start" => {
          let app = app.clone();
          tauri::async_runtime::spawn(async move {
            if let Err(e) = start_dictation(app).await {
              tracing::warn!(error = %e, "tray start failed");
            }
          });
        }
        "stop" => {
          let app = app.clone();
          tauri::async_runtime::spawn(async move {
            let _ = trigger_stop_dictation(app).await;
          });
        }
        "quit" => app.exit(0),
        _ => {}
      }
    })
    .on_tray_icon_event(|_app, _ev: TrayIconEvent| {})
    .build(app)?;
  Ok(())
}

pub fn run(context: tauri::Context<tauri::Wry>) -> tauri::Result<()> {
  tauri::Builder::default()
    .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
      if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
      }
    }))
    .plugin(tauri_plugin_store::Builder::default().build())
    .plugin(tauri_plugin_autostart::init(
      tauri_plugin_autostart::MacosLauncher::LaunchAgent,
      None,
    ))
    .plugin(tauri_plugin_global_shortcut::Builder::new().build())
    .plugin(tauri_plugin_clipboard_manager::init())
    .plugin(tauri_plugin_updater::Builder::new().build())
    .plugin(tauri_plugin_process::init())
    .setup(|app| {
      if let Ok(dir) = app.path().app_log_dir() {
        if let Some(guard) = logging::init(&dir) {
          app.manage(guard);
        }
      }
      tracing::info!(version = %app.package_info().version, "Dictation HUD starting");
      if let Some(s) = app.get_webview_window("settings") {
        let _ = s.hide();
      }
      if let Some(h) = app.get_webview_window("hud") {
        let _ = h.hide();
        let _ = h.set_decorations(false);
        let _ = h.set_always_on_top(true);
      }
      build_tray(app)?;
      let _ = hotkey::ensure_default_hotkey(app.handle().clone());
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      start_dictation,
      stop_dictation,
      is_dictation_active,
      set_recording_active,
      trigger_stop_dictation,
      refine_text,
      save_keys_secure,
      get_keys_secure,
      set_hotkey,
      get_hotkey,
      set_autostart,
      set_behavior,
      get_behavior,
      set_model,
      get_model,
      set_megallm_model,
      get_megallm_model,
      set_language,
      get_language,
      test_openrouter,
      test_deepgram,
      test_megallm,
      test_elevenlabs,
      list_megallm_models,
      create_elevenlabs_token,
      insert_text,
      runtime_keys,
      log_to_terminal,
      get_autostart
    ])
    .run(context)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn partial_behavior_keeps_defaults_for_missing_fields() {
    let stored = serde_json::json!({ "ai_refine": false, "auto_paste": false });
    let prefs = BehaviorPrefs::from_stored(stored);
    assert!(!prefs.ai_refine);
    assert!(!prefs.auto_paste);
    assert!(prefs.echo_cancellation);
    assert!(prefs.noise_suppression);
    assert_eq!(prefs.stt_provider, "deepgram");
  }

  #[test]
  fn legacy_behavior_with_removed_fields_loads() {
    let stored = serde_json::json!({
      "auto_paste": true, "silence_secs": 2, "stream_insert": false, "autostart": true,
      "ai_refine": true, "ai_provider": "megallm", "stt_provider": "elevenlabs",
      "echo_cancellation": false, "noise_suppression": true
    });
    let prefs = BehaviorPrefs::from_stored(stored);
    assert_eq!(prefs.ai_provider, "megallm");
    assert_eq!(prefs.stt_provider, "elevenlabs");
    assert!(!prefs.echo_cancellation);
    assert!(prefs.autostart);
    assert_eq!(prefs.version, BEHAVIOR_VERSION);
  }

  #[test]
  fn invalid_values_fall_back_safely() {
    let prefs = BehaviorPrefs::from_stored(serde_json::json!({ "stt_provider": "whisper", "ai_refine": "yes" }));
    // Wrong type for one field makes serde reject the object; we fall back to defaults.
    assert_eq!(prefs, BehaviorPrefs::default());
    let prefs = BehaviorPrefs::from_stored(serde_json::json!({ "stt_provider": "whisper" }));
    assert_eq!(prefs.stt_provider, "deepgram");
    let prefs = BehaviorPrefs::from_stored(serde_json::json!("garbage"));
    assert_eq!(prefs, BehaviorPrefs::default());
  }
}
