//! Tauri commands for keys, models, and provider checks.
//! Provider networking moves behind adapters in Phase 2.

use std::time::Duration;

use tauri::AppHandle;

use crate::{config, refine};

#[tauri::command]
pub async fn save_keys_secure(
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
pub async fn get_keys_secure(app: AppHandle) -> Result<(bool, bool, bool, bool), String> {
  Ok((
    config::get_openrouter_key(&app).await.is_some(),
    config::get_deepgram_key(&app).await.is_some(),
    config::get_megallm_key(&app).await.is_some(),
    config::get_elevenlabs_key(&app).await.is_some(),
  ))
}

#[tauri::command]
pub async fn set_model(app: AppHandle, name: String) -> Result<(), String> {
  config::set_model(&app, &name).await.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn get_model(app: AppHandle) -> Result<String, String> {
  Ok(
    config::get_model(&app)
      .await
      .unwrap_or_else(|| "openai/gpt-oss-20b:free".into()),
  )
}
#[tauri::command]
pub async fn set_megallm_model(app: AppHandle, name: String) -> Result<(), String> {
  config::set_megallm_model(&app, &name).await.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn get_megallm_model(app: AppHandle) -> Result<String, String> {
  Ok(config::get_megallm_model(&app).await.unwrap_or_else(|| "gpt-4".into()))
}
#[tauri::command]
pub async fn set_language(app: AppHandle, code: String) -> Result<(), String> {
  config::set_language(&app, &code).await.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn get_language(app: AppHandle) -> Result<String, String> {
  Ok(config::get_language(&app).await.unwrap_or_else(|| "en-US".into()))
}

#[tauri::command]
pub async fn test_openrouter(app: AppHandle) -> Result<(), String> {
  refine::refine_with_provider(&app, "openrouter", "this is a connection test")
    .await
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// Cheap Deepgram key validation (no audio, no credits).
#[tauri::command]
pub async fn test_deepgram(app: AppHandle) -> Result<(), String> {
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
pub async fn test_megallm(app: AppHandle) -> Result<(), String> {
  list_megallm_models(app).await.map(|_| ())
}

fn http_client() -> Result<reqwest::Client, String> {
  reqwest::Client::builder()
    .timeout(Duration::from_secs(5))
    .build()
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_elevenlabs_token(app: AppHandle) -> Result<String, String> {
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
pub async fn test_elevenlabs(app: AppHandle) -> Result<(), String> {
  create_elevenlabs_token(app).await.map(|_| ())
}

#[tauri::command]
pub async fn list_megallm_models(app: AppHandle) -> Result<Vec<String>, String> {
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

/// Temporary: the WebView still runs provider sockets until Phase 2 moves them to Rust.
#[tauri::command]
pub async fn runtime_keys(
  app: AppHandle,
) -> Result<(Option<String>, Option<String>, Option<String>, Option<String>), String> {
  Ok((
    config::get_openrouter_key(&app).await,
    config::get_deepgram_key(&app).await,
    config::get_megallm_key(&app).await,
    config::get_elevenlabs_key(&app).await,
  ))
}
