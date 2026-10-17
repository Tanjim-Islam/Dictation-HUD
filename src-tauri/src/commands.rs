//! Settings, provider, credential, and audio-device commands.
//!
//! No command returns a secret. A newly typed key crosses from the WebView exactly once (in
//! `set_provider_key` / `elevenlabs_pool_add`), is stored in the OS credential store, and is
//! never sent back.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt as _;

use crate::audio::{self, DeviceInfo};
use crate::credentials::names;
use crate::providers::elevenlabs_pool::{KeyStatus, PoolView};
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
  let mut s = settings::load(&app);
  patch.apply(&mut s);
  settings::save(&app, &s)?;
  let _ = app.emit(SETTINGS_EVENT, &s);
  tracing::info!(stt = %s.stt_provider, model = %s.stt_model(), language = %s.language, ai_refine = s.ai_refine, "settings updated");
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
    .map(|p| {
      let key_source = services.key_source(p.secret, p.env);
      RefineProviderView {
        id: p.id,
        name: p.name,
        configured: key_source != KeySource::None,
        key_source,
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
  services.pool.add(&secret).await.map_err(|e| e.to_string())?;
  Ok(services.pool.view())
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
