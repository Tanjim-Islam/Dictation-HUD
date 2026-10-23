//! Settings, provider, credential, and audio-device commands.
//!
//! No command returns a secret. A newly typed key crosses from the WebView exactly once (in
//! `set_provider_key` / `elevenlabs_pool_add`), is stored in the OS credential store, and is
//! never sent back.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt as _;

use tokio_util::sync::CancellationToken;

use crate::audio::{self, DeviceInfo};
use crate::credentials::names;
use crate::local_llm::{self, detect, scan, server, LocalApp, LocalModel, Pick};
use crate::providers::elevenlabs_pool::{ImportReport, KeyStatus, PoolView};
use crate::providers::ProviderDescriptor;
use crate::refine;
use crate::services::{dev_env, KeySource, Services};
use crate::settings::{self, Settings, SettingsPatch};

pub const SETTINGS_EVENT: &str = "settings://changed";

fn http() -> Result<reqwest::Client, String> {
  reqwest::Client::builder()
    .timeout(Duration::from_secs(8))
    .build()
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_settings(app: AppHandle) -> Result<Settings, String> {
  let mut s = settings::load(&app);
  if let Ok(os) = app.autolaunch().is_enabled() {
    s.autostart = os;
  }
  Ok(s)
}

#[tauri::command]
pub async fn update_settings(app: AppHandle, patch: SettingsPatch) -> Result<Settings, String> {
  apply_settings_patch(&app, patch)
}

/// The one path for changing settings (commands and tray): save, broadcast, run side effects.
pub fn apply_settings_patch(app: &AppHandle, patch: SettingsPatch) -> Result<Settings, String> {
  let mut s = settings::load(app);
  let fields = patch.fields().join(",");
  patch.apply(&mut s);
  settings::save(app, &s)?;
  let _ = app.emit(SETTINGS_EVENT, &s);
  if fields.contains("audio_retention_days") {
    crate::history::sweep_soon(app);
  }
  // Microphone and language checks live in the tray menu.
  crate::tray::refresh(app);
  tracing::info!(changed = %fields, stt = %s.stt_provider, model = %s.stt_model(), language = %s.language, ai_refine = s.ai_refine, "settings updated");
  Ok(s)
}

#[derive(Serialize)]
pub struct SttProviderView {
  #[serde(flatten)]
  pub descriptor: ProviderDescriptor,
  pub configured: bool,
  pub key_source: KeySource,
}

#[derive(Serialize)]
pub struct RefineProviderView {
  pub id: &'static str,
  pub name: &'static str,
  pub configured: bool,
  pub key_source: KeySource,
  /// Runs on this computer (Ollama, LM Studio); no key.
  pub local: bool,
  /// Always true for cloud providers.
  pub installed: bool,
}

#[derive(Serialize)]
pub struct ProvidersOverview {
  pub stt: Vec<SttProviderView>,
  pub refine: Vec<RefineProviderView>,
}

#[tauri::command]
pub async fn providers_overview(services: State<'_, Services>) -> Result<ProvidersOverview, String> {
  let stt = services
    .registry
    .ids()
    .into_iter()
    .filter_map(|id| services.registry.get(&id))
    .map(|a| {
      let descriptor = a.descriptor();
      let key_source = match descriptor.id.as_str() {
        "deepgram" => services.key_source(names::DEEPGRAM, "DEEPGRAM_API_KEY"),
        "elevenlabs" => {
          let v = services.pool.view();
          if !v.keys.is_empty() {
            KeySource::Saved
          } else if v.using_dev_key {
            KeySource::DevEnv
          } else {
            KeySource::None
          }
        }
        _ => KeySource::None,
      };
      SttProviderView {
        configured: a.configured(),
        key_source,
        descriptor,
      }
    })
    .collect();
  let refine = refine::PROVIDERS
    .iter()
    .map(|p| match p.local {
      Some(app) => {
        let installed = detect::installed(app, &detect::SystemProbe);
        RefineProviderView {
          id: p.id,
          name: p.name,
          configured: installed,
          key_source: KeySource::None,
          local: true,
          installed,
        }
      }
      None => {
        let key_source = services.key_source(p.secret, p.env);
        RefineProviderView {
          id: p.id,
          name: p.name,
          configured: key_source != KeySource::None,
          key_source,
          local: false,
          installed: true,
        }
      }
    })
    .collect();
  Ok(ProvidersOverview { stt, refine })
}

fn secret_name(provider: &str) -> Result<&'static str, String> {
  match provider {
    "deepgram" => Ok(names::DEEPGRAM),
    "openrouter" => Ok(names::OPENROUTER),
    "megallm" => Ok(names::MEGALLM),
    _ => Err(format!("unknown provider {provider}")),
  }
}

