//! App settings: one versioned struct, owned and persisted by Rust (Tauri Store
//! `prefs.json`, key `settings`). WebViews read and patch it only through commands.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

pub const SETTINGS_VERSION: u32 = 2;
pub const DEFAULT_OPENROUTER_MODEL: &str = "dots-studio/dots-3-note-preview:free";
pub const DEFAULT_MEGALLM_MODEL: &str = "gpt-4o-mini";
/// Former default that OpenRouter no longer serves (HTTP 404).
const DEAD_OPENROUTER_MODELS: &[&str] = &["openai/gpt-oss-20b:free"];

/// Every field has a default so an older or partial stored object keeps the fields it has
/// instead of resetting everything.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
  pub version: u32,
  // Delivery
  pub auto_paste: bool,
  pub autostart: bool,
  // Speech to text
  pub stt_provider: String,
  pub deepgram_model: String,
  pub elevenlabs_model: String,
  /// App language code (`en`, `bn`, `multi`, ...).
  pub language: String,
  // Audio
  pub input_device: Option<String>,
  pub echo_cancellation: bool,
  pub noise_suppression: bool,
  // AI refinement
  pub ai_refine: bool,
  pub ai_provider: String,
  pub openrouter_model: String,
  pub megallm_model: String,
  // History
  /// Days to keep local recordings (0 = keep forever). Transcripts are always kept.
  pub audio_retention_days: u32,
  // Flow Bar
  /// Screen edge the Flow Bar docks to: `bottom`, `top`, `left`, or `right`.
  pub flowbar_edge: String,
}

/// Longest finite retention offered.
pub const MAX_RETENTION_DAYS: u32 = 3_650;

/// Kept for call sites written before the rename.
pub type BehaviorPrefs = Settings;

impl Default for Settings {
  fn default() -> Self {
    Self {
      version: SETTINGS_VERSION,
      auto_paste: true,
      autostart: false,
      stt_provider: "deepgram".into(),
      deepgram_model: "nova-3".into(),
      elevenlabs_model: "scribe_v2_realtime".into(),
      language: "en".into(),
      input_device: None,
      echo_cancellation: true,
      noise_suppression: true,
      ai_refine: true,
      ai_provider: "openrouter".into(),
      openrouter_model: DEFAULT_OPENROUTER_MODEL.into(),
      megallm_model: DEFAULT_MEGALLM_MODEL.into(),
      audio_retention_days: 14,
      flowbar_edge: "bottom".into(),
    }
  }
}

impl Settings {
  /// Parses a stored value. Missing fields keep defaults; a mistyped object yields defaults.
  pub fn from_stored(value: serde_json::Value) -> Self {
    let mut s: Settings = serde_json::from_value(value).unwrap_or_default();
    s.sanitize();
    s
  }

  fn sanitize(&mut self) {
    if self.ai_provider != "openrouter" && self.ai_provider != "megallm" {
      self.ai_provider = "openrouter".into();
    }
    if self.stt_provider != "deepgram" && self.stt_provider != "elevenlabs" {
      self.stt_provider = "deepgram".into();
    }
    if self.openrouter_model.trim().is_empty() || DEAD_OPENROUTER_MODELS.contains(&self.openrouter_model.as_str()) {
      self.openrouter_model = DEFAULT_OPENROUTER_MODEL.into();
    }
    if self.megallm_model.trim().is_empty() {
      self.megallm_model = DEFAULT_MEGALLM_MODEL.into();
    }
    if self.language.trim().is_empty() {
      self.language = "en".into();
    }
    if self.input_device.as_deref().is_some_and(|d| d.is_empty()) {
      self.input_device = None;
    }
    self.audio_retention_days = self.audio_retention_days.min(MAX_RETENTION_DAYS);
    if crate::flowbar::Edge::parse(&self.flowbar_edge).is_none() {
      self.flowbar_edge = "bottom".into();
    }
    self.version = SETTINGS_VERSION;
  }

