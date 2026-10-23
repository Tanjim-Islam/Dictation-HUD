//! Local speech model commands: list, download (with progress events), cancel, remove, link.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio_util::sync::CancellationToken;

use super::catalog::{self, CATALOG};
use super::client::LoadSpec;
use super::download::{self, Hosts};
use super::link;
use super::store::ModelState;
use crate::services::Services;
use crate::settings::{self, SettingsPatch};

pub const DOWNLOAD_EVENT: &str = "local://download";

#[derive(Serialize)]
pub struct LocalSttModelView {
  pub id: &'static str,
  pub name: &'static str,
  pub size_bytes: u64,
  pub languages: Vec<&'static str>,
  pub state: &'static str,
  pub pct: Option<f32>,
  pub path: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct SttDownloadEvent {
  pub id: String,
  pub pct: f32,
  pub state: &'static str,
  pub error: Option<String>,
}

#[derive(Serialize)]
pub struct LinkedView {
  pub id: &'static str,
  pub name: &'static str,
}

/// Running downloads: cancel token and last progress.
#[derive(Default)]
pub struct LocalDownloads(Mutex<HashMap<String, (CancellationToken, f32)>>);

fn state_name(s: &ModelState) -> &'static str {
  match s {
    ModelState::Installed(_) => "installed",
    ModelState::Linked(_) | ModelState::Cached(_) => "linked",
    ModelState::Missing(_) => "missing",
    ModelState::NotInstalled => "not_installed",
  }
}

/// Emits only when progress moved by 1% or more: at most about 100 events per download.
#[derive(Default)]
struct Throttle {
  last: Option<f32>,
}

impl Throttle {
  fn ready(&mut self, pct: f32) -> bool {
    let go = self.last.is_none_or(|p| pct - p >= 0.01);
    if go {
      self.last = Some(pct);
    }
    go
  }
}

#[tauri::command]
pub async fn local_stt_models(
  app: AppHandle,
  downloads: State<'_, LocalDownloads>,
) -> Result<Vec<LocalSttModelView>, String> {
  let services = app.state::<Services>();
  let s = settings::load(&app);
  let running = downloads.0.lock().unwrap();
  Ok(
    CATALOG
      .iter()
      .map(|m| {
        let state = services
          .models
          .resolve(m, s.local_model_paths.get(m.id).map(std::path::Path::new));
        let path = match &state {
          ModelState::Linked(d) | ModelState::Cached(d) | ModelState::Missing(d) => {
            Some(d.to_string_lossy().into_owned())
          }
          _ => None,
        };
        let (state, pct) = match running.get(m.id) {
          Some((_, pct)) => ("downloading", Some(*pct)),
          None => (state_name(&state), None),
        };
        LocalSttModelView {
          id: m.id,
          name: m.name,
          size_bytes: m.download_bytes(),
          languages: m.languages.to_vec(),
          state,
          pct,
          path,
        }
      })
      .collect(),
  )
}

#[tauri::command]
pub async fn local_stt_download(
  app: AppHandle,
  downloads: State<'_, LocalDownloads>,
  id: String,
) -> Result<(), String> {
  let m = catalog::get(&id).ok_or("unknown model")?;
  let cancel = CancellationToken::new();
  {
    let mut running = downloads.0.lock().unwrap();
    if running.contains_key(&id) {
      return Ok(());
    }
    running.insert(id.clone(), (cancel.clone(), 0.0));
  }
  let models = app.state::<Services>().models.clone();
  let handle = app.clone();
  tauri::async_runtime::spawn(async move {
    let emit = |pct: f32, state: &'static str, error: Option<String>| {
      let _ = handle.emit(
        DOWNLOAD_EVENT,
        SttDownloadEvent {
          id: m.id.into(),
          pct,
          state,
          error,
        },
      );
    };
    emit(0.0, "downloading", None);
    let mut throttle = Throttle::default();
    let started = Instant::now();
    let result = download::download_model(m, &models, &Hosts::default(), cancel, |p| {
      let pct = if p.total == 0 {
        0.0
      } else {
        p.done as f32 / p.total as f32
      };
      if let Some(entry) = handle.state::<LocalDownloads>().0.lock().unwrap().get_mut(m.id) {
        entry.1 = pct;
      }
      if throttle.ready(pct) {
        emit(pct, "downloading", None);
      }
    })
    .await;
    handle.state::<LocalDownloads>().0.lock().unwrap().remove(m.id);
    match &result {
      Ok(()) => emit(1.0, "done", None),
      Err(download::DownloadError::Cancelled) => emit(0.0, "cancelled", None),
      Err(e) => emit(0.0, "failed", Some(e.message())),
    }
    tracing::info!(
      model = m.id,
      secs = started.elapsed().as_secs(),
      ok = result.is_ok(),
      "local model download finished"
    );
  });
  Ok(())
}