/// Stores a key in the OS credential store. The value is never returned or logged.
#[tauri::command]
pub async fn set_provider_key(services: State<'_, Services>, provider: String, secret: String) -> Result<(), String> {
  let secret = secret.trim();
  if secret.is_empty() {
    return Err("Paste an API key first".into());
  }
  if provider == "elevenlabs" {
    // Single-key mode: the pasted key replaces the pool after it verifies.
    let previous: Vec<String> = services.pool.view().keys.iter().map(|k| k.id.clone()).collect();
    services.pool.add(secret).await.map_err(|e| e.to_string())?;
    for id in previous {
      services.pool.remove(&id)?;
    }
    return Ok(());
  }
  services
    .secrets
    .set(secret_name(&provider)?, secret)
    .map_err(|e| e.to_string())?;
  tracing::info!(provider = %provider, "key saved to credential store");
  Ok(())
}

#[tauri::command]
pub async fn clear_provider_key(services: State<'_, Services>, provider: String) -> Result<(), String> {
  if provider == "elevenlabs" {
    return services.pool.clear();
  }
  services
    .secrets
    .delete(secret_name(&provider)?)
    .map_err(|e| e.to_string())?;
  tracing::info!(provider = %provider, "key removed from credential store");
  Ok(())
}

#[derive(Serialize)]
pub struct TestResult {
  pub ok: bool,
  pub message: String,
}

fn result(ok: bool, message: impl Into<String>) -> TestResult {
  TestResult {
    ok,
    message: message.into(),
  }
}

fn status_message(provider: &str, code: u16) -> TestResult {
  match code {
    200..=299 => result(true, format!("{provider} key works")),
    401 | 403 => result(false, format!("{provider} rejected the key")),
    402 => result(false, format!("{provider} account has no credits")),
    429 => result(
      false,
      format!("{provider} is rate limiting requests; try again shortly"),
    ),
    c => result(false, format!("{provider} returned HTTP {c}")),
  }
}

/// Checks a saved key through the same backend paths dictation uses. Never spends audio credits.
#[tauri::command]
pub async fn test_provider(services: State<'_, Services>, provider: String) -> Result<TestResult, String> {
  let client = http()?;
  let (name, request) = match provider.as_str() {
    "deepgram" => {
      let Some(key) = services
        .secrets
        .get(names::DEEPGRAM)
        .or_else(|| dev_env("DEEPGRAM_API_KEY"))
      else {
        return Ok(result(false, "Add a Deepgram key first"));
      };
      (
        "Deepgram",
        client
          .get("https://api.deepgram.com/v1/auth/token")
          .header("authorization", format!("Token {key}")),
      )
    }
    "elevenlabs" => {
      if services.pool.view().keys.is_empty() {
        let ok = services.pool.configured();
        return Ok(result(
          ok,
          if ok {
            "Using the development key from .env"
          } else {
            "Add an ElevenLabs key first"
          },
        ));
      }
      services.pool.refresh_all().await;
      let v = services.pool.view();
      let working = v
        .keys
        .iter()
        .filter(|k| !matches!(k.status, KeyStatus::Invalid | KeyStatus::Error))
        .count();
      return Ok(result(
        working > 0,
        format!("{working} of {} ElevenLabs keys work", v.keys.len()),
      ));
    }
    "openrouter" | "megallm" => {
      let p = refine::provider(&provider);
      let Some(key) = services.refine_key(p) else {
        return Ok(result(false, format!("Add a {} key first", p.name)));
      };
      let url = if provider == "openrouter" {
        "https://openrouter.ai/api/v1/key"
      } else {
        p.models_url
      };
      (p.name, client.get(url).bearer_auth(key))
    }
    other => return Err(format!("unknown provider {other}")),
  };
  Ok(match request.send().await {
    Ok(resp) => status_message(name, resp.status().as_u16()),
    Err(_) => result(false, format!("Could not reach {name}")),
  })
}

