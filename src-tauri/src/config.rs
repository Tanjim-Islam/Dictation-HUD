use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const K_OPENROUTER: &str = "openrouter_key";
const K_DEEPGRAM: &str = "deepgram_key";
const K_MEGALLM: &str = "megallm_key";
const K_ELEVENLABS: &str = "elevenlabs_key";
const K_MEGALLM_MODEL: &str = "megallm_model";

/// Development-only fallback to `.env`. Release builds never read it.
fn env_default(key: &str) -> Option<String> {
  if !cfg!(debug_assertions) {
    return None;
  }
  let _ = dotenvy::dotenv();
  std::env::var(key).ok().filter(|s| !s.is_empty())
}

fn store_string(app: &AppHandle, key: &str) -> Option<String> {
  let store = app.store("prefs.json").ok()?;
  store
    .get(key)
    .and_then(|v| v.as_str().map(|s| s.to_string()))
    .filter(|s| !s.is_empty())
}

fn set_store_string(app: &AppHandle, key: &str, value: &str) -> anyhow::Result<()> {
  let store = app.store("prefs.json")?;
  store.set(key, value);
  store.save()?;
  Ok(())
}

fn get_key(app: &AppHandle, store_key: &str, env_key: &str) -> Option<String> {
  store_string(app, store_key).or_else(|| env_default(env_key))
}

pub async fn set_openrouter_key(app: &AppHandle, key: &str) -> anyhow::Result<()> {
  set_store_string(app, K_OPENROUTER, key)
}
pub async fn get_openrouter_key(app: &AppHandle) -> Option<String> {
  get_key(app, K_OPENROUTER, "OPENROUTER_API_KEY")
}
pub async fn set_megallm_key(app: &AppHandle, key: &str) -> anyhow::Result<()> {
  set_store_string(app, K_MEGALLM, key)
}
pub async fn get_megallm_key(app: &AppHandle) -> Option<String> {
  get_key(app, K_MEGALLM, "MEGALLM_API_KEY")
}
pub async fn set_deepgram_key(app: &AppHandle, key: &str) -> anyhow::Result<()> {
  set_store_string(app, K_DEEPGRAM, key)
}
pub async fn get_deepgram_key(app: &AppHandle) -> Option<String> {
  get_key(app, K_DEEPGRAM, "DEEPGRAM_API_KEY")
}
pub async fn set_elevenlabs_key(app: &AppHandle, key: &str) -> anyhow::Result<()> {
  set_store_string(app, K_ELEVENLABS, key)
}
pub async fn get_elevenlabs_key(app: &AppHandle) -> Option<String> {
  get_key(app, K_ELEVENLABS, "ELEVENLABS_API_KEY")
}

pub async fn set_model(app: &AppHandle, name: &str) -> anyhow::Result<()> {
  set_store_string(app, "model", name)
}
pub async fn get_model(app: &AppHandle) -> Option<String> {
  store_string(app, "model")
}
pub async fn set_megallm_model(app: &AppHandle, name: &str) -> anyhow::Result<()> {
  set_store_string(app, K_MEGALLM_MODEL, name)
}
pub async fn get_megallm_model(app: &AppHandle) -> Option<String> {
  store_string(app, K_MEGALLM_MODEL)
}
pub async fn set_language(app: &AppHandle, code: &str) -> anyhow::Result<()> {
  set_store_string(app, "language", code)
}
pub async fn get_language(app: &AppHandle) -> Option<String> {
  store_string(app, "language")
}
