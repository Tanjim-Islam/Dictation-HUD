use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const K_OPENROUTER: &str = "openrouter_key";
const K_DEEPGRAM: &str = "deepgram_key";
const K_MEGALLM: &str = "megallm_key";
const K_ELEVENLABS: &str = "elevenlabs_key";
const K_MEGALLM_MODEL: &str = "megallm_model";

fn env_default(key: &str) -> Option<String> {
  // Load .env once
  let _ = dotenvy::dotenv();
  std::env::var(key).ok().filter(|s| !s.is_empty())
}

pub async fn set_openrouter_key(app: &AppHandle, key: &str) -> anyhow::Result<()> {
  let store = app.store("prefs.json")?;
  store.set(K_OPENROUTER, key);
  store.save()?;
  Ok(())
}

pub async fn get_openrouter_key(app: &AppHandle) -> Option<String> {
  eprintln!("?? Getting OpenRouter key...");
  let store = app.store("prefs.json").ok();
  let stored = store.as_ref().and_then(|s| s.get(K_OPENROUTER).and_then(|v| v.as_str().map(|s| s.to_string())));

  if let Some(ref key) = stored {
    eprintln!("? OpenRouter key found in store: {}...", &key[..key.len().min(10)]);
    Some(key.clone())
  } else {
    eprintln!("?? No OpenRouter key in store, checking environment...");
    let env_key = env_default("OPENROUTER_API_KEY");
    if let Some(ref key) = env_key {
      eprintln!("? OpenRouter key found in environment: {}...", &key[..key.len().min(10)]);
    } else {
      eprintln!("? No OpenRouter key in environment either");
    }
    env_key
  }
}

pub async fn set_megallm_key(app: &AppHandle, key: &str) -> anyhow::Result<()> {
  eprintln!("?? Saving MegaLLM key to store...");
  let store = app.store("prefs.json")?;
  store.set(K_MEGALLM, key);
  store.save()?;
  eprintln!("? MegaLLM key saved");
  Ok(())
}

pub async fn get_megallm_key(app: &AppHandle) -> Option<String> {
  eprintln!("?? Getting MegaLLM key...");
  let store = app.store("prefs.json").ok();
  let stored = store.as_ref().and_then(|s| s.get(K_MEGALLM).and_then(|v| v.as_str().map(|s| s.to_string())));

  if let Some(ref key) = stored {
    eprintln!("? MegaLLM key found in store: {}...", &key[..key.len().min(10)]);
    Some(key.clone())
  } else {
    eprintln!("?? No MegaLLM key in store, checking environment...");
    let env_key = env_default("MEGALLM_API_KEY");
    if let Some(ref key) = env_key {
      eprintln!("? MegaLLM key found in environment: {}...", &key[..key.len().min(10)]);
    } else {
      eprintln!("? No MegaLLM key in environment either");
    }
    env_key
  }
}

pub async fn set_deepgram_key(app: &AppHandle, key: &str) -> anyhow::Result<()> {
  eprintln!("?? Saving Deepgram key to store...");
  let store = app.store("prefs.json")?;
  store.set(K_DEEPGRAM, key);
  store.save()?;
  eprintln!("? Deepgram key saved");
  Ok(())
}

pub async fn get_deepgram_key(app: &AppHandle) -> Option<String> {
  eprintln!("?? Getting Deepgram key...");
  let store = app.store("prefs.json").ok();
  let stored = store.as_ref().and_then(|s| s.get(K_DEEPGRAM).and_then(|v| v.as_str().map(|s| s.to_string())));

  if let Some(ref key) = stored {
    eprintln!("? Deepgram key found in store: {}...", &key[..key.len().min(10)]);
    Some(key.clone())
  } else {
    eprintln!("?? No Deepgram key in store, checking environment...");
    let env_key = env_default("DEEPGRAM_API_KEY");
    if let Some(ref key) = env_key {
      eprintln!("? Deepgram key found in environment: {}...", &key[..key.len().min(10)]);
    } else {
      eprintln!("? No Deepgram key in environment either");
    }
    env_key
  }
}

pub async fn set_elevenlabs_key(app: &AppHandle, key: &str) -> anyhow::Result<()> {
  eprintln!("?? Saving ElevenLabs key to store...");
  let store = app.store("prefs.json")?;
  store.set(K_ELEVENLABS, key);
  store.save()?;
  eprintln!("? ElevenLabs key saved");
  Ok(())
}

pub async fn get_elevenlabs_key(app: &AppHandle) -> Option<String> {
  eprintln!("?? Getting ElevenLabs key...");
  let store = app.store("prefs.json").ok();
  let stored = store.as_ref().and_then(|s| s.get(K_ELEVENLABS).and_then(|v| v.as_str().map(|s| s.to_string())));

  if let Some(ref key) = stored {
    eprintln!("? ElevenLabs key found in store: {}...", &key[..key.len().min(10)]);
    Some(key.clone())
  } else {
    eprintln!("?? No ElevenLabs key in store, checking environment...");
    let env_key = env_default("ELEVENLABS_API_KEY");
    if let Some(ref key) = env_key {
      eprintln!("? ElevenLabs key found in environment: {}...", &key[..key.len().min(10)]);
    } else {
      eprintln!("? No ElevenLabs key in environment either");
    }
    env_key
  }
}

pub async fn set_model(app: &AppHandle, name: &str) -> anyhow::Result<()> {
  let store = app.store("prefs.json")?;
  store.set("model", name);
  store.save()?;
  Ok(())
}

pub async fn get_model(app: &AppHandle) -> Option<String> {
  let store = app.store("prefs.json").ok()?;
  store.get("model").and_then(|v| v.as_str().map(|s| s.to_string()))
}

pub async fn set_megallm_model(app: &AppHandle, name: &str) -> anyhow::Result<()> {
  let store = app.store("prefs.json")?;
  store.set(K_MEGALLM_MODEL, name);
  store.save()?;
  Ok(())
}

pub async fn get_megallm_model(app: &AppHandle) -> Option<String> {
  let store = app.store("prefs.json").ok()?;
  store.get(K_MEGALLM_MODEL).and_then(|v| v.as_str().map(|s| s.to_string()))
}

pub async fn set_language(app: &AppHandle, code: &str) -> anyhow::Result<()> {
  let store = app.store("prefs.json")?;
  store.set("language", code);
  store.save()?;
  Ok(())
}

pub async fn get_language(app: &AppHandle) -> Option<String> {
  let store = app.store("prefs.json").ok()?;
  store.get("language").and_then(|v| v.as_str().map(|s| s.to_string()))
}
