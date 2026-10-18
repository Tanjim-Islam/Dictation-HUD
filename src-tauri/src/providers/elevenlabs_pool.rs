//! Pool of ElevenLabs API keys used together.
//!
//! Selection and failure rules are ported from the MonerJanala implementation (proven in
//! production there): eligible keys need `remaining - reserved >= cost + 500`, the least
//! recently used eligible key wins, 401/403 marks a key invalid, quota errors mark it low on
//! credit with a 5 minute cooldown, 429/5xx put it on a 60 second cooldown, and a failing key
//! is excluded while the next one is tried. Added here: runtime add/remove with duplicate
//! detection, OS credential storage, request timeouts, and live credit polling.
//!
//! Secrets live only in the OS credential store (`elevenlabs:<id>`). Everything in
//! [`PoolData`] is non-secret metadata.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::credentials::{names, SecretStore};

use super::ProviderError;

pub const CREDIT_BUFFER: u64 = 500;
const LOW_CREDIT_COOLDOWN_MS: u64 = 5 * 60 * 1000;
const TRANSIENT_COOLDOWN_MS: u64 = 60 * 1000;
pub const FRESH_MS: u64 = 5 * 60 * 1000;
/// Estimated credits held while a realtime session runs. Real usage shows up at the next
/// subscription refresh, which runs right after each session.
const SESSION_RESERVATION: u64 = 1;

