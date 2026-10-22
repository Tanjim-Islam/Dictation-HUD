//! Scripted protocol tests. Samples encode words, so replay equality is checked independently
//! of transcript aggregation. All credentials and text in this file are synthetic.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use dictation_hud::credentials::MemorySecretStore;
use dictation_hud::providers::elevenlabs::{ElevenLabs, Timing};
use dictation_hud::providers::elevenlabs_pool::{
  BoxFuture, Credits, CreditsApi, ElevenLabsPool, FetchError, KeyStatus, PoolData, PoolStorage,
};
use dictation_hud::providers::{AudioMsg, ProviderError, StreamCtx, SttAdapter, SttEvent, SttOptions};

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
struct Credit;
impl CreditsApi for Credit {
  fn fetch(&self, _: String) -> BoxFuture<Result<Credits, FetchError>> {
    Box::pin(async {
      Ok(Credits {
        character_count: 0,
        character_limit: 100_000,
      })
    })
  }
}
#[derive(Clone, Default)]
struct Script {
  upgrade: Option<u16>,
  handshake_error: Option<&'static str>,
  start_delay: Duration,
  fail_after: usize,
  error: Option<&'static str>,
  reset: bool,
  fail_connections: usize,
  failure_delay: Duration,
  ack_delay: Duration,
  hang_commit: bool,
  no_speech_on_commit: bool,
}
type Received = Arc<Mutex<HashMap<String, Vec<Vec<i16>>>>>;
struct Server {
  url: String,
  received: Received,
  task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
  fn drop(&mut self) {
    self.task.abort();
  }
}

