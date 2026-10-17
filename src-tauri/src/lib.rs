pub mod app_host;
pub mod audio;
pub mod commands;
pub mod credentials;
pub mod db;
pub mod delivery;
pub mod dictionary;
pub mod engine;
pub mod flowbar;
pub mod history;
pub mod hotkey;
pub mod logging;
pub mod prompt;
pub mod providers;
pub mod refine;
pub mod services;
pub mod session;
pub mod settings;
pub mod target;
pub mod textproc;
pub mod transcript;

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tauri::{
  menu::{Menu, MenuItem},
  tray::TrayIconBuilder,
  AppHandle, Emitter, Manager, State, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt as _;

use session::{Input, SessionHandle, Snapshot, Trigger};

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

/// Frontend diagnostics. Messages must be content-free; they are length-capped here.
#[tauri::command]
fn log_to_terminal(message: String) {
  let capped: String = message.chars().take(300).collect();
  tracing::info!(target: "frontend", "{capped}");
}

fn show_main_window(app: &AppHandle) {
  if let Some(w) = app.get_webview_window("main") {
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
      Some(vec!["--hidden"]),
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
      let data_dir = app.path().app_local_data_dir().expect("app data dir");
      let db = db::Db::open(&data_dir.join("history").join("dictation.db")).map_err(|e| {
        tracing::error!(error = %e, "could not open history database");
        e
      })?;
      match db.reconcile_interrupted() {
        Ok(0) => {}
        Ok(n) => tracing::warn!(sessions = n, "reconciled sessions left in progress by a previous run"),
        Err(e) => tracing::warn!(error = %e, "reconcile failed"),
      }
      let recordings = audio::store::RecordingsDir::new(data_dir.join("recordings"));
      app.manage(recordings.clone());
      app.manage(db);
      // Leftover raw recordings from a crash are encoded and attached in the background.
      {
        let handle = app.handle().clone();
        let dir = recordings.clone();
        tauri::async_runtime::spawn_blocking(move || {
          let db = handle.state::<db::Db>();
          let n = dir.recover(&db);
          if n > 0 {
            tracing::warn!(recordings = n, "recovered recordings left by a previous run");
            let _ = handle.emit(app_host::HISTORY_EVENT, ());
          }
        });
      }
      app.manage(services::Services::init(app.handle()));
      app.manage(app_host::CopyReasons::default());
      app.manage(history::Busy::default());
      history::start_retention(app.handle());
      target::start_foreground_tracker();
      let session = session::actor::spawn(Arc::new(app_host::TauriHost::new(app.handle().clone())));
      app.manage(session);
      // Launched at login: stay in the tray. Otherwise open the main window.
      if std::env::args().any(|a| a == "--hidden") {
        tracing::info!("started hidden (login launch)");
      } else {
        show_main_window(app.handle());
      }
      flowbar::hide(app.handle());
      build_tray(app)?;
      hotkey::register_stored(app.handle());
      Ok(())
    })
    .on_window_event(|window, event| {
      // Closing the main window hides it; only tray Exit quits.
      if let WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == "main" {
          api.prevent_close();
          let _ = window.hide();
        }
      }
    })
    .invoke_handler(tauri::generate_handler![
      session_intent,
      session_snapshot,
      set_hotkey,
      get_hotkey,
      set_autostart,
      get_autostart,
      log_to_terminal,
      commands::get_settings,
      commands::update_settings,
      commands::providers_overview,
      commands::set_provider_key,
      commands::clear_provider_key,
      commands::test_provider,
      commands::list_refine_models,
      commands::elevenlabs_pool_view,
      commands::elevenlabs_pool_add,
      commands::elevenlabs_pool_remove,
      commands::elevenlabs_pool_clear,
      commands::elevenlabs_pool_set_multi,
      commands::elevenlabs_pool_refresh,
      commands::elevenlabs_pool_watch,
      commands::audio_devices,
      history::history_list,
      history::history_detail,
      history::history_audio,
      history::history_copy,
      history::history_retry,
      history::history_recover,
      history::history_retry_refinement,
      history::history_export_wav,
      history::history_show_export,
      history::history_delete,
      history::history_storage,
      dictionary::dictionary_list,
      dictionary::dictionary_add,
      dictionary::dictionary_update,
      dictionary::dictionary_delete,
      dictionary::dictionary_accept,
      dictionary::dictionary_import,
      dictionary::snippets_list,
      dictionary::snippets_add,
      dictionary::snippets_update,
      dictionary::snippets_delete,
    ])
    .run(context)
}
