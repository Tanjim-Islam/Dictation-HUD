//! LocalAdapter through the same replay path History uses, with an in-process fake helper.
//! Run with `--features test-helpers`.
#![cfg(feature = "test-helpers")]

use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Duration;

use dictation_hud::local_asr::client::testing::{Fake, FakeLauncher};
use dictation_hud::local_asr::client::{Helper, Limits};
use dictation_hud::providers::local::{LocalAdapter, ModelLocator};
use dictation_hud::providers::{replay, ProviderError, SttAdapter};
use tokio_util::sync::CancellationToken;

struct Locator {
  installed: bool,
  memory: bool,
}

impl ModelLocator for Locator {
  fn model_dir(&self, _id: &str) -> Option<PathBuf> {
    self.installed.then(|| PathBuf::from("m"))
  }
  fn vad_path(&self) -> Option<PathBuf> {
    Some(PathBuf::from("v"))
  }
  fn enough_memory(&self, _bytes: u64) -> bool {
    self.memory
  }
}

fn adapter(mode: Fake, installed: bool, memory: bool) -> Arc<dyn SttAdapter> {
  let limits = Limits {
    flush: Duration::from_millis(300),
    ..Limits::default()
  };
  let helper = Helper::new(
    Arc::new(FakeLauncher {
      mode,
      launches: Arc::new(AtomicUsize::new(0)),
    }),
    limits,
  );
  Arc::new(LocalAdapter::new(helper, Arc::new(Locator { installed, memory })))
}

async fn run(a: Arc<dyn SttAdapter>, lang: &str) -> Result<String, ProviderError> {
  let options = a
    .descriptor()
    .resolve("parakeet-v3", lang, vec!["City Bank".into()])
    .expect("options");
  let pace = replay::Pace {
    speed: 0,
    finalize_timeout: Duration::from_secs(5),
  };
  replay::transcribe(a, options, &vec![0i16; 16_000], pace, CancellationToken::new()).await
}

#[tokio::test]
async fn transcribes_through_the_helper() {
  assert_eq!(
    run(adapter(Fake::Normal, true, true), "en").await.unwrap(),
    "hello hello hello world"
  );
}

#[tokio::test]
async fn crash_fails_without_any_cloud_fallback() {
  let err = run(adapter(Fake::CrashOnAudio, true, true), "en").await.unwrap_err();
  assert_eq!(err, ProviderError::Protocol("local engine stopped".into()));
}

#[tokio::test]
async fn missing_model_and_low_memory_fail_fast() {
  assert_eq!(
    run(adapter(Fake::Normal, false, true), "en").await.unwrap_err(),
    ProviderError::Protocol("model not installed".into())
  );
  assert_eq!(
    run(adapter(Fake::Normal, true, false), "en").await.unwrap_err(),
    ProviderError::Protocol("not enough memory for this model".into())
  );
}

#[test]
fn descriptor_lists_the_catalog_without_keyterms() {
  let d = adapter(Fake::Normal, true, true).descriptor();
  assert_eq!(d.id, "local");
  assert_eq!(d.default_model, "parakeet-v3");
  assert!(d.models.iter().all(|m| !m.keyterms));
  assert!(d.resolve("parakeet-v2", "bn", vec![]).is_err());
}