async fn server(scripts: HashMap<String, Script>) -> Server {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let addr = listener.local_addr().unwrap();
  let received: Received = Arc::default();
  let rec = received.clone();
  let scripts = Arc::new(scripts);
  let task = tokio::spawn(async move {
    while let Ok((tcp, _)) = listener.accept().await {
      let (rec, scripts) = (rec.clone(), scripts.clone());
      tokio::spawn(async move {
        let key = Arc::new(Mutex::new(String::new()));
        let saved_key = key.clone();
        let ss = scripts.clone();
        #[allow(clippy::result_large_err)]
        let cb = move |req: &Request, resp: Response| {
          assert!(req.uri().query().unwrap().contains("commit_strategy=manual"));
          let id = req.headers()["xi-api-key"].to_str().unwrap().to_string();
          *saved_key.lock().unwrap() = id.clone();
          if let Some(status) = ss.get(&id).and_then(|s| s.upgrade) {
            return Err(
              tokio_tungstenite::tungstenite::http::Response::builder()
                .status(status)
                .body(Some("scripted rejection".into()))
                .unwrap(),
            );
          }
          Ok(resp)
        };
        let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(tcp, cb).await else {
          return;
        };
        let key = key.lock().unwrap().clone();
        let script = scripts.get(&key).cloned().unwrap_or_default();
        let connection = {
          let mut rec = rec.lock().unwrap();
          let entries = rec.entry(key.clone()).or_default();
          entries.push(Vec::new());
          entries.len() - 1
        };
        tokio::time::sleep(script.start_delay).await;
        let initial =
          serde_json::json!({"message_type":script.handshake_error.unwrap_or("session_started")}).to_string();
        if ws.send(Message::Text(initial.into())).await.is_err() {
          return;
        }
        if script.handshake_error.is_some() {
          return;
        }
        let mut segment = Vec::new();
        let mut count = 0;
        while let Some(Ok(Message::Text(t))) = ws.next().await {
          let v: serde_json::Value = serde_json::from_str(&t).unwrap();
          let bytes = base64::engine::general_purpose::STANDARD
            .decode(v["audio_base_64"].as_str().unwrap())
            .unwrap();
          let pcm: Vec<i16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect();
          rec.lock().unwrap().get_mut(&key).unwrap()[connection].extend_from_slice(&pcm);
          segment.extend_from_slice(&pcm);
          count += pcm.len();
          if script.fail_after > 0 && count >= script.fail_after && connection < script.fail_connections {
            tokio::time::sleep(script.failure_delay).await;
            if let Some(error) = script.error {
              let _ = ws
                .send(Message::Text(
                  serde_json::json!({"message_type":error}).to_string().into(),
                ))
                .await;
              let _ = ws.close(None).await;
              // Drain already queued audio so a TCP reset cannot erase the rejection frame.
              let _ = tokio::time::timeout(Duration::from_secs(1), async {
                while let Some(Ok(_)) = ws.next().await {}
              })
              .await;
            } else if !script.reset {
              let _ = ws.close(None).await;
            }
            return;
          }
          if !pcm.is_empty() {
            let _ = ws
              .send(Message::Text(
                serde_json::json!({"message_type":"partial_transcript","text":words(&segment)})
                  .to_string()
                  .into(),
              ))
              .await;
          }
          if v["commit"] == true && script.no_speech_on_commit {
            let _ = ws
              .send(Message::Text(
                serde_json::json!({"message_type":"insufficient_audio_activity"})
                  .to_string()
                  .into(),
              ))
              .await;
            segment.clear();
            continue;
          }
          if v["commit"] == true && !script.hang_commit {
            tokio::time::sleep(script.ack_delay).await;
            let _ = ws
              .send(Message::Text(
                serde_json::json!({"message_type":"committed_transcript","text":words(&segment)})
                  .to_string()
                  .into(),
              ))
              .await;
            segment.clear();
          }
        }
      });
    }
  });
  Server {
    url: format!("ws://{addr}/v1/speech-to-text/realtime"),
    received,
    task,
  }
}
fn words(pcm: &[i16]) -> String {
  pcm
    .chunks(1600)
    .filter_map(|c| c.first())
    .map(|n| format!("w{n}"))
    .collect::<Vec<_>>()
    .join(" ")
}
fn audio(chunks: usize) -> Vec<i16> {
  (1..=chunks).flat_map(|i| vec![i as i16; 1600]).collect()
}
fn timing() -> Timing {
  Timing {
    failover: Duration::from_secs(4),
    connect: Duration::from_secs(1),
    acknowledgment: Duration::from_secs(1),
    chunk_interval: Duration::from_millis(2),
    checkpoint_samples: 20 * 16_000,
    network_backoff: Duration::from_millis(10),
  }
}
struct Running {
  pool: Arc<ElevenLabsPool>,
  storage: Arc<MemStorage>,
  audio: mpsc::Sender<AudioMsg>,
  events: mpsc::Receiver<(uuid::Uuid, SttEvent)>,
  cancel: CancellationToken,
  id: uuid::Uuid,
}
async fn start(server: &Server, keys: &[&str], timing: Timing) -> Running {
  let storage = Arc::new(MemStorage::default());
  let pool = Arc::new(ElevenLabsPool::new(
    Arc::new(MemorySecretStore::default()),
    storage.clone(),
    Arc::new(Credit),
  ));
  for key in keys {
    pool.add(key).await.unwrap();
  }
  pool.set_multi(true);
  let (audio_tx, audio) = mpsc::channel(256);
  let (events, ev_rx) = mpsc::channel(256);
  let id = uuid::Uuid::new_v4();
  let cancel = CancellationToken::new();
  ElevenLabs::with_base_url(pool.clone(), &server.url)
    .with_timing(timing)
    .spawn(StreamCtx {
      session_id: id,
      options: SttOptions {
        model: "scribe_v2_realtime".into(),
        language: "en".into(),
        keyterms: vec![],
      },
      audio,
      events,
      cancel: cancel.clone(),
    });
  Running {
    pool,
    storage,
    audio: audio_tx,
    events: ev_rx,
    cancel,
    id,
  }
}
impl Running {
  async fn feed(&self, pcm: &[i16], finish: bool) {
    for chunk in pcm.chunks(1600) {
      self.audio.send(AudioMsg::Pcm(chunk.to_vec())).await.unwrap();
    }
    if finish {
      self.audio.send(AudioMsg::Finish).await.unwrap();
    }
  }
  async fn collect(&mut self) -> Vec<SttEvent> {
    let mut events = Vec::new();
    loop {
      let (sid, ev) = tokio::time::timeout(Duration::from_secs(45), self.events.recv())
        .await
        .unwrap()
        .expect("terminal event");
      assert_eq!(sid, self.id);
      let done = matches!(ev, SttEvent::Finalized | SttEvent::Failed(_));
      events.push(ev);
      if done {
        break;
      }
    }
    // Reservations are dropped as the terminal event is sent.
    for _ in 0..100 {
      if !self.pool.session_active() {
        break;
      }
      tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert!(self.storage.load().keys.iter().all(|k| k.reserved == 0));
    events
  }
}
impl Drop for Running {
  fn drop(&mut self) {
    self.cancel.cancel();
  }
}
fn text(events: &[SttEvent]) -> String {
  events
    .iter()
    .filter_map(|e| {
      if let SttEvent::Final(t) = e {
        Some(t.as_str())
      } else {
        None
      }
    })
    .collect::<Vec<_>>()
    .join(" ")
}
fn refused(n: usize) -> Script {
  Script {
    fail_after: n,
    error: Some("quota_exceeded"),
    fail_connections: usize::MAX,
    ..Default::default()
  }
}

#[tokio::test]
async fn handshake_auth_and_http_rejections_send_no_audio() {
  for (script, status) in [
    (
      Script {
        handshake_error: Some("auth_error"),
        ..Default::default()
      },
      KeyStatus::Invalid,
    ),
    (
      Script {
        upgrade: Some(401),
        ..Default::default()
      },
      KeyStatus::Invalid,
    ),
    (
      Script {
        upgrade: Some(402),
        ..Default::default()
      },
      KeyStatus::Refused,
    ),
    (
      Script {
        upgrade: Some(429),
        ..Default::default()
      },
      KeyStatus::Cooldown,
    ),
  ] {
    let server = server(HashMap::from([("bad".into(), script)])).await;
    let mut r = start(&server, &["bad", "good"], timing()).await;
    r.feed(&audio(5), true).await;
    let events = r.collect().await;
    assert_eq!(events.last(), Some(&SttEvent::Finalized));
    assert_eq!(text(&events), words(&audio(5)));
    assert_eq!(r.pool.view().keys[0].status, status);
    assert!(server
      .received
      .lock()
      .unwrap()
      .get("bad")
      .is_none_or(|c| c.iter().all(Vec::is_empty)));
  }
}
#[tokio::test]
async fn midstream_quota_replays_every_uncommitted_sample_without_duplicate_interims() {
  let server = server(HashMap::from([("bad".into(), refused(3200))])).await;
  let mut r = start(&server, &["bad", "good"], timing()).await;
  let pcm = audio(12);
  r.feed(&pcm, true).await;
  let events = r.collect().await;
  assert_eq!(events.last(), Some(&SttEvent::Finalized));
  assert_eq!(text(&events), words(&pcm));
  assert!(
    server.received.lock().unwrap()["good"][0] == pcm,
    "replayed samples differ"
  );
  assert_eq!(r.pool.view().keys[0].status, KeyStatus::Refused);
}
#[tokio::test]
async fn commit_then_failure_keeps_audio_after_the_exact_fence() {
  let server = server(HashMap::from([("bad".into(), refused(1600 * 6))])).await;
  let mut t = timing();
  t.checkpoint_samples = 1600 * 4;
  let mut r = start(&server, &["bad", "good"], t).await;
  let pcm = audio(12);
  r.feed(&pcm, true).await;
  let events = r.collect().await;
  assert_eq!(events.last(), Some(&SttEvent::Finalized));
  assert_eq!(text(&events), words(&pcm));
  assert!(
    server.received.lock().unwrap()["good"][0] == pcm[1600 * 4..],
    "checkpoint tail differs"
  );
}
#[tokio::test]
async fn delayed_commit_does_not_acknowledge_audio_captured_after_the_fence() {
  let mut bad = refused(1600 * 6);
  bad.ack_delay = Duration::from_millis(60);
  let server = server(HashMap::from([("bad".into(), bad)])).await;
  let mut t = timing();
  t.checkpoint_samples = 1600 * 4;
  let mut r = start(&server, &["bad", "good"], t).await;
  let pcm = audio(12);
  r.feed(&pcm, true).await;
  let events = r.collect().await;
  assert_eq!(text(&events), words(&pcm));
  assert!(
    server.received.lock().unwrap()["good"][0] == pcm[6400..],
    "delayed checkpoint tail differs"
  );
}
#[tokio::test]
async fn network_close_and_reset_retry_the_same_key_without_rotating() {
  for reset in [false, true] {
    let script = Script {
      fail_after: 3200,
      fail_connections: 2,
      reset,
      ..Default::default()
    };
    let server = server(HashMap::from([("flaky".into(), script)])).await;
    let mut r = start(&server, &["flaky", "good"], timing()).await;
    let pcm = audio(8);
    r.feed(&pcm, true).await;
    let events = r.collect().await;
    assert_eq!(events.last(), Some(&SttEvent::Finalized));
    assert_eq!(text(&events), words(&pcm));
    // Two dropped connections, then the same key finishes: a blip never burns other keys.
    assert_eq!(server.received.lock().unwrap()["flaky"].len(), 3);
    assert!(
      server.received.lock().unwrap()["flaky"][2] == pcm,
      "replayed samples differ"
    );
    assert!(!server.received.lock().unwrap().contains_key("good"));
    assert_eq!(r.pool.view().keys[0].status, KeyStatus::Usable);
  }
}
#[tokio::test]
async fn finish_while_reconnecting_waits_for_all_audio() {
  let server = server(HashMap::from([
    ("bad".into(), refused(1600)),
    (
      "good".into(),
      Script {
        start_delay: Duration::from_millis(100),
        ..Default::default()
      },
    ),
  ]))
  .await;
  let mut r = start(&server, &["bad", "good"], timing()).await;
  let pcm = audio(8);
  r.feed(&pcm, false).await;
  loop {
    if r.events.recv().await.unwrap().1 == SttEvent::Reconnecting {
      break;
    }
  }
  r.audio.send(AudioMsg::Finish).await.unwrap();
  let events = r.collect().await;
  assert_eq!(events.last(), Some(&SttEvent::Finalized));
  assert_eq!(text(&events), words(&pcm));
  // The new key's handshake ends the reconnect state before any text arrives.
  let connected = events
    .iter()
    .position(|e| *e == SttEvent::Connected)
    .expect("connected after reconnect");
  let first_final = events.iter().position(|e| matches!(e, SttEvent::Final(_))).unwrap();
  assert!(connected < first_final, "{events:?}");
}
#[tokio::test]
async fn twenty_realistically_slow_refusals_then_good_finish_within_budget() {
  let keys: Vec<String> = (0..20)
    .map(|n| format!("bad-{n}"))
    .chain(std::iter::once("good".into()))
    .collect();
  let scripts = keys[..20]
    .iter()
    .map(|k| {
      let mut s = refused(1600);
      s.failure_delay = Duration::from_millis(1300);
      (k.clone(), s)
    })
    .collect();
  let server = server(scripts).await;
  let mut t = timing();
  t.failover = Duration::from_secs(40);
  t.acknowledgment = Duration::from_secs(3);
  let mut r = start(&server, &keys.iter().map(String::as_str).collect::<Vec<_>>(), t).await;
  let pcm = audio(8);
  let start = tokio::time::Instant::now();
  r.feed(&pcm, true).await;
  let events = r.collect().await;
  assert_eq!(events.last(), Some(&SttEvent::Finalized));
  assert_eq!(text(&events), words(&pcm));
  assert!(start.elapsed() >= Duration::from_secs(26));
  assert!(start.elapsed() < Duration::from_secs(40));
  assert_eq!(
    r.pool
      .view()
      .keys
      .iter()
      .filter(|k| k.status == KeyStatus::Refused)
      .count(),
    20
  );
}
#[tokio::test]
async fn recovery_budget_is_enforced_inside_an_attempt() {
  let mut bad = refused(1600);
  bad.failure_delay = Duration::from_millis(80);
  let server = server(HashMap::from([("a".into(), bad.clone()), ("b".into(), bad)])).await;
  let mut t = timing();
  t.failover = Duration::from_millis(40);
  let mut r = start(&server, &["a", "b", "good"], t).await;
  r.feed(&audio(8), true).await;
  let events = r.collect().await;
  assert_eq!(events.last(), Some(&SttEvent::Failed(ProviderError::Timeout)));
  assert!(!events.contains(&SttEvent::Finalized));
}
#[tokio::test]
async fn unacknowledged_finish_never_reports_success() {
  let server = server(HashMap::from([(
    "a".into(),
    Script {
      hang_commit: true,
      ..Default::default()
    },
  )]))
  .await;
  let mut t = timing();
  t.acknowledgment = Duration::from_millis(30);
  let mut r = start(&server, &["a"], t).await;
  r.feed(&audio(3), true).await;
  let events = r.collect().await;
  assert_eq!(events.last(), Some(&SttEvent::Failed(ProviderError::Network)));
  assert!(!events.contains(&SttEvent::Finalized));
}
#[tokio::test]
async fn cancel_releases_reservation_without_proving_the_key() {
  let server = server(HashMap::from([(
    "a".into(),
    Script {
      start_delay: Duration::from_secs(1),
      ..Default::default()
    },
  )]))
  .await;
  let mut r = start(&server, &["a"], timing()).await;
  r.feed(&audio(1), false).await;
  tokio::time::sleep(Duration::from_millis(20)).await;
  r.cancel.cancel();
  while r.events.recv().await.is_some() {}
  assert!(!r.pool.session_active());
  assert!(r
    .storage
    .load()
    .keys
    .iter()
    .all(|k| k.reserved == 0 && k.last_ok_ms.is_none()));
  assert_eq!(r.pool.view().keys[0].status, KeyStatus::Usable);
}
#[tokio::test]
async fn single_auth_failure_preserves_its_reason() {
  let server = server(HashMap::from([(
    "a".into(),
    Script {
      handshake_error: Some("auth_error"),
      ..Default::default()
    },
  )]))
  .await;
  let mut r = start(&server, &["a"], timing()).await;
  r.feed(&audio(1), true).await;
  assert_eq!(r.collect().await.last(), Some(&SttEvent::Failed(ProviderError::Auth)));
}

#[tokio::test]
async fn client_side_errors_fail_without_benching_any_key() {
  let script = Script {
    fail_after: 1600,
    fail_connections: 1,
    error: Some("input_error"),
    ..Default::default()
  };
  let server = server(HashMap::from([("a".into(), script)])).await;
  let mut r = start(&server, &["a", "b"], timing()).await;
  r.feed(&audio(4), true).await;
  let events = r.collect().await;
  assert!(
    matches!(events.last(), Some(SttEvent::Failed(ProviderError::Protocol(_)))),
    "{events:?}"
  );
  assert!(
    !server.received.lock().unwrap().contains_key("b"),
    "no rotation for our own bug"
  );
  assert!(r.pool.view().keys.iter().all(|k| k.status == KeyStatus::Usable));
}

#[tokio::test]
async fn silent_final_fence_finishes_without_text() {
  let script = Script {
    no_speech_on_commit: true,
    ..Default::default()
  };
  let server = server(HashMap::from([("a".into(), script)])).await;
  let mut r = start(&server, &["a"], timing()).await;
  r.feed(&vec![0; 8000], true).await;
  let events = r.collect().await;
  assert_eq!(events.last(), Some(&SttEvent::Finalized), "{events:?}");
  assert_eq!(server.received.lock().unwrap()["a"].len(), 1, "no reconnect");
}
