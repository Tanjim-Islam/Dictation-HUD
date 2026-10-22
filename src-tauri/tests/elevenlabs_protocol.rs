//! ElevenLabs adapter against a local mock of the realtime protocol: header auth, pooled-key
//! failover after `auth_error`, replay of uncommitted audio on the next key, commit on finish.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use dictation_hud::credentials::MemorySecretStore;
use dictation_hud::providers::elevenlabs::ElevenLabs;
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
  fn fetch(&self, _secret: String) -> BoxFuture<Result<Credits, FetchError>> {
    Box::pin(async {
      Ok(Credits {
        character_count: 0,
        character_limit: 100_000,
      })
    })
  }
}

/// Records audio bytes received per key.
type Received = Arc<Mutex<HashMap<String, usize>>>;

async fn mock_server() -> (String, Received) {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let addr = listener.local_addr().unwrap();
  let received: Received = Arc::default();
  let rec = received.clone();
  tokio::spawn(async move {
    while let Ok((tcp, _)) = listener.accept().await {
      let rec = rec.clone();
      tokio::spawn(async move {
        let key = Arc::new(Mutex::new(String::new()));
        let k2 = key.clone();
        // The handshake callback's error type is fixed by tungstenite (a full HTTP response).
        #[allow(clippy::result_large_err)]
        let cb = move |req: &Request, resp: Response| {
          *k2.lock().unwrap() = req
            .headers()
            .get("xi-api-key")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
          Ok(resp)
        };
        let mut ws = tokio_tungstenite::accept_hdr_async(tcp, cb).await.unwrap();
        let key = key.lock().unwrap().clone();
        // Accepted, then refused for quota once audio arrives (seen live with keys whose
        // subscription still showed full credit).
        if key.starts_with("refused-") {
          let _ = ws
            .send(Message::Text(
              r#"{"message_type":"session_started","session_id":"s0"}"#.into(),
            ))
            .await;
          let _ = ws.next().await;
          let _ = ws
            .send(Message::Text(
              r#"{"message_type":"quota_exceeded","error":"Quota exceeded"}"#.into(),
            ))
            .await;
          // Close cleanly: read what the client still sends so the socket is not reset.
          let _ = ws.close(None).await;
          while let Some(Ok(_)) = ws.next().await {}
          return;
        }
        if key != "good-key" {
          let _ = ws
            .send(Message::Text(
              r#"{"message_type":"auth_error","error":"You must be authenticated"}"#.into(),
            ))
            .await;
          let _ = ws.close(None).await;
          return;
        }
        let _ = ws
          .send(Message::Text(
            r#"{"message_type":"session_started","session_id":"s1"}"#.into(),
          ))
          .await;
        let mut sent_partial = false;
        while let Some(Ok(msg)) = ws.next().await {
          if let Message::Text(t) = msg {
            let v: serde_json::Value = serde_json::from_str(&t).unwrap();
            let b64 = v["audio_base_64"].as_str().unwrap_or("");
            use base64::Engine as _;
            let n = base64::engine::general_purpose::STANDARD
              .decode(b64)
              .map(|b| b.len())
              .unwrap_or(0);
            *rec.lock().unwrap().entry(key.clone()).or_default() += n;
            if !sent_partial && n > 0 {
              sent_partial = true;
              let _ = ws
                .send(Message::Text(
                  r#"{"message_type":"partial_transcript","text":"hello"}"#.into(),
                ))
                .await;
            }
            if v["commit"].as_bool() == Some(true) {
              let _ = ws
                .send(Message::Text(
                  r#"{"message_type":"committed_transcript","text":"hello world"}"#.into(),
                ))
                .await;
            }
          }
        }
      });
    }
  });
  (format!("ws://{addr}/v1/speech-to-text/realtime"), received)
}