#[tauri::command]
pub async fn local_stt_cancel(downloads: State<'_, LocalDownloads>, id: String) -> Result<(), String> {
  if let Some((c, _)) = downloads.0.lock().unwrap().get(&id) {
    c.cancel();
  }
  Ok(())
}

#[tauri::command]
pub async fn local_stt_remove(app: AppHandle, id: String) -> Result<(), String> {
  let m = catalog::get(&id).ok_or("unknown model")?;
  let s = settings::load(&app);
  if s.local_model_paths.contains_key(m.id) {
    // Unlink only: the user's own folder is never deleted.
    let mut paths = s.local_model_paths.clone();
    paths.remove(m.id);
    crate::commands::apply_settings_patch(
      &app,
      SettingsPatch {
        local_model_paths: Some(paths),
        ..Default::default()
      },
    )?;
    return Ok(());
  }
  let services = app.state::<Services>();
  match services.models.resolve(m, None) {
    ModelState::Installed(_) => services
      .models
      .remove_install(m.id)
      .map_err(|_| "Could not remove the model files".to_string()),
    ModelState::Cached(_) => Err("This model is in your Hugging Face cache, so it stays there.".into()),
    _ => Ok(()),
  }
}

#[tauri::command]
pub async fn local_stt_link(app: AppHandle) -> Result<Option<LinkedView>, String> {
  let picker = app.clone();
  let picked = tokio::task::spawn_blocking(move || {
    picker
      .dialog()
      .file()
      .set_title("Choose the model folder")
      .blocking_pick_folder()
  })
  .await
  .map_err(|e| e.to_string())?;
  let Some(folder) = picked.and_then(|p| p.into_path().ok()) else {
    return Ok(None);
  };
  let (m, dir) = tokio::task::spawn_blocking(move || link::identify(&folder, CATALOG))
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.message())?;
  // Prove the files load before saving the path.
  let vad = app
    .path()
    .resource_dir()
    .map_err(|e| e.to_string())?
    .join("silero_vad.onnx");
  let spec = LoadSpec {
    kind: m.kind,
    files: m.files_in(&dir),
    vad,
    language: String::new(),
  };
  let helper = app.state::<Services>().helper.clone();
  helper
    .session(&spec)
    .await
    .map_err(|_| "This model's files could not be loaded.".to_string())?;
  let mut paths = settings::load(&app).local_model_paths;
  paths.insert(m.id.to_string(), dir.to_string_lossy().into_owned());
  crate::commands::apply_settings_patch(
    &app,
    SettingsPatch {
      local_model_paths: Some(paths),
      ..Default::default()
    },
  )?;
  tracing::info!(model = m.id, "linked a local model folder");
  Ok(Some(LinkedView { id: m.id, name: m.name }))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn states_map_to_ui_names() {
    let p = std::path::PathBuf::from("x");
    assert_eq!(state_name(&ModelState::Installed(p.clone())), "installed");
    assert_eq!(state_name(&ModelState::Linked(p.clone())), "linked");
    assert_eq!(state_name(&ModelState::Cached(p.clone())), "linked");
    assert_eq!(state_name(&ModelState::Missing(p)), "missing");
    assert_eq!(state_name(&ModelState::NotInstalled), "not_installed");
  }

  #[test]
  fn progress_events_are_throttled() {
    let mut t = Throttle::default();
    assert!(t.ready(0.0));
    assert!(!t.ready(0.001));
    assert!(t.ready(0.02));
  }
}