  pub fn stt_model(&self) -> &str {
    match self.stt_provider.as_str() {
      "elevenlabs" => &self.elevenlabs_model,
      _ => &self.deepgram_model,
    }
  }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct SettingsPatch {
  pub auto_paste: Option<bool>,
  pub stt_provider: Option<String>,
  pub deepgram_model: Option<String>,
  pub elevenlabs_model: Option<String>,
  pub language: Option<String>,
  /// Empty string selects the system default microphone.
  pub input_device: Option<String>,
  pub echo_cancellation: Option<bool>,
  pub noise_suppression: Option<bool>,
  pub ai_refine: Option<bool>,
  pub ai_provider: Option<String>,
  pub openrouter_model: Option<String>,
  pub megallm_model: Option<String>,
  pub audio_retention_days: Option<u32>,
  pub flowbar_edge: Option<String>,
}

pub type BehaviorPatch = SettingsPatch;

impl SettingsPatch {
  /// Names of the fields this patch sets (for diagnostics; no values).
  pub fn fields(&self) -> Vec<&'static str> {
    let mut f = Vec::new();
    macro_rules! has {
      ($($n:ident),*) => { $( if self.$n.is_some() { f.push(stringify!($n)); } )* };
    }
    has!(
      auto_paste,
      stt_provider,
      deepgram_model,
      elevenlabs_model,
      language,
      input_device,
      echo_cancellation,
      noise_suppression,
      ai_refine,
      ai_provider,
      openrouter_model,
      megallm_model,
      audio_retention_days,
      flowbar_edge
    );
    f
  }

  pub fn apply(self, s: &mut Settings) {
    macro_rules! set {
      ($($f:ident),*) => { $( if let Some(v) = self.$f { s.$f = v; } )* };
    }
    set!(
      auto_paste,
      echo_cancellation,
      noise_suppression,
      ai_refine,
      audio_retention_days
    );
    if let Some(v) = self.stt_provider {
      s.stt_provider = v.to_lowercase();
    }
    if let Some(v) = self.ai_provider {
      s.ai_provider = v.to_lowercase();
    }
    set!(
      deepgram_model,
      elevenlabs_model,
      language,
      openrouter_model,
      megallm_model,
      flowbar_edge
    );
    if let Some(v) = self.input_device {
      s.input_device = if v.is_empty() { None } else { Some(v) };
    }
    s.sanitize();
  }
}

/// Builds settings from the store, migrating v1 (`behavior` + loose `model`,
/// `megallm_model`, `language` keys) on first load. Pure so it can be tested.
pub fn migrate(get: impl Fn(&str) -> Option<serde_json::Value>) -> (Settings, bool) {
  if let Some(v) = get("settings") {
    return (Settings::from_stored(v), false);
  }
  let mut s = get("behavior").map(Settings::from_stored).unwrap_or_default();
  let text = |k: &str| {
    get(k)
      .and_then(|v| v.as_str().map(str::to_string))
      .filter(|v| !v.is_empty())
  };
  if let Some(m) = text("model") {
    s.openrouter_model = m;
  }
  if let Some(m) = text("megallm_model") {
    s.megallm_model = m;
  }
  if let Some(l) = text("language") {
    s.language = l;
  }
  s.sanitize();
  (s, true)
}

pub fn load(app: &AppHandle) -> Settings {
  let Ok(store) = app.store("prefs.json") else {
    return Settings::default();
  };
  let (settings, migrated) = migrate(|k| store.get(k));
  if migrated {
    store.set("settings", serde_json::to_value(&settings).unwrap_or_default());
    for k in ["behavior", "model", "megallm_model", "language"] {
      store.delete(k);
    }
    let _ = store.save();
    tracing::info!("settings migrated to v{SETTINGS_VERSION}");
  }
  settings
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
  let store = app.store("prefs.json").map_err(|e| e.to_string())?;
  store.set("settings", serde_json::to_value(settings).map_err(|e| e.to_string())?);
  store.save().map_err(|e| e.to_string())
}

pub fn load_behavior(app: &AppHandle) -> Settings {
  load(app)
}