pub fn now_ms() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.as_millis() as u64)
    .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyStatus {
  Unknown,
  Usable,
  LowCredit,
  Invalid,
  Cooldown,
  Error,
  /// Key works for speech-to-text but cannot read its subscription (restricted key).
  NoCreditAccess,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct KeyEntry {
  pub id: String,
  pub added_at_ms: u64,
  pub status: KeyStatus,
  pub character_count: Option<u64>,
  pub character_limit: Option<u64>,
  pub reserved: u64,
  pub last_checked_ms: Option<u64>,
  pub last_used_ms: Option<u64>,
  pub last_error: Option<String>,
  pub cooldown_until_ms: Option<u64>,
  pub consecutive_failures: u32,
}

impl Default for KeyEntry {
  fn default() -> Self {
    Self {
      id: String::new(),
      added_at_ms: 0,
      status: KeyStatus::Unknown,
      character_count: None,
      character_limit: None,
      reserved: 0,
      last_checked_ms: None,
      last_used_ms: None,
      last_error: None,
      cooldown_until_ms: None,
      consecutive_failures: 0,
    }
  }
}

impl KeyEntry {
  pub fn remaining(&self) -> Option<u64> {
    Some(self.character_limit?.saturating_sub(self.character_count?))
  }

  fn cooling(&self, now: u64) -> bool {
    self.cooldown_until_ms.is_some_and(|t| t > now)
  }

  fn eligible(&self, now: u64, cost: u64) -> bool {
    let status_ok = match self.status {
      KeyStatus::Usable | KeyStatus::Unknown | KeyStatus::NoCreditAccess => true,
      // A cooldown status ends when the timer does.
      KeyStatus::Cooldown => !self.cooling(now),
      _ => false,
    };
    if !status_ok || self.cooling(now) {
      return false;
    }
    match self.remaining() {
      Some(rem) => rem.saturating_sub(self.reserved) >= cost + CREDIT_BUFFER,
      None => true,
    }
  }

  fn stale(&self, now: u64) -> bool {
    self.last_checked_ms.is_none_or(|t| now.saturating_sub(t) > FRESH_MS)
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct PoolData {
  /// When false only the first key is used.
  pub multi: bool,
  pub keys: Vec<KeyEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct KeyView {
  pub id: String,
  pub label: String,
  pub status: KeyStatus,
  pub remaining: Option<u64>,
  pub limit: Option<u64>,
  pub last_checked_ms: Option<u64>,
  pub last_error: Option<String>,
  pub active: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PoolView {
  pub multi: bool,
  pub keys: Vec<KeyView>,
  pub total_remaining: u64,
  pub total_limit: u64,
  pub configured: bool,
  /// Debug builds only: no saved key, but a `.env` key is available.
  pub using_dev_key: bool,
  pub last_used_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Credits {
  pub character_count: u64,
  pub character_limit: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FetchError {
  Status(u16),
  Network,
  Malformed,
}

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

pub trait CreditsApi: Send + Sync {
  fn fetch(&self, secret: String) -> BoxFuture<Result<Credits, FetchError>>;
}

pub trait PoolStorage: Send + Sync {
  fn load(&self) -> PoolData;
  fn save(&self, data: &PoolData);
}

#[derive(Debug, PartialEq)]
pub enum AddError {
  Empty,
  Duplicate,
  Invalid,
  Unverified,
  Store(String),
}

impl std::fmt::Display for AddError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      AddError::Empty => write!(f, "Paste an API key first"),
      AddError::Duplicate => write!(f, "This key is already in the pool"),
      AddError::Invalid => write!(f, "ElevenLabs rejected this key"),
      AddError::Unverified => write!(f, "Could not reach ElevenLabs to verify the key"),
      AddError::Store(e) => write!(f, "Could not save the key securely ({e})"),
    }
  }
}

/// Result of adding keys from pasted text. Counts only; never contains a key.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct ImportReport {
  /// Keys recognized in the text.
  pub found: usize,
  pub added: usize,
  pub duplicates: usize,
  pub rejected: usize,
  pub unverified: usize,
  pub failed: usize,
  /// Whether Multiple APIs was on (otherwise only the first key was used).
  pub multi: bool,
}

/// Finds ElevenLabs API keys in pasted text, in order and without duplicates. Accepts any mix
/// of `NAME=key`, `export NAME="key"`, `NAME: key`, comma or line separated, or bare keys; the
/// variable names are ignored. A key is `sk_` followed by 20+ letters or digits, or a legacy
/// 32-character hex key. If nothing matches, a single bare token is taken as one key.
pub fn parse_keys(text: &str) -> Vec<String> {
  let is_token = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
  let looks_like_key = |t: &str| {
    (t.starts_with("sk_") && t.len() >= 23 && t[3..].chars().all(|c| c.is_ascii_alphanumeric()))
      || (t.len() == 32 && t.chars().all(|c| c.is_ascii_hexdigit()))
  };
  let mut out: Vec<String> = Vec::new();
  for token in text.split(|c: char| !is_token(c)).filter(|t| !t.is_empty()) {
    if looks_like_key(token) && !out.iter().any(|k| k == token) {
      out.push(token.to_string());
    }
  }
  if out.is_empty() {
    let trimmed = text.trim().trim_matches(|c| c == '"' || c == '\'');
    if !trimmed.is_empty() && !trimmed.contains(char::is_whitespace) && !trimmed.contains('=') {
      out.push(trimmed.to_string());
    }
  }
  out
}

/// A key handed to a session. Must be released with the outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct Lease {
  pub key_id: String,
  pub secret: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LeaseOutcome {
  Ok,
  Auth,
  Quota,
  Transient,
  Other,
}

type ChangeFn = Box<dyn Fn(&PoolView) + Send + Sync>;

pub struct ElevenLabsPool {
  secrets: Arc<dyn SecretStore>,
  storage: Arc<dyn PoolStorage>,
  api: Arc<dyn CreditsApi>,
  data: Mutex<PoolData>,
  dev_key: Option<Arc<dyn Fn() -> Option<String> + Send + Sync>>,
  on_change: Mutex<Option<ChangeFn>>,
}

impl ElevenLabsPool {
  pub fn new(secrets: Arc<dyn SecretStore>, storage: Arc<dyn PoolStorage>, api: Arc<dyn CreditsApi>) -> Self {
    let data = storage.load();
    Self {
      secrets,
      storage,
      api,
      data: Mutex::new(data),
      dev_key: None,
      on_change: Mutex::new(None),
    }
  }

  pub fn with_dev_key(mut self, dev: Arc<dyn Fn() -> Option<String> + Send + Sync>) -> Self {
    self.dev_key = Some(dev);
    self
  }

  pub fn set_on_change(&self, f: ChangeFn) {
    *self.on_change.lock().unwrap() = Some(f);
  }

  fn commit(&self, data: &PoolData) {
    self.storage.save(data);
    let view = Self::view_of(data, self.dev_available(data));
    if let Some(f) = self.on_change.lock().unwrap().as_ref() {
      f(&view);
    }
  }

  fn dev_available(&self, data: &PoolData) -> bool {
    data.keys.is_empty() && self.dev_key.as_ref().is_some_and(|f| f().is_some())
  }

  fn view_of(data: &PoolData, using_dev_key: bool) -> PoolView {
    let active_count = if data.multi {
      data.keys.len()
    } else {
      data.keys.len().min(1)
    };
    let keys: Vec<KeyView> = data
      .keys
      .iter()
      .enumerate()
      .map(|(i, k)| KeyView {
        id: k.id.clone(),
        label: format!("Key {}", i + 1),
        status: k.status,
        remaining: k.remaining(),
        limit: k.character_limit,
        last_checked_ms: k.last_checked_ms,
        last_error: k.last_error.clone(),
        active: i < active_count,
      })
      .collect();
    let counted = keys.iter().filter(|k| k.active && k.status != KeyStatus::Invalid);
    let (rem, lim) = counted.fold((0u64, 0u64), |(r, l), k| {
      (r + k.remaining.unwrap_or(0), l + k.limit.unwrap_or(0))
    });
    let last_used_id = data
      .keys
      .iter()
      .filter(|k| k.last_used_ms.is_some())
      .max_by_key(|k| k.last_used_ms)
      .map(|k| k.id.clone());
    PoolView {
      multi: data.multi,
      configured: !data.keys.is_empty() || using_dev_key,
      keys,
      total_remaining: rem,
      total_limit: lim,
      using_dev_key,
      last_used_id,
    }
  }

  pub fn view(&self) -> PoolView {
    let data = self.data.lock().unwrap();
    Self::view_of(&data, self.dev_available(&data))
  }

  pub fn configured(&self) -> bool {
    self.view().configured
  }

  pub fn set_multi(&self, multi: bool) {
    let mut data = self.data.lock().unwrap();
    data.multi = multi;
    let snapshot = data.clone();
    drop(data);
    self.commit(&snapshot);
  }

  /// Verifies and stores a new key. The secret is written to the OS store before metadata.
  pub async fn add(&self, secret: &str) -> Result<KeyView, AddError> {
    let secret = secret.trim().to_string();
    if secret.is_empty() {
      return Err(AddError::Empty);
    }
    let ids: Vec<String> = self.data.lock().unwrap().keys.iter().map(|k| k.id.clone()).collect();
    if ids
      .iter()
      .any(|id| self.secrets.get(&names::elevenlabs(id)).as_deref() == Some(secret.as_str()))
    {
      return Err(AddError::Duplicate);
    }
    let now = now_ms();
    let mut entry = KeyEntry {
      id: uuid::Uuid::new_v4().to_string(),
      added_at_ms: now,
      ..Default::default()
    };
    match self.api.fetch(secret.clone()).await {
      Ok(c) => apply_credits(&mut entry, &c, now),
      // ElevenLabs answers 400 for a malformed key and 401 for an unknown one.
      Err(FetchError::Status(400 | 401)) => return Err(AddError::Invalid),
      Err(FetchError::Status(403)) => {
        entry.status = KeyStatus::NoCreditAccess;
        entry.last_checked_ms = Some(now);
        entry.last_error = Some("Key cannot read credits (restricted permissions)".into());
      }
      Err(_) => return Err(AddError::Unverified),
    }
    self
      .secrets
      .set(&names::elevenlabs(&entry.id), &secret)
      .map_err(|e| AddError::Store(e.to_string()))?;
    let mut data = self.data.lock().unwrap();
    data.keys.push(entry.clone());
    let snapshot = data.clone();
    drop(data);
    self.commit(&snapshot);
    tracing::info!(provider = "elevenlabs", keys = snapshot.keys.len(), "pool key added");
    Ok(
      self
        .view()
        .keys
        .into_iter()
        .find(|k| k.id == entry.id)
        .expect("just added"),
    )
  }

  /// Imports a legacy plaintext key without network validation (status Unknown).
  pub fn import_unverified(&self, secret: &str) -> Result<(), AddError> {
    let secret = secret.trim();
    if secret.is_empty() {
      return Ok(());
    }
    let id = uuid::Uuid::new_v4().to_string();
    self
      .secrets
      .set(&names::elevenlabs(&id), secret)
      .map_err(|e| AddError::Store(e.to_string()))?;
    match self.secrets.get(&names::elevenlabs(&id)) {
      Some(v) if v == secret => {}
      _ => return Err(AddError::Store("verify".into())),
    }
    let mut data = self.data.lock().unwrap();
    data.keys.push(KeyEntry {
      id,
      added_at_ms: now_ms(),
      ..Default::default()
    });
    let snapshot = data.clone();
    drop(data);
    self.commit(&snapshot);
    Ok(())
  }

  /// Moves a key to the front. In single-key mode the first key is the one used, so a newly
  /// added key replaces the active one while older keys stay saved for Multiple APIs.
  pub fn promote(&self, id: &str) {
    let mut data = self.data.lock().unwrap();
    let Some(pos) = data.keys.iter().position(|k| k.id == id) else {
      return;
    };
    let entry = data.keys.remove(pos);
    data.keys.insert(0, entry);
    let snapshot = data.clone();
    drop(data);
    self.commit(&snapshot);
  }

  /// Adds a key; in single-key mode it becomes the active (first) key.
  pub async fn add_for_mode(&self, secret: &str) -> Result<(), AddError> {
    let key = self.add(secret).await?;
    if !self.data.lock().unwrap().multi {
      self.promote(&key.id);
    }
    Ok(())
  }

  /// Adds keys found in pasted text (one or many, any `NAME=value` style). With Multiple APIs on,
  /// every key is added; otherwise only the first, which becomes the active key.
  pub async fn import_text(&self, text: &str) -> ImportReport {
    let keys = parse_keys(text);
    let multi = self.data.lock().unwrap().multi;
    let mut report = ImportReport {
      found: keys.len(),
      multi,
      ..Default::default()
    };
    let take = if multi { keys.len() } else { keys.len().min(1) };
    for key in keys.iter().take(take) {
      match self.add_for_mode(key).await {
        Ok(()) => report.added += 1,
        Err(AddError::Duplicate) => report.duplicates += 1,
        Err(AddError::Invalid) => report.rejected += 1,
        Err(AddError::Unverified) => report.unverified += 1,
        Err(AddError::Store(_)) | Err(AddError::Empty) => report.failed += 1,
      }
    }
    // In single mode a pasted key that is already saved still becomes the active one.
    if !multi && report.duplicates == 1 {
      if let Some(id) = self.id_of(&keys[0]) {
        self.promote(&id);
      }
    }
    tracing::info!(
      provider = "elevenlabs",
      found = report.found,
      added = report.added,
      duplicates = report.duplicates,
      rejected = report.rejected,
      unverified = report.unverified,
      multi,
      "pool keys imported"
    );
    report
  }

  fn id_of(&self, secret: &str) -> Option<String> {
    let ids: Vec<String> = self.data.lock().unwrap().keys.iter().map(|k| k.id.clone()).collect();
    ids
      .into_iter()
      .find(|id| self.secrets.get(&names::elevenlabs(id)).as_deref() == Some(secret))
  }

  pub fn remove(&self, id: &str) -> Result<(), String> {
    self.secrets.delete(&names::elevenlabs(id)).map_err(|e| e.to_string())?;
    let mut data = self.data.lock().unwrap();
    data.keys.retain(|k| k.id != id);
    let snapshot = data.clone();
    drop(data);
    self.commit(&snapshot);
    Ok(())
  }

  pub fn clear(&self) -> Result<(), String> {
    let ids: Vec<String> = self.data.lock().unwrap().keys.iter().map(|k| k.id.clone()).collect();
    for id in &ids {
      self.secrets.delete(&names::elevenlabs(id)).map_err(|e| e.to_string())?;
    }
    let mut data = self.data.lock().unwrap();
    data.keys.clear();
    let snapshot = data.clone();
    drop(data);
    self.commit(&snapshot);
    Ok(())
  }

  fn candidate_ids(data: &PoolData) -> Vec<String> {
    let n = if data.multi {
      data.keys.len()
    } else {
      data.keys.len().min(1)
    };
    data.keys.iter().take(n).map(|k| k.id.clone()).collect()
  }

  /// Picks a key for a session, refreshing stale keys one at a time until one is eligible.
  pub async fn acquire(&self, exclude: &[String]) -> Result<Lease, ProviderError> {
    let (candidates, empty) = {
      let data = self.data.lock().unwrap();
      (Self::candidate_ids(&data), data.keys.is_empty())
    };
    if empty {
      if let Some(secret) = self.dev_key.as_ref().and_then(|f| f()) {
        return Ok(Lease {
          key_id: "dev-env".into(),
          secret,
        });
      }
      return Err(ProviderError::MissingKey);
    }
    let candidates: Vec<String> = candidates.into_iter().filter(|id| !exclude.contains(id)).collect();
    if let Some(lease) = self.try_reserve(&candidates) {
      return Ok(lease);
    }
    // Lazy refresh: stale keys one at a time, stop at the first eligible one.
    let stale: Vec<String> = {
      let data = self.data.lock().unwrap();
      let now = now_ms();
      data
        .keys
        .iter()
        .filter(|k| candidates.contains(&k.id) && k.stale(now))
        .map(|k| k.id.clone())
        .collect()
    };
    for id in stale {
      self.refresh(&id).await;
      if let Some(lease) = self.try_reserve(&candidates) {
        return Ok(lease);
      }
    }
    let data = self.data.lock().unwrap();
    let pool: Vec<&KeyEntry> = data.keys.iter().filter(|k| candidates.contains(&k.id)).collect();
    if pool.is_empty() {
      return Err(ProviderError::QuotaExceeded);
    }
    if pool.iter().all(|k| k.status == KeyStatus::Invalid) {
      Err(ProviderError::Auth)
    } else if pool.iter().any(|k| k.status == KeyStatus::LowCredit) {
      Err(ProviderError::QuotaExceeded)
    } else {
      Err(ProviderError::RateLimited)
    }
  }

  fn try_reserve(&self, candidates: &[String]) -> Option<Lease> {
    let now = now_ms();
    let mut data = self.data.lock().unwrap();
    let pick = data
      .keys
      .iter()
      .enumerate()
      .filter(|(_, k)| candidates.contains(&k.id) && k.eligible(now, SESSION_RESERVATION))
      .min_by_key(|(i, k)| (k.last_used_ms.unwrap_or(0), *i))
      .map(|(i, _)| i)?;
    let id = data.keys[pick].id.clone();
    let secret = self.secrets.get(&names::elevenlabs(&id))?;
    let k = &mut data.keys[pick];
    k.reserved += SESSION_RESERVATION;
    k.last_used_ms = Some(now);
    let snapshot = data.clone();
    drop(data);
    self.storage.save(&snapshot);
    Some(Lease { key_id: id, secret })
  }

  /// Ends a lease. Updates status from the outcome, then refreshes credits in the background
  /// caller (see `refresh`).
  pub fn release(&self, key_id: &str, outcome: LeaseOutcome) {
    let now = now_ms();
    let mut data = self.data.lock().unwrap();
    let Some(k) = data.keys.iter_mut().find(|k| k.id == key_id) else {
      return;
    };
    k.reserved = k.reserved.saturating_sub(SESSION_RESERVATION);
    match outcome {
      LeaseOutcome::Ok => k.consecutive_failures = 0,
      LeaseOutcome::Auth => {
        k.status = KeyStatus::Invalid;
        k.last_error = Some("Rejected by ElevenLabs".into());
        k.consecutive_failures += 1;
      }
      LeaseOutcome::Quota => {
        k.status = KeyStatus::LowCredit;
        k.cooldown_until_ms = Some(now + LOW_CREDIT_COOLDOWN_MS);
        k.last_error = Some("Out of credits".into());
        k.consecutive_failures += 1;
      }
      LeaseOutcome::Transient => {
        k.status = KeyStatus::Cooldown;
        k.cooldown_until_ms = Some(now + TRANSIENT_COOLDOWN_MS);
        k.last_error = Some("Rate limited or unavailable".into());
        k.consecutive_failures += 1;
      }
      LeaseOutcome::Other => {
        k.last_error = Some("Session error".into());
        k.consecutive_failures += 1;
      }
    }
    let snapshot = data.clone();
    drop(data);
    self.commit(&snapshot);
  }

  /// Fetches credits for one key and updates its status.
  pub async fn refresh(&self, key_id: &str) {
    let Some(secret) = self.secrets.get(&names::elevenlabs(key_id)) else {
      return;
    };
    let result = tokio::time::timeout(Duration::from_secs(10), self.api.fetch(secret))
      .await
      .unwrap_or(Err(FetchError::Network));
    let now = now_ms();
    let mut data = self.data.lock().unwrap();
    let Some(k) = data.keys.iter_mut().find(|k| k.id == key_id) else {
      return;
    };
    match result {
      Ok(c) => apply_credits(k, &c, now),
      Err(FetchError::Status(400 | 401)) => {
        k.status = KeyStatus::Invalid;
        k.last_error = Some("Rejected by ElevenLabs".into());
        k.last_checked_ms = Some(now);
      }
      Err(FetchError::Status(403)) => {
        if k.status != KeyStatus::Invalid {
          k.status = KeyStatus::NoCreditAccess;
        }
        k.last_error = Some("Key cannot read credits (restricted permissions)".into());
        k.last_checked_ms = Some(now);
      }
      Err(FetchError::Status(402)) => {
        k.status = KeyStatus::LowCredit;
        k.last_checked_ms = Some(now);
      }
      Err(FetchError::Status(s)) if s == 429 || s >= 500 => {
        k.cooldown_until_ms = Some(now + TRANSIENT_COOLDOWN_MS);
        k.last_error = Some(format!("ElevenLabs HTTP {s}"));
      }
      Err(e) => {
        if k.status == KeyStatus::Unknown {
          k.status = KeyStatus::Error;
        }
        k.last_error = Some(match e {
          FetchError::Network => "Could not reach ElevenLabs".into(),
          FetchError::Malformed => "Unexpected response".into(),
          FetchError::Status(s) => format!("ElevenLabs HTTP {s}"),
        });
      }
    }
    let snapshot = data.clone();
    drop(data);
    self.commit(&snapshot);
  }

  /// Refreshes every key, three at a time.
  pub async fn refresh_all(&self) {
    let ids: Vec<String> = self.data.lock().unwrap().keys.iter().map(|k| k.id.clone()).collect();
    for chunk in ids.chunks(3) {
      futures_util::future::join_all(chunk.iter().map(|id| self.refresh(id))).await;
    }
  }
}

fn apply_credits(k: &mut KeyEntry, c: &Credits, now: u64) {
  k.character_count = Some(c.character_count);
  k.character_limit = Some(c.character_limit);
  k.last_checked_ms = Some(now);
  k.cooldown_until_ms = None;
  let remaining = c.character_limit.saturating_sub(c.character_count);
  k.status = if remaining <= CREDIT_BUFFER {
    KeyStatus::LowCredit
  } else {
    KeyStatus::Usable
  };
  if k.status == KeyStatus::Usable {
    k.consecutive_failures = 0;
    k.last_error = None;
  }
}

/// Real ElevenLabs subscription endpoint.
pub struct HttpCreditsApi;

impl CreditsApi for HttpCreditsApi {
  fn fetch(&self, secret: String) -> BoxFuture<Result<Credits, FetchError>> {
    Box::pin(async move {
      let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|_| FetchError::Network)?;
      let resp = client
        .get("https://api.elevenlabs.io/v1/user/subscription")
        .header("xi-api-key", secret)
        .send()
        .await
        .map_err(|_| FetchError::Network)?;
      let status = resp.status().as_u16();
      if !(200..300).contains(&status) {
        return Err(FetchError::Status(status));
      }
      let v: serde_json::Value = resp.json().await.map_err(|_| FetchError::Malformed)?;
      Ok(Credits {
        character_count: v["character_count"].as_u64().ok_or(FetchError::Malformed)?,
        character_limit: v["character_limit"].as_u64().ok_or(FetchError::Malformed)?,
      })
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::credentials::MemorySecretStore;
  use std::collections::HashMap;

  #[derive(Default)]
  struct MemStorage(Mutex<PoolData>);
  impl PoolStorage for MemStorage {
    fn load(&self) -> PoolData {
      self.0.lock().unwrap().clone()
    }
    fn save(&self, d: &PoolData) {
      *self.0.lock().unwrap() = d.clone();
    }
  }

  /// Credits per secret; secrets not listed return 401.
  #[derive(Default)]
  struct FakeApi(Mutex<HashMap<String, Result<Credits, FetchError>>>);
  impl FakeApi {
    fn set(&self, secret: &str, r: Result<Credits, FetchError>) {
      self.0.lock().unwrap().insert(secret.into(), r);
    }
  }
  impl CreditsApi for FakeApi {
    fn fetch(&self, secret: String) -> BoxFuture<Result<Credits, FetchError>> {
      let r = self
        .0
        .lock()
        .unwrap()
        .get(&secret)
        .cloned()
        .unwrap_or(Err(FetchError::Status(401)));
      Box::pin(async move { r })
    }
  }

  fn credits(count: u64, limit: u64) -> Result<Credits, FetchError> {
    Ok(Credits {
      character_count: count,
      character_limit: limit,
    })
  }

  fn pool(api: Arc<FakeApi>) -> ElevenLabsPool {
    ElevenLabsPool::new(
      Arc::new(MemorySecretStore::default()),
      Arc::new(MemStorage::default()),
      api,
    )
  }

  fn run<F: Future>(f: F) -> F::Output {
    tauri::async_runtime::block_on(f)
  }

  #[test]
  fn add_validates_rejects_invalid_and_duplicates() {
    let api = Arc::new(FakeApi::default());
    api.set("good", credits(100, 10_000));
    let p = pool(api.clone());
    run(async {
      assert_eq!(p.add("  ").await, Err(AddError::Empty));
      assert_eq!(p.add("bad").await, Err(AddError::Invalid));
      let k = p.add("good").await.unwrap();
      assert_eq!(k.label, "Key 1");
      assert_eq!(k.remaining, Some(9_900));
      assert_eq!(p.add(" good ").await, Err(AddError::Duplicate));
    });
    assert_eq!(p.view().keys.len(), 1);
  }

  #[test]
  fn parse_keys_from_env_style_paste() {
    let k = |n: u8| format!("sk_{}", format!("{n:02}").repeat(16));
    let text = format!(
      "ELEVENLABS_API_KEY_1={}\r\nELEVENLABS_API_KEY_2 = \"{}\"\nexport OTHER_NAME='{}'\nkey3: {}, {}\n\n# comment\nELEVENLABS_API_KEY_1={}",
      k(1), k(2), k(3), k(4), k(5), k(1)
    );
    assert_eq!(
      parse_keys(&text),
      vec![k(1), k(2), k(3), k(4), k(5)],
      "names ignored, order kept, duplicate dropped"
    );
    let hex = "0123456789abcdef0123456789abcdef";
    assert_eq!(
      parse_keys(&format!("XI_KEY={hex}")),
      vec![hex.to_string()],
      "legacy hex keys"
    );
    assert_eq!(
      parse_keys("  some-odd-key  "),
      vec!["some-odd-key".to_string()],
      "single bare token"
    );
    assert!(parse_keys("ELEVENLABS_API_KEY=").is_empty());
    assert!(parse_keys("not a key at all").is_empty());
  }

  #[test]
  fn import_adds_all_in_multi_and_first_in_single() {
    let api = Arc::new(FakeApi::default());
    let keys: Vec<String> = (1..=4)
      .map(|n| format!("sk_{}", format!("{n:02}").repeat(16)))
      .collect();
    for key in &keys {
      api.set(key, credits(0, 10_000));
    }
    api.set(&format!("sk_{}", "99".repeat(16)), Err(FetchError::Status(400)));
    let text: String = keys
      .iter()
      .enumerate()
      .map(|(i, key)| format!("ELEVENLABS_API_KEY_{}={key}\n", i + 1))
      .collect();
    let p = pool(api.clone());

    // Single mode: only the first key, and it becomes the active one.
    run(p.add(&format!("sk_{}", "50".repeat(16)))).ok();
    let r = run(p.import_text(&text));
    assert_eq!((r.found, r.added, r.multi), (4, 1, false));
    assert_eq!(p.view().keys.len(), 1, "the unverifiable key was not stored");
    assert_eq!(
      p.id_of(&keys[0]),
      Some(p.view().keys[0].id.clone()),
      "first pasted key is active"
    );

    // Multi mode: the rest are added; the already saved first key counts as a duplicate.
    p.set_multi(true);
    let bad = format!("\nBAD=sk_{}", "99".repeat(16));
    let r = run(p.import_text(&(text.clone() + &bad)));
    assert_eq!(
      (r.found, r.added, r.duplicates, r.rejected, r.multi),
      (5, 3, 1, 1, true)
    );
    assert_eq!(p.view().keys.len(), 4);
  }

  #[test]
  fn single_mode_add_replaces_the_active_key() {
    let api = Arc::new(FakeApi::default());
    api.set("old", credits(0, 10_000));
    api.set("new", credits(5_000, 10_000));
    let p = pool(api);
    run(async {
      p.add_for_mode("old").await.unwrap();
      p.add_for_mode("new").await.unwrap();
    });
    let v = p.view();
    assert_eq!(v.keys.len(), 2, "the old key stays saved");
    assert_eq!(v.total_remaining, 5_000, "single mode now uses the new key");
    // A pasted key that is already saved becomes active again.
    run(p.import_text("old"));
    assert_eq!(p.view().total_remaining, 10_000);
  }

  #[test]
  fn network_failure_does_not_store_key() {
    let api = Arc::new(FakeApi::default());
    api.set("k", Err(FetchError::Network));
    let p = pool(api);
    assert_eq!(run(p.add("k")), Err(AddError::Unverified));
    assert!(p.view().keys.is_empty());
  }

  #[test]
  fn restricted_key_is_accepted_without_credits() {
    let api = Arc::new(FakeApi::default());
    api.set("r", Err(FetchError::Status(403)));
    let p = pool(api);
    let k = run(p.add("r")).unwrap();
    assert_eq!(k.status, KeyStatus::NoCreditAccess);
    assert!(run(p.acquire(&[])).is_ok(), "restricted keys can still transcribe");
  }

  #[test]
  fn totals_sum_active_keys_only() {
    let api = Arc::new(FakeApi::default());
    api.set("a", credits(9_800, 20_000));
    api.set("b", credits(0, 10_000));
    let p = pool(api);
    run(async {
      p.add("a").await.unwrap();
      p.add("b").await.unwrap();
    });
    let v = p.view();
    assert!(!v.multi);
    assert_eq!(
      (v.total_remaining, v.total_limit),
      (10_200, 20_000),
      "single mode counts Key 1 only"
    );
    p.set_multi(true);
    let v = p.view();
    assert_eq!((v.total_remaining, v.total_limit), (20_200, 30_000));
  }

  #[test]
  fn acquire_uses_lru_and_skips_low_credit_keys() {
    let api = Arc::new(FakeApi::default());
    api.set("a", credits(0, 10_000));
    api.set("b", credits(0, 10_000));
    api.set("low", credits(9_700, 10_000)); // 300 left, under the 500 buffer
    let p = pool(api);
    run(async {
      for s in ["a", "b", "low"] {
        p.add(s).await.unwrap();
      }
    });
    p.set_multi(true);
    let ids: Vec<String> = p.view().keys.iter().map(|k| k.id.clone()).collect();
    let first = run(p.acquire(&[])).unwrap();
    std::thread::sleep(Duration::from_millis(3));
    let second = run(p.acquire(&[])).unwrap();
    assert_ne!(first.key_id, second.key_id, "least recently used rotates");
    assert!(
      first.key_id != ids[2] && second.key_id != ids[2],
      "low credit key never chosen"
    );
  }

  #[test]
  fn single_mode_uses_only_first_key() {
    let api = Arc::new(FakeApi::default());
    api.set("a", credits(0, 10_000));
    api.set("b", credits(0, 10_000));
    let p = pool(api);
    run(async {
      p.add("a").await.unwrap();
      p.add("b").await.unwrap();
    });
    let first_id = p.view().keys[0].id.clone();
    for _ in 0..3 {
      assert_eq!(run(p.acquire(&[])).unwrap().key_id, first_id);
    }
  }

  #[test]
  fn failures_update_status_and_exclusion_moves_on() {
    let api = Arc::new(FakeApi::default());
    api.set("a", credits(0, 10_000));
    api.set("b", credits(0, 10_000));
    let p = pool(api);
    run(async {
      p.add("a").await.unwrap();
      p.add("b").await.unwrap();
    });
    p.set_multi(true);
    let lease = run(p.acquire(&[])).unwrap();
    p.release(&lease.key_id, LeaseOutcome::Quota);
    let k = p.view().keys.into_iter().find(|k| k.id == lease.key_id).unwrap();
    assert_eq!(k.status, KeyStatus::LowCredit);
    let next = run(p.acquire(std::slice::from_ref(&lease.key_id))).unwrap();
    assert_ne!(next.key_id, lease.key_id);
    p.release(&next.key_id, LeaseOutcome::Auth);
    // Both unusable now: refresh of stale keys is skipped because they are fresh.
    assert_eq!(run(p.acquire(&[])), Err(ProviderError::QuotaExceeded));
  }

  #[test]
  fn refresh_marks_revoked_key_invalid() {
    let api = Arc::new(FakeApi::default());
    api.set("a", credits(0, 10_000));
    let p = pool(api.clone());
    run(p.add("a")).unwrap();
    api.set("a", Err(FetchError::Status(401)));
    let id = p.view().keys[0].id.clone();
    run(p.refresh(&id));
    assert_eq!(p.view().keys[0].status, KeyStatus::Invalid);
    assert_eq!(run(p.acquire(&[])), Err(ProviderError::Auth));
  }

  #[test]
  fn remove_and_clear_delete_secrets() {
    let api = Arc::new(FakeApi::default());
    api.set("a", credits(0, 10_000));
    api.set("b", credits(0, 10_000));
    let secrets = Arc::new(MemorySecretStore::default());
    let p = ElevenLabsPool::new(secrets.clone(), Arc::new(MemStorage::default()), api);
    run(async {
      p.add("a").await.unwrap();
      p.add("b").await.unwrap();
    });
    let ids: Vec<String> = p.view().keys.iter().map(|k| k.id.clone()).collect();
    p.remove(&ids[0]).unwrap();
    assert!(secrets.get(&names::elevenlabs(&ids[0])).is_none());
    assert_eq!(p.view().keys.len(), 1);
    assert_eq!(p.view().keys[0].label, "Key 1", "labels renumber");
    p.clear().unwrap();
    assert!(secrets.get(&names::elevenlabs(&ids[1])).is_none());
    assert!(!p.view().configured);
  }

  #[test]
  fn view_never_contains_secrets() {
    let api = Arc::new(FakeApi::default());
    api.set("super-secret-value", credits(0, 10_000));
    let p = pool(api);
    run(p.add("super-secret-value")).unwrap();
    let json = serde_json::to_string(&p.view()).unwrap();
    assert!(!json.contains("super-secret-value"));
  }

  #[test]
  fn dev_key_used_only_when_pool_empty() {
    let api = Arc::new(FakeApi::default());
    let p = pool(api).with_dev_key(Arc::new(|| Some("env-key".into())));
    let lease = run(p.acquire(&[])).unwrap();
    assert_eq!(lease.key_id, "dev-env");
    assert!(p.view().using_dev_key);
  }
}