/// Model ids for a refinement provider (public list for OpenRouter, key required for MegaLLM).
#[tauri::command]
pub async fn list_refine_models(services: State<'_, Services>, provider: String) -> Result<Vec<String>, String> {
  let p = refine::provider(&provider);
  if let Some(app) = p.local {
    return Ok(status_for(app, None).await.models.into_iter().map(|m| m.id).collect());
  }
  let mut req = http()?.get(p.models_url);
  if let Some(key) = services.refine_key(p) {
    req = req.bearer_auth(key);
  }
  let resp = req.send().await.map_err(|_| format!("Could not reach {}", p.name))?;
  if !resp.status().is_success() {
    return Err(format!("{} returned HTTP {}", p.name, resp.status().as_u16()));
  }
  let v: serde_json::Value = resp.json().await.map_err(|_| "Unexpected response".to_string())?;
  let mut ids: Vec<String> = v["data"]
    .as_array()
    .map(|a| a.iter().filter_map(|m| m["id"].as_str().map(str::to_string)).collect())
    .unwrap_or_default();
  ids.sort();
  Ok(ids)
}

#[tauri::command]
pub async fn elevenlabs_pool_view(services: State<'_, Services>) -> Result<PoolView, String> {
  Ok(services.pool.view())
}

#[tauri::command]
pub async fn elevenlabs_pool_add(services: State<'_, Services>, secret: String) -> Result<PoolView, String> {
  services.pool.add_for_mode(&secret).await.map_err(|e| e.to_string())?;
  Ok(services.pool.view())
}

#[derive(Serialize)]
pub struct PoolImport {
  pub report: ImportReport,
  pub view: PoolView,
}

/// Adds ElevenLabs keys from pasted text (for example a block of `NAME=key` lines). Keys cross
/// to Rust once and are never returned; the result holds counts only.
#[tauri::command]
pub async fn elevenlabs_pool_import(services: State<'_, Services>, text: String) -> Result<PoolImport, String> {
  let report = services.pool.import_text(&text).await;
  if report.found == 0 {
    return Err("No ElevenLabs API keys found in the pasted text".into());
  }
  Ok(PoolImport {
    report,
    view: services.pool.view(),
  })
}

#[tauri::command]
pub async fn elevenlabs_pool_remove(services: State<'_, Services>, id: String) -> Result<PoolView, String> {
  services.pool.remove(&id)?;
  Ok(services.pool.view())
}

#[tauri::command]
pub async fn elevenlabs_pool_clear(services: State<'_, Services>) -> Result<PoolView, String> {
  services.pool.clear()?;
  Ok(services.pool.view())
}

#[tauri::command]
pub async fn elevenlabs_pool_set_multi(services: State<'_, Services>, multi: bool) -> Result<PoolView, String> {
  services.pool.set_multi(multi);
  Ok(services.pool.view())
}

#[tauri::command]
pub async fn elevenlabs_pool_refresh(services: State<'_, Services>) -> Result<PoolView, String> {
  services.pool.refresh_all().await;
  Ok(services.pool.view())
}

/// Live credit polling while the provider settings are visible.
#[tauri::command]
pub fn elevenlabs_pool_watch(services: State<'_, Services>, active: bool) {
  services.set_pool_watch(active);
}

#[tauri::command]
pub async fn audio_devices() -> Result<Vec<DeviceInfo>, String> {
  tauri::async_runtime::spawn_blocking(audio::list_input_devices)
    .await
    .map_err(|e| e.to_string())
}

pub const LLM_PULL_EVENT: &str = "local://llm-pull";

#[derive(Serialize)]
pub struct LocalLlmStatus {
  pub installed: bool,
  pub state: &'static str,
  pub models: Vec<LocalModel>,
  pub picks: Vec<Pick>,
}

#[derive(Serialize, Clone)]
pub struct LlmPullEvent {
  pub provider: String,
  pub model: String,
  pub pct: f32,
  pub state: &'static str,
  pub error: Option<String>,
}