#[tokio::test(flavor = "multi_thread")]
async fn fails_over_to_next_key_and_replays_audio() {
  let (url, received) = mock_server().await;
  let pool = Arc::new(ElevenLabsPool::new(
    Arc::new(MemorySecretStore::default()),
    Arc::new(MemStorage::default()),
    Arc::new(Credit),
  ));
  pool.add("bad-key").await.unwrap();
  pool.add("good-key").await.unwrap();
  pool.set_multi(true);
  let adapter = ElevenLabs::with_base_url(pool.clone(), url);

  let (audio_tx, audio_rx) = mpsc::channel(64);
  let (ev_tx, mut ev_rx) = mpsc::channel(64);
  let id = uuid::Uuid::new_v4();
  adapter.spawn(StreamCtx {
    session_id: id,
    options: SttOptions {
      model: "scribe_v2_realtime".into(),
      language: "en".into(),
      keyterms: vec![],
    },
    audio: audio_rx,
    events: ev_tx,
    cancel: CancellationToken::new(),
  });

  // 0.5 s of audio, queued before and during the failover.
  for _ in 0..5 {
    audio_tx.send(AudioMsg::Pcm(vec![100i16; 1600])).await.unwrap();
  }
  tokio::time::sleep(Duration::from_millis(400)).await;
  audio_tx.send(AudioMsg::Finish).await.unwrap();

  let mut events = Vec::new();
  while let Ok(Some((sid, ev))) = tokio::time::timeout(Duration::from_secs(5), ev_rx.recv()).await {
    assert_eq!(sid, id, "every event carries the session id");
    let done = matches!(ev, SttEvent::Finalized | SttEvent::Failed(_));
    events.push(ev);
    if done {
      break;
    }
  }
  assert!(events.contains(&SttEvent::Connected));
  assert!(events.contains(&SttEvent::Final("hello world".into())), "{events:?}");
  assert_eq!(events.last(), Some(&SttEvent::Finalized));
  // All 0.5 s (16,000 bytes) reached the good key even though key 1 failed first.
  assert!(received.lock().unwrap().get("good-key").copied().unwrap_or(0) >= 16_000);
  // Key 1 is now marked invalid.
  let view = pool.view();
  assert_eq!(view.keys[0].status, KeyStatus::Invalid);
}

#[tokio::test(flavor = "multi_thread")]
async fn single_bad_key_reports_auth_not_quota() {
  let (url, _) = mock_server().await;
  let pool = Arc::new(ElevenLabsPool::new(
    Arc::new(MemorySecretStore::default()),
    Arc::new(MemStorage::default()),
    Arc::new(Credit),
  ));
  pool.add("bad-key").await.unwrap();
  let adapter = ElevenLabs::with_base_url(pool, url);
  let (audio_tx, audio_rx) = mpsc::channel(8);
  let (ev_tx, mut ev_rx) = mpsc::channel(8);
  adapter.spawn(StreamCtx {
    session_id: uuid::Uuid::new_v4(),
    options: SttOptions {
      model: "scribe_v2_realtime".into(),
      language: "en".into(),
      keyterms: vec![],
    },
    audio: audio_rx,
    events: ev_tx,
    cancel: CancellationToken::new(),
  });
  audio_tx.send(AudioMsg::Pcm(vec![0; 1600])).await.unwrap();
  let (_, ev) = tokio::time::timeout(Duration::from_secs(5), ev_rx.recv())
    .await
    .unwrap()
    .unwrap();
  assert_eq!(ev, SttEvent::Failed(ProviderError::Auth));
}

#[tokio::test(flavor = "multi_thread")]
async fn tries_past_many_refused_keys_and_benches_them() {
  let (url, received) = mock_server().await;
  let pool = Arc::new(ElevenLabsPool::new(
    Arc::new(MemorySecretStore::default()),
    Arc::new(MemStorage::default()),
    Arc::new(Credit),
  ));
  for i in 0..7 {
    pool.add(&format!("refused-{i}")).await.unwrap();
  }
  pool.add("good-key").await.unwrap();
  pool.set_multi(true);
  let adapter = ElevenLabs::with_base_url(pool.clone(), url);
  let (audio_tx, audio_rx) = mpsc::channel(64);
  let (ev_tx, mut ev_rx) = mpsc::channel(64);
  adapter.spawn(StreamCtx {
    session_id: uuid::Uuid::new_v4(),
    options: SttOptions {
      model: "scribe_v2_realtime".into(),
      language: "en".into(),
      keyterms: vec![],
    },
    audio: audio_rx,
    events: ev_tx,
    cancel: CancellationToken::new(),
  });
  for _ in 0..5 {
    let _ = audio_tx.send(AudioMsg::Pcm(vec![100i16; 1600])).await;
  }
  tokio::time::sleep(Duration::from_millis(600)).await;
  let _ = audio_tx.send(AudioMsg::Finish).await;
  let mut last = None;
  while let Ok(Some((_, ev))) = tokio::time::timeout(Duration::from_secs(10), ev_rx.recv()).await {
    let done = matches!(ev, SttEvent::Finalized | SttEvent::Failed(_));
    last = Some(ev);
    if done {
      break;
    }
  }
  // More than five keys failed, and the session still finished on the working one.
  assert_eq!(last, Some(SttEvent::Finalized));
  assert!(received.lock().unwrap().get("good-key").copied().unwrap_or(0) >= 16_000);
  let view = pool.view();
  assert_eq!(view.keys.iter().filter(|k| k.status == KeyStatus::Refused).count(), 7);
}
