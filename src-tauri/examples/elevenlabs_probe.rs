//! Live diagnostic: <16k mono wav> <ordered pool ids...> [--compare <good pool id>].
//! Only status, timing, and text shape are printed. Credentials and transcripts stay in memory.
//! Add --force-order to deliberately ignore saved quarantine/proof in this isolated test pool.
use std::sync::{Arc, Mutex};
use std::time::Instant;

use dictation_hud::credentials::{names, MemorySecretStore, OsSecretStore, SecretStore, SERVICE};
use dictation_hud::providers::elevenlabs::{self, ElevenLabs};
use dictation_hud::providers::elevenlabs_pool::{
  now_ms, ElevenLabsPool, HttpCreditsApi, KeyEntry, KeyStatus, PoolData, PoolStorage,
};
use dictation_hud::providers::{replay, ProviderError, SttAdapter};
use tokio_util::sync::CancellationToken;

struct MemStorage(Mutex<PoolData>);
impl PoolStorage for MemStorage {
  fn load(&self) -> PoolData {
    self.0.lock().unwrap().clone()
  }
  fn save(&self, d: &PoolData) {
    *self.0.lock().unwrap() = d.clone();
  }
}

async fn transcribe(pcm: &[i16], ids: &[String]) -> Result<String, ProviderError> {
  let saved: PoolData = std::env::var_os("APPDATA")
    .and_then(|root| std::fs::read(std::path::Path::new(&root).join("com.r0ju.dictationhud/prefs.json")).ok())
    .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
    .and_then(|v| v.get("elevenlabs_pool").cloned())
    .and_then(|v| serde_json::from_value(v).ok())
    .unwrap_or_default();
  let force_order = std::env::args().any(|a| a == "--force-order");
  let os = OsSecretStore::new(SERVICE);
  let secrets = Arc::new(MemorySecretStore::default());
  let mut data = PoolData {
    multi: ids.len() > 1,
    keys: Vec::new(),
  };
  for id in ids {
    let secret = os.get(&names::elevenlabs(id)).expect("saved credential");
    secrets
      .set(&names::elevenlabs(id), &secret)
      .expect("in-memory credential");
    let entry = KeyEntry {
      id: id.clone(),
      status: KeyStatus::Usable,
      character_count: Some(0),
      character_limit: Some(10_000),
      last_checked_ms: Some(now_ms()),
      ..Default::default()
    };
    data.keys.push(if force_order {
      entry
    } else {
      saved.keys.iter().find(|k| k.id == *id).cloned().unwrap_or(entry)
    });
  }
  let pool = Arc::new(ElevenLabsPool::new(
    secrets,
    Arc::new(MemStorage(Mutex::new(data))),
    Arc::new(HttpCreditsApi),
  ));
  let adapter: Arc<dyn SttAdapter> = Arc::new(ElevenLabs::new(pool));
  let options = adapter
    .descriptor()
    .resolve("scribe_v2_realtime", "multi", Vec::new())
    .unwrap();
  replay::transcribe(adapter, options, pcm, replay::Pace::default(), CancellationToken::new()).await
}

#[tokio::main]
async fn main() {
  let args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--force-order").collect();
  assert!(args.len() >= 2, "expected wav and ordered pool ids");
  if args[0] == "--probe-only" {
    let os = OsSecretStore::new(SERVICE);
    for id in &args[1..] {
      let start = Instant::now();
      let result = elevenlabs::probe_access(os.get(&names::elevenlabs(id)).expect("saved credential")).await;
      println!("probe key={id} result={result:?} ms={}", start.elapsed().as_millis());
    }
    return;
  }
  let pcm = dictation_hud::audio::capture::load_wav_16k(std::path::Path::new(&args[0])).expect("wav");
  let split = args.iter().position(|s| s == "--compare").unwrap_or(args.len());
  let start = Instant::now();
  let result = transcribe(&pcm, &args[1..split]).await;
  match &result {
    Ok(text) => println!(
      "completed keys={} ms={} text={}",
      split - 1,
      start.elapsed().as_millis(),
      dictation_hud::logging::text_shape(text)
    ),
    Err(e) => println!("failed={e:?} ms={}", start.elapsed().as_millis()),
  }
  if let Some(id) = args.get(split + 1) {
    let baseline = transcribe(&pcm, std::slice::from_ref(id)).await;
    println!(
      "baseline_success={} exact_match={}",
      baseline.is_ok(),
      result
        .as_ref()
        .ok()
        .zip(baseline.as_ref().ok())
        .is_some_and(|(a, b)| a == b)
    );
    if let (Ok(a), Ok(b)) = (&result, &baseline) {
      println!("same_words={}", a.split_whitespace().eq(b.split_whitespace()));
      // Word-level difference without printing any text: words only in one of the two results.
      let norm = |t: &str| -> Vec<String> {
        t.split_whitespace()
          .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
          .filter(|w| !w.is_empty())
          .collect()
      };
      let (x, y) = (norm(a), norm(b));
      let mut lcs = vec![vec![0u32; y.len() + 1]; x.len() + 1];
      for i in (0..x.len()).rev() {
        for j in (0..y.len()).rev() {
          lcs[i][j] = if x[i] == y[j] {
            lcs[i + 1][j + 1] + 1
          } else {
            lcs[i + 1][j].max(lcs[i][j + 1])
          };
        }
      }
      let common = lcs[0][0] as usize;
      println!(
        "baseline_words={} result_words={} common={} only_in_result={} only_in_baseline={}",
        y.len(),
        x.len(),
        common,
        x.len() - common,
        y.len() - common
      );
    }
  }
}
