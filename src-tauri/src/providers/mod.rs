//! Speech-to-text provider contract and registry.
//!
//! A provider adapter owns everything provider-specific: connection, auth, wire format,
//! finalization, error mapping. The engine and session manager only see [`SttEvent`]s tagged
//! with the session id, so adding a provider means implementing [`SttAdapter`] and registering
//! it; nothing in session, audio, or UI orchestration changes.

pub mod deepgram;
pub mod elevenlabs;
pub mod elevenlabs_pool;
pub mod languages;
pub mod mock;

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::session::machine::{EngineError, SessionId};

pub const SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderError {
  MissingKey,
  Auth,
  RateLimited,
  QuotaExceeded,
  Network,
  Timeout,
  Unsupported(String),
  Protocol(String),
}

impl ProviderError {
  pub fn to_engine(&self) -> EngineError {
    match self {
      ProviderError::MissingKey => EngineError::MissingKey,
      ProviderError::Auth => EngineError::Auth,
      ProviderError::RateLimited => EngineError::RateLimited,
      ProviderError::QuotaExceeded => EngineError::Provider("quota exceeded".into()),
      ProviderError::Network | ProviderError::Timeout => EngineError::Network,
      ProviderError::Unsupported(what) => EngineError::Provider(format!("unsupported {what}")),
      ProviderError::Protocol(kind) => EngineError::Provider(kind.clone()),
    }
  }

  /// Maps an HTTP status from a failed WebSocket upgrade or REST call.
  pub fn from_status(status: u16) -> Self {
    match status {
      401 | 403 => ProviderError::Auth,
      402 => ProviderError::QuotaExceeded,
      408 => ProviderError::Timeout,
      429 => ProviderError::RateLimited,
      _ => ProviderError::Protocol(format!("http {status}")),
    }
  }
}

impl std::fmt::Display for ProviderError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{:?}", self)
  }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SttEvent {
  /// Socket is open and accepting audio.
  Connected,
  Interim(String),
  Final(String),
  /// Everything was flushed after `AudioMsg::Finish`.
  Finalized,
  Failed(ProviderError),
}

#[derive(Debug, Clone)]
pub enum AudioMsg {
  /// 16 kHz mono signed 16-bit PCM.
  Pcm(Vec<i16>),
  Finish,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SttOptions {
  pub model: String,
  /// App language code (`en`, `bn`, `multi`, ...). Adapters map it to provider codes.
  pub language: String,
  /// Vocabulary hints. Adapters drop them when the model does not support hints.
  pub keyterms: Vec<String>,
}

pub struct StreamCtx {
  pub session_id: SessionId,
  pub options: SttOptions,
  pub audio: mpsc::Receiver<AudioMsg>,
  pub events: mpsc::Sender<(SessionId, SttEvent)>,
  pub cancel: CancellationToken,
}

impl StreamCtx {
  pub async fn emit(&self, event: SttEvent) {
    let _ = self.events.send((self.session_id, event)).await;
  }
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
  pub id: String,
  pub name: String,
  pub languages: Vec<String>,
  pub keyterms: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderDescriptor {
  pub id: String,
  pub name: String,
  pub models: Vec<ModelInfo>,
  pub default_model: String,
  /// Supports a pool of API keys used together.
  pub multi_key: bool,
}

impl ProviderDescriptor {
  pub fn model(&self, id: &str) -> Option<&ModelInfo> {
    self.models.iter().find(|m| m.id == id)
  }

  /// Validates and normalizes requested options. Unknown models fall back to the default;
  /// unsupported languages are an error the UI shows before dictation.
  pub fn resolve(&self, model: &str, language: &str, keyterms: Vec<String>) -> Result<SttOptions, ProviderError> {
    let m = self
      .model(model)
      .or_else(|| self.model(&self.default_model))
      .expect("default model exists");
    let supported: Vec<&str> = m.languages.iter().map(String::as_str).collect();
    let lang = languages::closest(&supported, language)
      .ok_or_else(|| ProviderError::Unsupported(format!("language {language} on {}", m.id)))?;
    Ok(SttOptions {
      model: m.id.clone(),
      language: lang.to_string(),
      keyterms: if m.keyterms { keyterms } else { Vec::new() },
    })
  }
}

pub trait SttAdapter: Send + Sync + 'static {
  fn descriptor(&self) -> ProviderDescriptor;
  /// Whether credentials are available. Never returns or exposes the secret.
  fn configured(&self) -> bool;
  /// Starts the stream on its own task. The task must end with exactly one `Finalized` or
  /// `Failed` event unless cancelled, and must stop promptly when `cancel` fires.
  fn spawn(&self, ctx: StreamCtx);
}

#[derive(Default, Clone)]
pub struct Registry {
  adapters: BTreeMap<String, Arc<dyn SttAdapter>>,
}

impl Registry {
  pub fn register(&mut self, adapter: Arc<dyn SttAdapter>) {
    self.adapters.insert(adapter.descriptor().id, adapter);
  }

  pub fn get(&self, id: &str) -> Option<Arc<dyn SttAdapter>> {
    self.adapters.get(id).cloned()
  }

  pub fn descriptors(&self) -> Vec<ProviderDescriptor> {
    self.adapters.values().map(|a| a.descriptor()).collect()
  }

  pub fn ids(&self) -> Vec<String> {
    self.adapters.keys().cloned().collect()
  }
}

/// Converts PCM samples to little-endian bytes for the wire.
pub fn pcm_bytes(pcm: &[i16]) -> Vec<u8> {
  let mut out = Vec::with_capacity(pcm.len() * 2);
  for s in pcm {
    out.extend_from_slice(&s.to_le_bytes());
  }
  out
}

/// Cleans provider error text for logs: drop long token-like runs and cap the length.
pub fn sanitize_for_log(text: &str) -> String {
  let cleaned: Vec<String> = text
    .split_whitespace()
    .map(|w| {
      if w.len() >= 24 && w.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
        "[redacted]".to_string()
      } else {
        w.to_string()
      }
    })
    .collect();
  cleaned.join(" ").chars().take(160).collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn resolve_validates_language_per_model() {
    let d = deepgram::descriptor();
    let o = d.resolve("nova-3", "bn", vec!["Dictation HUD".into()]).unwrap();
    assert_eq!(o.language, "bn");
    assert_eq!(o.keyterms.len(), 1);
    assert!(matches!(
      d.resolve("nova-2", "bn", vec![]),
      Err(ProviderError::Unsupported(_))
    ));
    // nova-2 does not take keyterms; they are dropped rather than sent.
    assert!(d
      .resolve("nova-2", "en-US", vec!["x".into()])
      .unwrap()
      .keyterms
      .is_empty());
    // Unknown model falls back to default.
    assert_eq!(d.resolve("whisper", "en", vec![]).unwrap().model, d.default_model);
  }

  #[test]
  fn status_mapping() {
    assert_eq!(ProviderError::from_status(401), ProviderError::Auth);
    assert_eq!(ProviderError::from_status(402), ProviderError::QuotaExceeded);
    assert_eq!(ProviderError::from_status(429), ProviderError::RateLimited);
  }

  #[test]
  fn sanitize_redacts_token_like_words() {
    let s = sanitize_for_log("invalid key abcdefghijklmnopqrstuvwxyz0123 provided");
    assert!(s.contains("[redacted]"));
    assert!(!s.contains("abcdefghijklmnop"));
  }

  #[test]
  fn pcm_is_little_endian() {
    assert_eq!(pcm_bytes(&[1, -2]), vec![1, 0, 0xFE, 0xFF]);
  }
}
