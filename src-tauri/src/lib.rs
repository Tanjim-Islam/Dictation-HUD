pub mod app_host;
pub mod commands;
pub mod config;
pub mod flowbar;
pub mod hotkey;
pub mod logging;
pub mod paste;
pub mod prompt;
pub mod refine;
pub mod session;
pub mod settings;
pub mod symbols;
pub mod transcript;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use tauri::{
  menu::{Menu, MenuItem},
  tray::TrayIconBuilder,
  AppHandle, Manager, State, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt as _;

use session::machine::{EngineError, EngineEvent, SessionId};
use session::{Input, SessionHandle, Snapshot, Trigger};
use settings::{BehaviorPatch, BehaviorPrefs};

/// Most recent completed transcript, for "Paste last transcript". Moves to SQLite in Phase 3.
#[derive(Default)]
pub struct LastTranscript(pub Mutex<Option<String>>);

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Intent {
  Toggle,
  Start,
  Finish,
  Cancel,
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum IntentSource {
  FlowBar,
  #[default]
  App,
}

/// The only way a WebView can change session state.
#[tauri::command]
async fn session_intent(
  session: State<'_, SessionHandle>,
  intent: Intent,
  source: Option<IntentSource>,
) -> Result<(), String> {
  let trigger = match source.unwrap_or_default() {
    IntentSource::FlowBar => Trigger::FlowBar,
    IntentSource::App => Trigger::App,
  };
  let input = match intent {
    Intent::Toggle => Input::Toggle(trigger),
    Intent::Start => Input::Start(trigger),
    Intent::Finish => Input::Finish(trigger),
    Intent::Cancel => Input::Cancel(trigger),
  };
  session.send(input).await;
  Ok(())
}

#[tauri::command]
async fn session_snapshot(session: State<'_, SessionHandle>) -> Result<Snapshot, String> {
  session
    .snapshot()
    .await
    .ok_or_else(|| "session manager unavailable".into())
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum EngineEventDto {
  Ready,
  Interim { text: String },
  Final { text: String },
  Finalized,
  Failed { error: String },
}

/// Phase 1 bridge for the WebView-hosted engine. Events carry the session id; the machine
/// drops any that do not match the active session.
#[tauri::command]
async fn engine_event(
  session: State<'_, SessionHandle>,
  session_id: String,
  event: EngineEventDto,
) -> Result<(), String> {
  let id: SessionId = session_id.parse().map_err(|_| "bad session id".to_string())?;
  let ev = match event {
    EngineEventDto::Ready => EngineEvent::Ready,
    EngineEventDto::Interim { text } => EngineEvent::Interim(text),
    EngineEventDto::Final { text } => EngineEvent::Final(text),
    EngineEventDto::Finalized => EngineEvent::Finalized,
    EngineEventDto::Failed { error } => EngineEvent::Failed(match error.as_str() {
      "missing_key" => EngineError::MissingKey,
      "auth" => EngineError::Auth,
      "microphone" => EngineError::Microphone,
      "network" => EngineError::Network,
      "lost" => EngineError::Lost,
      other => EngineError::Provider(other.chars().take(40).collect()),
    }),
  };
  session.send(Input::Engine(id, ev)).await;
  Ok(())
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
  let mut prefs = settings::load_behavior(&app);
  prefs.autostart = enabled;
  settings::save_behavior(&app, &prefs)?;
  tracing::info!(enabled, "autostart updated");
  Ok(())
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> Result<bool, String> {
  app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_behavior(app: AppHandle, patch: BehaviorPatch) -> Result<BehaviorPrefs, String> {
  let mut prefs = settings::load_behavior(&app);
  patch.apply(&mut prefs);
  settings::save_behavior(&app, &prefs)?;
  tracing::info!(?prefs, "behavior saved");
  Ok(prefs)
}

#[tauri::command]
async fn get_behavior(app: AppHandle) -> Result<BehaviorPrefs, String> {
  let mut prefs = settings::load_behavior(&app);
  if let Ok(os_enabled) = app.autolaunch().is_enabled() {
    prefs.autostart = os_enabled;
  }
  Ok(prefs)
}

/// Frontend diagnostics. Messages must be content-free; they are length-capped here.
#[tauri::command]
fn log_to_terminal(message: String) {
  let capped: String = message.chars().take(300).collect();
  tracing::info!(target: "frontend", "{capped}");
}

fn show_main_window(app: &AppHandle) {
  if let Some(w) = app.get_webview_window("settings") {
    let _ = w.unminimize();
    let _ = w.show();
    let _ = w.set_focus();
  }
}

/// Stops any active session safely, then exits.
fn graceful_exit(app: &AppHandle) {
  let app = app.clone();
  tauri::async_runtime::spawn(async move {
    if let Some(session) = app.try_state::<SessionHandle>() {
      session.shutdown(Duration::from_secs(5)).await;
    }
    tracing::info!("exiting");
    app.exit(0);
  });
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
  let menu = Menu::new(app)?;
  menu.append(&MenuItem::with_id(
    app,
    "open",
    "Open Dictation HUD",
    true,
    None::<&str>,
  )?)?;
  menu.append(&MenuItem::with_id(app, "start", "Start Dictation", true, None::<&str>)?)?;
  menu.append(&MenuItem::with_id(app, "stop", "Finish Dictation", true, None::<&str>)?)?;
  menu.append(&MenuItem::with_id(app, "quit", "Exit", true, None::<&str>)?)?;
  TrayIconBuilder::with_id("main")
    .tooltip("Dictation HUD")
    .icon(app.default_window_icon().unwrap().clone())
    .menu(&menu)
    .on_menu_event(|app, event| {
      let session = app.state::<SessionHandle>();
      match event.id.as_ref() {
        "open" => show_main_window(app),
        "start" => session.try_send(Input::Start(Trigger::Tray)),
        "stop" => session.try_send(Input::Finish(Trigger::Tray)),
        "quit" => graceful_exit(app),
        _ => {}
      }
    })
    .build(app)?;
  Ok(())
}

pub fn run(context: tauri::Context<tauri::Wry>) -> tauri::Result<()> {
  tauri::Builder::default()
    .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
      show_main_window(app)
    }))
    .plugin(tauri_plugin_store::Builder::default().build())
    .plugin(tauri_plugin_autostart::init(
      tauri_plugin_autostart::MacosLauncher::LaunchAgent,
      None,
    ))
    .plugin(hotkey::plugin())
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
      app.manage(LastTranscript::default());
      let session = session::actor::spawn(Arc::new(app_host::TauriHost::new(app.handle().clone())));
      app.manage(session);
      if let Some(s) = app.get_webview_window("settings") {
        let _ = s.hide();
      }
      flowbar::hide(app.handle());
      build_tray(app)?;
      hotkey::register_stored(app.handle());
      Ok(())
    })
    .on_window_event(|window, event| {
      // Closing the main window hides it; only tray Exit quits.
      if let WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == "settings" {
          api.prevent_close();
          let _ = window.hide();
        }
      }
    })
    .invoke_handler(tauri::generate_handler![
      session_intent,
      session_snapshot,
      engine_event,
      set_hotkey,
      get_hotkey,
      set_autostart,
      get_autostart,
      set_behavior,
      get_behavior,
      log_to_terminal,
      commands::save_keys_secure,
      commands::get_keys_secure,
      commands::set_model,
      commands::get_model,
      commands::set_megallm_model,
      commands::get_megallm_model,
      commands::set_language,
      commands::get_language,
      commands::test_openrouter,
      commands::test_deepgram,
      commands::test_megallm,
      commands::test_elevenlabs,
      commands::list_megallm_models,
      commands::create_elevenlabs_token,
      commands::runtime_keys,
    ])
    .run(context)
}
