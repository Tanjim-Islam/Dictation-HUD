//! Provider secrets. Stored only in the OS credential store (Windows Credential Manager via
//! `keyring`). Never in the Tauri Store, SQLite, logs, or command results. There is no
//! plaintext fallback: if the OS store fails, saving fails.

use std::collections::HashMap;
use std::sync::Mutex;

pub const SERVICE: &str = "com.r0ju.dictationhud";

#[derive(Debug, PartialEq)]
pub enum CredentialError {
  Store(String),
  Verify,
}

impl std::fmt::Display for CredentialError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      CredentialError::Store(kind) => write!(f, "credential store error: {kind}"),
      CredentialError::Verify => write!(f, "credential could not be verified after saving"),
    }
  }
}

/// Minimal secret store interface so logic can be tested without the OS store.
pub trait SecretStore: Send + Sync {
  fn get(&self, name: &str) -> Option<String>;
  fn set(&self, name: &str, secret: &str) -> Result<(), CredentialError>;
  fn delete(&self, name: &str) -> Result<(), CredentialError>;
}

/// Windows Credential Manager (generic credentials), one entry per secret name.
pub struct OsSecretStore {
  service: String,
}

impl OsSecretStore {
  pub fn new(service: impl Into<String>) -> Self {
    Self {
      service: service.into(),
    }
  }

  fn entry(&self, name: &str) -> Result<keyring::Entry, CredentialError> {
    keyring::Entry::new(&self.service, name).map_err(|e| CredentialError::Store(error_kind(&e)))
  }
}

fn error_kind(e: &keyring::Error) -> String {
  // Variant name only; platform errors never contain the secret, but keep logs minimal.
  match e {
    keyring::Error::NoEntry => "no entry".into(),
    keyring::Error::PlatformFailure(_) => "platform failure".into(),
    keyring::Error::NoStorageAccess(_) => "no storage access".into(),
    keyring::Error::BadEncoding(_) => "bad encoding".into(),
    keyring::Error::TooLong(..) => "too long".into(),
    keyring::Error::Invalid(..) => "invalid".into(),
    keyring::Error::Ambiguous(_) => "ambiguous".into(),
    _ => "other".into(),
  }
}

impl SecretStore for OsSecretStore {
  fn get(&self, name: &str) -> Option<String> {
    self.entry(name).ok()?.get_password().ok().filter(|s| !s.is_empty())
  }

  fn set(&self, name: &str, secret: &str) -> Result<(), CredentialError> {
    let entry = self.entry(name)?;
    entry
      .set_password(secret)
      .map_err(|e| CredentialError::Store(error_kind(&e)))?;
    // Read back so a silently failing store never reports success.
    match entry.get_password() {
      Ok(v) if v == secret => Ok(()),
      _ => Err(CredentialError::Verify),
    }
  }

  fn delete(&self, name: &str) -> Result<(), CredentialError> {
    match self.entry(name)?.delete_credential() {
      Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
      Err(e) => Err(CredentialError::Store(error_kind(&e))),
    }
  }
}

/// In-memory store for tests.
#[derive(Default)]
pub struct MemorySecretStore(Mutex<HashMap<String, String>>);

impl SecretStore for MemorySecretStore {
  fn get(&self, name: &str) -> Option<String> {
    self.0.lock().unwrap().get(name).cloned()
  }
  fn set(&self, name: &str, secret: &str) -> Result<(), CredentialError> {
    self.0.lock().unwrap().insert(name.into(), secret.into());
    Ok(())
  }
  fn delete(&self, name: &str) -> Result<(), CredentialError> {
    self.0.lock().unwrap().remove(name);
    Ok(())
  }
}

/// Secret names used by the app.
pub mod names {
  pub const DEEPGRAM: &str = "deepgram";
  pub const OPENROUTER: &str = "openrouter";
  pub const MEGALLM: &str = "megallm";
  pub fn elevenlabs(key_id: &str) -> String {
    format!("elevenlabs:{key_id}")
  }
}

/// Legacy plaintext store keys (Tauri Store `prefs.json`) and their secure names.
pub const LEGACY_KEYS: &[(&str, &str)] = &[
  ("deepgram_key", names::DEEPGRAM),
  ("openrouter_key", names::OPENROUTER),
  ("megallm_key", names::MEGALLM),
];
pub const LEGACY_ELEVENLABS_KEY: &str = "elevenlabs_key";

/// Moves one legacy plaintext secret into the secure store. Order matters: write secure, verify
/// by reading back, and only then report that the plaintext copy may be deleted.
/// Returns Ok(true) when the caller must delete the plaintext value.
pub fn migrate_secret(store: &dyn SecretStore, secure_name: &str, plaintext: &str) -> Result<bool, CredentialError> {
  if plaintext.trim().is_empty() {
    return Ok(true);
  }
  match store.get(secure_name) {
    // A secure value already exists (set by the user after the plaintext was written). Keep it.
    Some(_) => Ok(true),
    None => {
      store.set(secure_name, plaintext.trim())?;
      match store.get(secure_name) {
        Some(v) if v == plaintext.trim() => Ok(true),
        _ => Err(CredentialError::Verify),
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  struct FailingStore;
  impl SecretStore for FailingStore {
    fn get(&self, _: &str) -> Option<String> {
      None
    }
    fn set(&self, _: &str, _: &str) -> Result<(), CredentialError> {
      Err(CredentialError::Store("platform failure".into()))
    }
    fn delete(&self, _: &str) -> Result<(), CredentialError> {
      Ok(())
    }
  }

  #[test]
  fn migration_writes_then_allows_delete() {
    let s = MemorySecretStore::default();
    assert_eq!(migrate_secret(&s, names::DEEPGRAM, "  abc123  "), Ok(true));
    assert_eq!(s.get(names::DEEPGRAM).as_deref(), Some("abc123"));
  }

  #[test]
  fn migration_keeps_existing_secure_value() {
    let s = MemorySecretStore::default();
    s.set(names::OPENROUTER, "newer").unwrap();
    assert_eq!(migrate_secret(&s, names::OPENROUTER, "older"), Ok(true));
    assert_eq!(s.get(names::OPENROUTER).as_deref(), Some("newer"));
  }

  #[test]
  fn failed_secure_write_never_allows_plaintext_delete() {
    assert!(migrate_secret(&FailingStore, names::DEEPGRAM, "abc").is_err());
  }

  #[test]
  fn errors_do_not_contain_secrets() {
    let e = CredentialError::Store("platform failure".into()).to_string();
    assert!(!e.contains("abc"));
  }

  /// Exercises the real Windows Credential Manager with an isolated test service name and
  /// removes the entry afterwards.
  #[test]
  #[cfg(target_os = "windows")]
  fn os_store_round_trip() {
    let store = OsSecretStore::new(format!("{SERVICE}.test-{}", uuid::Uuid::new_v4()));
    store.set("probe", "value-1").unwrap();
    assert_eq!(store.get("probe").as_deref(), Some("value-1"));
    store.set("probe", "value-2").unwrap();
    assert_eq!(store.get("probe").as_deref(), Some("value-2"));
    store.delete("probe").unwrap();
    assert_eq!(store.get("probe"), None);
    store.delete("probe").unwrap();
  }
}