/// One pull at a time per local app.
#[derive(Default)]
pub struct LocalPulls(std::sync::Mutex<std::collections::HashMap<LocalApp, CancellationToken>>);

fn local_app(provider: &str) -> Result<LocalApp, String> {
  LocalApp::from_id(provider).ok_or_else(|| format!("unknown local provider {provider}"))
}

fn start_state(e: server::StartError) -> &'static str {
  match e {
    server::StartError::NotLocal => "not_local",
    server::StartError::NotInstalled => "not_installed",
    _ => "failed",
  }
}

fn disk_models(app: LocalApp) -> Vec<LocalModel> {
  scan::models_dir(app, &detect::SystemProbe)
    .map(|d| scan::scan(app, &d))
    .unwrap_or_default()
}

async fn status_for(app: LocalApp, start_error: Option<server::StartError>) -> LocalLlmStatus {
  let p = detect::SystemProbe;
  let picks = local_llm::picks(app).to_vec();
  if !detect::installed(app, &p) {
    return LocalLlmStatus {
      installed: false,
      state: "not_installed",
      models: Vec::new(),
      picks,
    };
  }
  let base = match detect::endpoint(app, &p) {
    Ok(e) => e.base,
    Err(_) => {
      return LocalLlmStatus {
        installed: true,
        state: "not_local",
        models: disk_models(app),
        picks,
      }
    }
  };
  if server::is_up(app, &base).await {
    let models = server::list_models(app, &base)
      .await
      .unwrap_or_else(|_| disk_models(app));
    return LocalLlmStatus {
      installed: true,
      state: "running",
      models,
      picks,
    };
  }
  LocalLlmStatus {
    installed: true,
    state: start_error.map(start_state).unwrap_or("stopped"),
    models: disk_models(app),
    picks,
  }
}

#[tauri::command]
pub async fn local_llm_status(provider: String) -> Result<LocalLlmStatus, String> {
  Ok(status_for(local_app(&provider)?, None).await)
}

/// Starts the server hidden (the UI calls this right after a `stopped` status).
#[tauri::command]
pub async fn local_llm_start(provider: String) -> Result<LocalLlmStatus, String> {
  let app = local_app(&provider)?;
  let err = server::ensure_running(app, &detect::SystemProbe).await.err();
  Ok(status_for(app, err).await)
}

#[tauri::command]
pub async fn local_llm_pull(
  app: AppHandle,
  pulls: State<'_, LocalPulls>,
  provider: String,
  model: String,
) -> Result<(), String> {
  let local = local_app(&provider)?;
  let cancel = CancellationToken::new();
  if let Some(old) = pulls.0.lock().unwrap().insert(local, cancel.clone()) {
    old.cancel();
  }
  let emit = |pct: f32, state: &'static str, error: Option<String>| {
    let _ = app.emit(
      LLM_PULL_EVENT,
      LlmPullEvent {
        provider: provider.clone(),
        model: model.clone(),
        pct,
        state,
        error,
      },
    );
  };
  emit(0.0, "downloading", None);
  let result = match server::ensure_running(local, &detect::SystemProbe).await {
    Ok(base) => server::pull(local, &base, &model, cancel, |u| emit(u.pct, "downloading", None)).await,
    Err(_) => Err(format!("{} did not start", local.name())),
  };
  pulls.0.lock().unwrap().remove(&local);
  match &result {
    Ok(()) => emit(1.0, "done", None),
    Err(e) => emit(0.0, "failed", Some(e.clone())),
  }
  tracing::info!(app = local.id(), ok = result.is_ok(), "local model download finished");
  result
}

#[tauri::command]
pub async fn local_llm_cancel_pull(pulls: State<'_, LocalPulls>, provider: String) -> Result<(), String> {
  if let Some(c) = pulls.0.lock().unwrap().remove(&local_app(&provider)?) {
    c.cancel();
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn status_states_map_start_errors() {
    use crate::local_llm::server::StartError;
    assert_eq!(start_state(StartError::NotLocal), "not_local");
    assert_eq!(start_state(StartError::NotInstalled), "not_installed");
    assert_eq!(start_state(StartError::Timeout), "failed");
    assert_eq!(start_state(StartError::NoCli), "failed");
  }
}
