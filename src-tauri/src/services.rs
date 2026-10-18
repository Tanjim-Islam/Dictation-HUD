//! Long-lived backend services, created once at startup and managed by Tauri.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tauri_plugin_store::StoreExt;
use tokio::sync::Notify;

use crate::credentials::{self, names, OsSecretStore, SecretStore};
use crate::providers::deepgram::Deepgram;
use crate::providers::elevenlabs::ElevenLabs;
use crate::providers::elevenlabs_pool::{ElevenLabsPool, HttpCreditsApi, PoolData, PoolStorage};
use crate::providers::Registry;
use crate::refine::RefineProvider;

pub const POOL_EVENT: &str = "elevenlabs://pool";
const POOL_STORE_KEY: &str = "elevenlabs_pool";
/// Live polling while the provider settings are on screen.
const POOL_POLL_ACTIVE: Duration = Duration::from_secs(5);
const POOL_POLL_IDLE: Duration = Duration::from_secs(300);

pub struct Services {
  pub secrets: Arc<dyn SecretStore>,
  pub registry: Registry,
  pub pool: Arc<ElevenLabsPool>,
  pool_watch: Arc<(AtomicBool, Notify)>,
}

/// Development builds may fall back to `.env`; release builds never read it.
pub fn dev_env(var: &str) -> Option<String> {
  if !cfg!(debug_assertions) {
    return None;
  }
  let _ = dotenvy::dotenv();
  std::env::var(var)
    .ok()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeySource {
  Saved,
  DevEnv,
  None,
}

struct StorePoolStorage(AppHandle);

impl PoolStorage for StorePoolStorage {
  fn load(&self) -> PoolData {
    self
      .0
      .store("prefs.json")
      .ok()
      .and_then(|s| s.get(POOL_STORE_KEY))
      .and_then(|v| serde_json::from_value(v).ok())
      .unwrap_or_default()
  }

  fn save(&self, data: &PoolData) {
    if let Ok(store) = self.0.store("prefs.json") {
      store.set(POOL_STORE_KEY, serde_json::to_value(data).unwrap_or_default());
      let _ = store.save();
    }
  }
}

impl Services {
  pub fn init(app: &AppHandle) -> Self {
    let secrets: Arc<dyn SecretStore> = Arc::new(OsSecretStore::new(credentials::SERVICE));
    let pool = Arc::new(
      ElevenLabsPool::new(
        secrets.clone(),
        Arc::new(StorePoolStorage(app.clone())),
        Arc::new(HttpCreditsApi),
      )
      .with_dev_key(Arc::new(|| dev_env("ELEVENLABS_API_KEY"))),
    );
    let emitter = app.clone();
    pool.set_on_change(Box::new(move |view| {
      let _ = emitter.emit(POOL_EVENT, view);
    }));

    migrate_legacy_keys(app, secrets.as_ref(), &pool);

    let dg_secrets = secrets.clone();
    let mut registry = Registry::default();
    registry.register(Arc::new(Deepgram::new(Arc::new(move || {
      dg_secrets.get(names::DEEPGRAM).or_else(|| dev_env("DEEPGRAM_API_KEY"))
    }))));
    registry.register(Arc::new(ElevenLabs::new(pool.clone())));

    let services = Self {
      secrets,
      registry,
      pool,
      pool_watch: Arc::new((AtomicBool::new(false), Notify::new())),
    };
    services.spawn_pool_watcher();
    services
  }

  pub fn key_source(&self, secret: &str, env: &str) -> KeySource {
    if self.secrets.get(secret).is_some() {
      KeySource::Saved
    } else if dev_env(env).is_some() {
      KeySource::DevEnv
    } else {
      KeySource::None
    }
  }

  pub fn refine_key(&self, p: &RefineProvider) -> Option<String> {
    self.secrets.get(p.secret).or_else(|| dev_env(p.env))
  }

  /// Turns live credit polling on while the provider settings are visible.
  pub fn set_pool_watch(&self, active: bool) {
    self.pool_watch.0.store(active, Ordering::SeqCst);
    if active {
      self.pool_watch.1.notify_one();
    }
  }

  fn spawn_pool_watcher(&self) {
    let pool = self.pool.clone();
    let watch = self.pool_watch.clone();
    tauri::async_runtime::spawn(async move {
      loop {
        if !pool.view().keys.is_empty() {
          pool.refresh_all().await;
        }
        let wait = if watch.0.load(Ordering::SeqCst) {
          POOL_POLL_ACTIVE
        } else {
          POOL_POLL_IDLE
        };
        tokio::select! {
          _ = tokio::time::sleep(wait) => {}
          _ = watch.1.notified() => {}
        }
      }
    });
  }
}

/// Moves plaintext keys from older versions (`prefs.json`) into the OS credential store.
/// Each plaintext value is deleted only after the secure copy was written and read back.
fn migrate_legacy_keys(app: &AppHandle, secrets: &dyn SecretStore, pool: &ElevenLabsPool) {
  let Ok(store) = app.store("prefs.json") else { return };
  let mut changed = false;
  for (legacy, secure) in credentials::LEGACY_KEYS {
    let Some(value) = store.get(*legacy).and_then(|v| v.as_str().map(str::to_string)) else {
      continue;
    };
    match credentials::migrate_secret(secrets, secure, &value) {
      Ok(true) => {
        store.delete(*legacy);
        changed = true;
        tracing::info!(key = secure, "legacy plaintext key moved to credential store");
      }
      Ok(false) => {}
      Err(e) => tracing::error!(key = secure, error = %e, "legacy key migration failed; plaintext kept"),
    }
  }
  if let Some(value) = store
    .get(credentials::LEGACY_ELEVENLABS_KEY)
    .and_then(|v| v.as_str().map(str::to_string))
  {
    let already = pool.view().keys.len();
    let result = if value.trim().is_empty() || already > 0 {
      Ok(())
    } else {
      pool.import_unverified(&value)
    };
    match result {
      Ok(()) => {
        store.delete(credentials::LEGACY_ELEVENLABS_KEY);
        changed = true;
        tracing::info!(key = "elevenlabs", "legacy plaintext key moved to credential store");
      }
      Err(e) => tracing::error!(key = "elevenlabs", error = %e, "legacy key migration failed; plaintext kept"),
    }
  }
  if changed {
    let _ = store.save();
  }
}
