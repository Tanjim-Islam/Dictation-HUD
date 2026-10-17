//! Behavior settings, owned and persisted by Rust.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

pub const BEHAVIOR_VERSION: u32 = 1;

/// Every field has a default so an older or partial stored object keeps the fields it has
/// instead of resetting everything.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BehaviorPrefs {
  pub version: u32,
  pub auto_paste: bool,
  pub autostart: bool,
  pub ai_refine: bool,
  pub ai_provider: String,  // "openrouter" | "megallm"
  pub stt_provider: String, // "deepgram" | "elevenlabs"
  pub echo_cancellation: bool,
  pub noise_suppression: bool,
}

impl Default for BehaviorPrefs {
  fn default() -> Self {
    Self {
      version: BEHAVIOR_VERSION,
      auto_paste: true,
      autostart: false,
      ai_refine: true,
      ai_provider: "openrouter".into(),
      stt_provider: "deepgram".into(),
      echo_cancellation: true,
      noise_suppression: true,
    }
  }
}

impl BehaviorPrefs {
  /// Parses a stored value. Unknown providers fall back to defaults; missing fields keep
  /// their defaults; a non-object or mistyped value yields defaults.
  pub fn from_stored(value: serde_json::Value) -> Self {
    let mut prefs: BehaviorPrefs = serde_json::from_value(value).unwrap_or_default();
    if prefs.ai_provider != "openrouter" && prefs.ai_provider != "megallm" {
      prefs.ai_provider = "openrouter".into();
    }
    if prefs.stt_provider != "deepgram" && prefs.stt_provider != "elevenlabs" {
      prefs.stt_provider = "deepgram".into();
    }
    prefs.version = BEHAVIOR_VERSION;
    prefs
  }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub struct BehaviorPatch {
  pub auto_paste: Option<bool>,
  pub ai_refine: Option<bool>,
  pub ai_provider: Option<String>,
  pub stt_provider: Option<String>,
  pub echo_cancellation: Option<bool>,
  pub noise_suppression: Option<bool>,
}

impl BehaviorPatch {
  pub fn apply(self, prefs: &mut BehaviorPrefs) {
    if let Some(v) = self.auto_paste {
      prefs.auto_paste = v;
    }
    if let Some(v) = self.ai_refine {
      prefs.ai_refine = v;
    }
    if let Some(v) = self.ai_provider.map(|s| s.to_lowercase()) {
      if v == "openrouter" || v == "megallm" {
        prefs.ai_provider = v;
      }
    }
    if let Some(v) = self.stt_provider.map(|s| s.to_lowercase()) {
      if v == "deepgram" || v == "elevenlabs" {
        prefs.stt_provider = v;
      }
    }
    if let Some(v) = self.echo_cancellation {
      prefs.echo_cancellation = v;
    }
    if let Some(v) = self.noise_suppression {
      prefs.noise_suppression = v;
    }
  }
}

pub fn load_behavior(app: &AppHandle) -> BehaviorPrefs {
  match app.store("prefs.json") {
    Ok(store) => store
      .get("behavior")
      .map(BehaviorPrefs::from_stored)
      .unwrap_or_default(),
    Err(_) => BehaviorPrefs::default(),
  }
}

pub fn save_behavior(app: &AppHandle, prefs: &BehaviorPrefs) -> Result<(), String> {
  let store = app.store("prefs.json").map_err(|e| e.to_string())?;
  store.set("behavior", serde_json::to_value(prefs).map_err(|e| e.to_string())?);
  store.save().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn partial_behavior_keeps_defaults_for_missing_fields() {
    let prefs = BehaviorPrefs::from_stored(serde_json::json!({ "ai_refine": false, "auto_paste": false }));
    assert!(!prefs.ai_refine);
    assert!(!prefs.auto_paste);
    assert!(prefs.echo_cancellation);
    assert!(prefs.noise_suppression);
    assert_eq!(prefs.stt_provider, "deepgram");
  }

  #[test]
  fn legacy_behavior_with_removed_fields_loads() {
    let stored = serde_json::json!({
      "auto_paste": true, "silence_secs": 2, "stream_insert": false, "autostart": true,
      "ai_refine": true, "ai_provider": "megallm", "stt_provider": "elevenlabs",
      "echo_cancellation": false, "noise_suppression": true
    });
    let prefs = BehaviorPrefs::from_stored(stored);
    assert_eq!(prefs.ai_provider, "megallm");
    assert_eq!(prefs.stt_provider, "elevenlabs");
    assert!(!prefs.echo_cancellation);
    assert!(prefs.autostart);
    assert_eq!(prefs.version, BEHAVIOR_VERSION);
  }

  #[test]
  fn invalid_values_fall_back_safely() {
    let prefs = BehaviorPrefs::from_stored(serde_json::json!({ "stt_provider": "whisper", "ai_refine": "yes" }));
    assert_eq!(prefs, BehaviorPrefs::default());
    let prefs = BehaviorPrefs::from_stored(serde_json::json!({ "stt_provider": "whisper" }));
    assert_eq!(prefs.stt_provider, "deepgram");
    assert_eq!(
      BehaviorPrefs::from_stored(serde_json::json!("garbage")),
      BehaviorPrefs::default()
    );
  }

  #[test]
  fn patch_ignores_unknown_providers() {
    let mut prefs = BehaviorPrefs::default();
    BehaviorPatch {
      stt_provider: Some("ElevenLabs".into()),
      ai_provider: Some("bogus".into()),
      ..Default::default()
    }
    .apply(&mut prefs);
    assert_eq!(prefs.stt_provider, "elevenlabs");
    assert_eq!(prefs.ai_provider, "openrouter");
  }
}
