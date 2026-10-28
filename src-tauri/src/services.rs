//! Long-lived backend services, created once at startup and managed by Tauri.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;
use tokio::sync::Notify;

use crate::credentials::{self, names, OsSecretStore, SecretStore};
use crate::local_asr::client::{Helper, Limits, ProcessLauncher};
use crate::local_asr::store::{default_hf_hub, ModelsDir};
use crate::providers::deepgram::Deepgram;
use crate::providers::elevenlabs::ElevenLabs;
use crate::providers::elevenlabs_pool::{ElevenLabsPool, HttpCreditsApi, PoolData, PoolStorage};
use crate::providers::local::{LocalAdapter, ModelLocator};
use crate::providers::Registry;
use crate::refine::RefineProvider;

pub const POOL_EVENT: &str = "elevenlabs://pool";
const POOL_STORE_KEY: &str = "elevenlabs_pool";
/// Live polling while the provider settings are on screen. Each round refreshes every key,
/// so it stays gentle with large pools.
const POOL_POLL_ACTIVE: Duration = Duration::from_secs(30);
const POOL_POLL_IDLE: Duration = Duration::from_secs(300);

pub struct Services {
  pub secrets: Arc<dyn SecretStore>,
  pub registry: Registry,
  pub pool: Arc<ElevenLabsPool>,
  pool_watch: Arc<(AtomicBool, Notify)>,
  /// The app's own local speech model store (the default folder).
  models: ModelsDir,
  /// The local speech helper process (shared by dictation, retries, and linking).
  pub helper: Arc<Helper>,
}

/// Resolves local speech models from settings (linked folders) and the model store.
struct AppLocator {
  app: AppHandle,
  models: ModelsDir,
}

impl ModelLocator for AppLocator {
  fn model_dir(&self, id: &str) -> Option<PathBuf> {
    let m = crate::local_asr::catalog::get(id)?;
    let s = crate::settings::load(&self.app);
    models_for(&self.models, &s)
      .resolve(m, s.local_model_paths.get(id).map(Path::new))
      .dir()
  }

  fn vad_path(&self) -> Option<PathBuf> {
    let p = self.app.path().resource_dir().ok()?.join("silero_vad.onnx");
    p.exists().then_some(p)
  }

  fn enough_memory(&self, bytes: u64) -> bool {
    crate::local_asr::available_memory().is_none_or(|a| a >= bytes + bytes / 2)
  }
}

/// The model store the settings select: the user's folder for downloads, or the default store.
fn models_for(default: &ModelsDir, s: &crate::settings::Settings) -> ModelsDir {
  default.select(s.local_models_dir.as_deref().map(Path::new))
}

/// `%LOCALAPPDATA%\com.r0ju.dictationhud\models\stt`, or `DICTATION_MODELS_DIR` in debug builds.
fn models_root(app: &AppHandle) -> PathBuf {
  if let Some(dir) = dev_env("DICTATION_MODELS_DIR") {
    return PathBuf::from(dir);
  }
  app
    .path()
    .app_local_data_dir()
    .expect("app data dir")
    .join(r"models\stt")
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
      if store.save().is_err() {
        tracing::warn!("could not persist ElevenLabs pool metadata");
      }
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

    let models = ModelsDir::new(models_root(app), default_hf_hub(|k| std::env::var(k).ok()));
    let removed = models_for(&models, &crate::settings::load(app)).cleanup_parts();
    if removed > 0 {
      tracing::info!(removed, "removed unfinished model downloads");
    }
    let helper = Helper::new(Arc::new(ProcessLauncher::find()), Limits::default());
    registry.register(Arc::new(LocalAdapter::new(
      helper.clone(),
      Arc::new(AppLocator {
        app: app.clone(),
        models: models.clone(),
      }),
    )));
    let reaper = helper.clone();
    tauri::async_runtime::spawn(async move {
      loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
        reaper.reap_if_idle();
      }
    });

    let services = Self {
      secrets,
      registry,
      pool,
      pool_watch: Arc::new((AtomicBool::new(false), Notify::new())),
      models,
      helper,
    };
    services.spawn_pool_watcher();
    services
  }

  /// The model store for these settings (see [`ModelsDir::select`]).
  pub fn models(&self, s: &crate::settings::Settings) -> ModelsDir {
    models_for(&self.models, s)
  }

  /// The app's own model store, used when no folder is picked.
  pub fn default_models(&self) -> &ModelsDir {
    &self.models
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
    if p.local.is_some() {
      return None;
    }
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
          if watch.0.load(Ordering::SeqCst) {
            pool.refresh_all().await;
          } else {
            pool.refresh_stale().await;
          }
          pool.probe_idle().await;
        }
        // Credits do not gate dictation, so a session needs no extra polling.
        let wait = if watch.0.load(Ordering::SeqCst) {
          POOL_POLL_ACTIVE
        } else {
          POOL_POLL_IDLE
        };
        tokio::select! {
          _ = tokio::time::sleep(wait) => {}
          _ = watch.1.notified() => {}
          _ = pool.activity_changed() => {}
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