pub fn save_behavior(app: &AppHandle, settings: &Settings) -> Result<(), String> {
  save(app, settings)
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::collections::HashMap;

  fn store(pairs: &[(&str, serde_json::Value)]) -> HashMap<String, serde_json::Value> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
  }

  #[test]
  fn partial_settings_keep_defaults_for_missing_fields() {
    let s = Settings::from_stored(serde_json::json!({ "ai_refine": false, "auto_paste": false }));
    assert!(!s.ai_refine);
    assert!(!s.auto_paste);
    assert!(s.echo_cancellation);
    assert_eq!(s.stt_provider, "deepgram");
    assert_eq!(s.deepgram_model, "nova-3");
  }

  #[test]
  fn v1_store_migrates_including_loose_keys() {
    let m = store(&[
      (
        "behavior",
        serde_json::json!({
          "auto_paste": true, "silence_secs": 2, "stream_insert": false, "autostart": true,
          "ai_refine": true, "ai_provider": "megallm", "stt_provider": "elevenlabs",
          "echo_cancellation": false, "noise_suppression": true
        }),
      ),
      ("model", serde_json::json!("openai/gpt-oss-20b:free")),
      ("megallm_model", serde_json::json!("claude-x")),
      ("language", serde_json::json!("en-US")),
    ]);
    let (s, migrated) = migrate(|k| m.get(k).cloned());
    assert!(migrated);
    assert_eq!(s.ai_provider, "megallm");
    assert_eq!(s.stt_provider, "elevenlabs");
    assert!(!s.echo_cancellation);
    assert_eq!(s.megallm_model, "claude-x");
    assert_eq!(s.language, "en-US");
    assert_eq!(s.openrouter_model, DEFAULT_OPENROUTER_MODEL, "dead model replaced");
    assert_eq!(s.version, SETTINGS_VERSION);
  }

  #[test]
  fn existing_v2_is_not_remigrated() {
    let m = store(&[
      ("settings", serde_json::json!({ "language": "bn" })),
      ("language", serde_json::json!("fr")),
    ]);
    let (s, migrated) = migrate(|k| m.get(k).cloned());
    assert!(!migrated);
    assert_eq!(s.language, "bn");
  }

  #[test]
  fn invalid_values_fall_back_safely() {
    assert_eq!(
      Settings::from_stored(serde_json::json!({ "ai_refine": "yes" })),
      Settings::default()
    );
    assert_eq!(
      Settings::from_stored(serde_json::json!({ "stt_provider": "whisper" })).stt_provider,
      "deepgram"
    );
    assert_eq!(Settings::from_stored(serde_json::json!("garbage")), Settings::default());
  }

  #[test]
  fn patch_validates_and_clears_device() {
    let mut s = Settings {
      input_device: Some("wasapi:x".into()),
      ..Default::default()
    };
    SettingsPatch {
      stt_provider: Some("ElevenLabs".into()),
      ai_provider: Some("bogus".into()),
      input_device: Some(String::new()),
      language: Some("bn".into()),
      ..Default::default()
    }
    .apply(&mut s);
    assert_eq!(s.stt_provider, "elevenlabs");
    assert_eq!(s.ai_provider, "openrouter");
    assert_eq!(s.input_device, None);
    assert_eq!(s.language, "bn");
    assert_eq!(s.stt_model(), "scribe_v2_realtime");
  }

  #[test]
  fn retention_defaults_to_14_days_and_is_bounded() {
    assert_eq!(Settings::default().audio_retention_days, 14);
    let s = Settings::from_stored(serde_json::json!({ "audio_retention_days": 999_999 }));
    assert_eq!(s.audio_retention_days, MAX_RETENTION_DAYS);
    let mut s = Settings::default();
    SettingsPatch {
      audio_retention_days: Some(0),
      ..Default::default()
    }
    .apply(&mut s);
    assert_eq!(s.audio_retention_days, 0, "0 keeps recordings forever");
  }

  #[test]
  fn flowbar_edge_is_validated() {
    assert_eq!(Settings::default().flowbar_edge, "bottom");
    assert_eq!(
      Settings::from_stored(serde_json::json!({ "flowbar_edge": "left" })).flowbar_edge,
      "left"
    );
    assert_eq!(
      Settings::from_stored(serde_json::json!({ "flowbar_edge": "middle" })).flowbar_edge,
      "bottom"
    );
  }
}
