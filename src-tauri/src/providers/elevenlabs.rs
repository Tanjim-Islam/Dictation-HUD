//! ElevenLabs realtime speech-to-text adapter with key-pool failover.
//! Docs: https://elevenlabs.io/docs/api-reference/speech-to-text/v-1-speech-to-text-realtime
//!
//! Auth uses the `xi-api-key` header from Rust; no token or key reaches the WebView. Audio not
//! yet covered by a committed transcript is kept so a key failure mid-session can reconnect
//! with the next pooled key and replay it without losing speech.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

use super::elevenlabs_pool::{ElevenLabsPool, Lease, LeaseOutcome};
use super::{
  languages, pcm_bytes, AudioMsg, ModelInfo, ProviderDescriptor, ProviderError, StreamCtx, SttAdapter, SttEvent,
  SttOptions, SAMPLE_RATE,
};

/// Uncommitted audio kept for replay after a failover (90 s at 16 kHz).
const REPLAY_CAP: usize = 90 * SAMPLE_RATE as usize;
const MAX_KEYTERMS: usize = 50;
const MAX_KEYTERM_CHARS: usize = 20;
/// How long one session keeps failing over to further pooled keys after the first key fails.
const FAILOVER_BUDGET: Duration = Duration::from_secs(45);

pub fn descriptor() -> ProviderDescriptor {
  ProviderDescriptor {
    id: "elevenlabs".into(),
    name: "ElevenLabs".into(),
    models: vec![ModelInfo {
      id: "scribe_v2_realtime".into(),
      name: "Scribe v2 Realtime".into(),
      languages: languages::ELEVENLABS_SCRIBE_V2
        .iter()
        .map(|(a, _)| a.to_string())
        .collect(),
      keyterms: true,
    }],
    default_model: "scribe_v2_realtime".into(),
    multi_key: true,
  }
}

pub struct ElevenLabs {
  pool: Arc<ElevenLabsPool>,
  base_url: String,
}

impl ElevenLabs {
  pub fn new(pool: Arc<ElevenLabsPool>) -> Self {
    Self {
      pool,
      base_url: "wss://api.elevenlabs.io/v1/speech-to-text/realtime".into(),
    }
  }

  /// Points the adapter at another endpoint (local protocol tests).
  pub fn with_base_url(pool: Arc<ElevenLabsPool>, base_url: impl Into<String>) -> Self {
    Self {
      pool,
      base_url: base_url.into(),
    }
  }
}

pub fn build_url(base: &str, o: &SttOptions) -> String {
  let mut url = url::Url::parse(base).expect("static base url");
  {
    let mut q = url.query_pairs_mut();
    q.append_pair("model_id", &o.model)
      .append_pair("audio_format", "pcm_16000")
      .append_pair("commit_strategy", "vad");
    if let Some(code) = languages::elevenlabs_code(&o.language) {
      if !code.is_empty() {
        q.append_pair("language_code", code);
      }
    }
    for term in o
      .keyterms
      .iter()
      .filter(|t| t.chars().count() <= MAX_KEYTERM_CHARS)
      .take(MAX_KEYTERMS)
    {
      q.append_pair("keyterms", term);
    }
  }
  url.to_string()
}

#[derive(Debug, PartialEq)]
pub enum Parsed {
  Started,
  Partial(String),
  Committed(String),
  Error(LeaseOutcome, String),
  NoSpeech,
  Other,
}

pub fn parse_message(text: &str) -> Parsed {
  let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
    return Parsed::Other;
  };
  let t = v["message_type"].as_str().unwrap_or("");
  let body = || v["text"].as_str().unwrap_or("").to_string();
  // The provider's own explanation, cleaned for logs.
  let detail = provider_detail(v["error"].as_str().or(v["message"].as_str()).unwrap_or(""));
  let kind = |t: &str| {
    if detail.is_empty() {
      t.to_string()
    } else {
      format!("{t}: {detail}")
    }
  };
  match t {
    "session_started" => Parsed::Started,
    "partial_transcript" => Parsed::Partial(body()),
    "committed_transcript" => Parsed::Committed(body()),
    "auth_error" | "unaccepted_terms" => Parsed::Error(LeaseOutcome::Auth, kind(t)),
    "quota_exceeded" => Parsed::Error(LeaseOutcome::Quota, kind(t)),
    // No speech in the audio: not a failure.
    "insufficient_audio_activity" => Parsed::NoSpeech,
    "rate_limited" | "commit_throttled" | "queue_overflow" | "resource_exhausted" | "session_time_limit_exceeded" => {
      Parsed::Error(LeaseOutcome::Transient, kind(t))
    }
    other
      if other.contains("error")
        || other == "input_error"
        || other == "invalid_request"
        || other == "chunk_size_exceeded" =>
    {
      let detail = v["error"].as_str().unwrap_or("");
      let low = detail.to_lowercase();
      if ["quota", "insufficient", "credit", "exceeded"]
        .iter()
        .any(|w| low.contains(w))
      {
        Parsed::Error(LeaseOutcome::Quota, kind(other))
      } else {
        Parsed::Error(LeaseOutcome::Other, kind(other))
      }
    }
    _ => Parsed::Other,
  }
}

/// Provider error text safe to log: long token-like runs masked, whitespace collapsed, capped.
fn provider_detail(raw: &str) -> String {
  let words: Vec<String> = raw
    .split_whitespace()
    .map(|w| {
      let token = w
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .count();
      if token >= 24 {
        "***".to_string()
      } else {
        w.to_string()
      }
    })
    .collect();
  words.join(" ").chars().take(160).collect()
}

fn outcome_error(o: LeaseOutcome) -> ProviderError {
  match o {
    LeaseOutcome::Auth => ProviderError::Auth,
    LeaseOutcome::Quota => ProviderError::QuotaExceeded,
    LeaseOutcome::Transient => ProviderError::RateLimited,
    LeaseOutcome::Ok | LeaseOutcome::Other => ProviderError::Protocol("elevenlabs".into()),
  }
}

fn chunk_message(pcm: &[i16], commit: bool) -> String {
  serde_json::json!({
    "message_type": "input_audio_chunk",
    "audio_base_64": base64::engine::general_purpose::STANDARD.encode(pcm_bytes(pcm)),
    "commit": commit,
    "sample_rate": SAMPLE_RATE,
  })
  .to_string()
}

impl SttAdapter for ElevenLabs {
  fn descriptor(&self) -> ProviderDescriptor {
    descriptor()
  }

  fn configured(&self) -> bool {
    self.pool.configured()
  }

  fn spawn(&self, ctx: StreamCtx) {
    let pool = self.pool.clone();
    let base = self.base_url.clone();
    tauri::async_runtime::spawn(async move { run(pool, base, ctx).await });
  }
}

enum Attempt {
  /// Stream ended normally (Finalized emitted) or was cancelled.
  Done,
  /// The key failed; try another one.
  KeyFailed(LeaseOutcome),
  /// Unrecoverable for this session.
  Fatal(ProviderError),
}

struct Shared {
  replay: VecDeque<i16>,
  finishing: bool,
  connected_once: bool,
  audio_closed: bool,
}

async fn run(pool: Arc<ElevenLabsPool>, base: String, mut ctx: StreamCtx) {
  let mut tried: Vec<String> = Vec::new();
  let mut last_failure: Option<LeaseOutcome> = None;
  let mut shared = Shared {
    replay: VecDeque::new(),
    finishing: false,
    connected_once: false,
    audio_closed: false,
  };
  // Every pooled key may be tried (as in MonerJanala), within a time budget so a live
  // dictation never hangs on a long run of refused keys.
  let max_attempts = pool.view().keys.len().max(1);
  let mut first_failure: Option<Instant> = None;
  for _ in 0..max_attempts {
    if first_failure.is_some_and(|t| t.elapsed() > FAILOVER_BUDGET) {
      tracing::warn!(
        provider = "elevenlabs",
        tried = tried.len(),
        "failover time budget used up"
      );
      break;
    }
    let lease = tokio::select! {
      _ = ctx.cancel.cancelled() => return,
      l = pool.acquire(&tried) => l,
    };
    let lease = match lease {
      Ok(l) => l,
      // Report why the keys we tried failed, not that no key is left.
      Err(e) => {
        return ctx
          .emit(SttEvent::Failed(last_failure.map(outcome_error).unwrap_or(e)))
          .await
      }
    };
    tried.push(lease.key_id.clone());
    let result = attempt(&base, &lease, &mut ctx, &mut shared).await;
    match result {
      Attempt::Done => {
        pool.release(&lease.key_id, LeaseOutcome::Ok);
        let (p, id) = (pool.clone(), lease.key_id.clone());
        // Credits changed: refresh this key right away for the live total.
        tauri::async_runtime::spawn(async move { p.refresh(&id).await });
        return;
      }
      Attempt::KeyFailed(outcome) => {
        tracing::warn!(provider = "elevenlabs", key = %lease.key_id, outcome = ?outcome, tried = tried.len(), "key failed; trying next pooled key");
        first_failure.get_or_insert_with(Instant::now);
        pool.release(&lease.key_id, outcome);
        last_failure = Some(outcome);
        if ctx.cancel.is_cancelled() {
          return;
        }
        continue;
      }
      Attempt::Fatal(err) => {
        pool.release(&lease.key_id, LeaseOutcome::Other);
        return ctx.emit(SttEvent::Failed(err)).await;
      }
    }
  }
  ctx
    .emit(SttEvent::Failed(
      last_failure.map(outcome_error).unwrap_or(ProviderError::QuotaExceeded),
    ))
    .await;
}

/// After a failed send, the server has usually already explained why (for example
/// `auth_error` followed by close). Read briefly before deciding.
async fn failure_reason<S>(stream: &mut S) -> Attempt
where
  S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
  let deadline = tokio::time::sleep(Duration::from_millis(300));
  tokio::pin!(deadline);
  loop {
    tokio::select! {
      _ = &mut deadline => return Attempt::Fatal(ProviderError::Network),
      frame = stream.next() => match frame {
        Some(Ok(Message::Text(t))) => {
          if let Parsed::Error(outcome, _) = parse_message(&t) {
            return match outcome {
              LeaseOutcome::Auth | LeaseOutcome::Quota | LeaseOutcome::Transient => Attempt::KeyFailed(outcome),
              _ => Attempt::Fatal(outcome_error(outcome)),
            };
          }
        }
        Some(Ok(_)) => {}
        _ => return Attempt::Fatal(ProviderError::Network),
      }
    }
  }
}

async fn attempt(base: &str, lease: &Lease, ctx: &mut StreamCtx, s: &mut Shared) -> Attempt {
  let url = build_url(base, &ctx.options);
  let Ok(mut req) = url.as_str().into_client_request() else {
    return Attempt::Fatal(ProviderError::Protocol("request".into()));
  };
  let Ok(header) = lease.secret.parse() else {
    return Attempt::KeyFailed(LeaseOutcome::Auth);
  };
  req.headers_mut().insert("xi-api-key", header);
  let started = Instant::now();
  let connect = tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::connect_async(req));
  let ws = tokio::select! {
    _ = ctx.cancel.cancelled() => return Attempt::Done,
    r = connect => match r {
      Err(_) => return Attempt::Fatal(ProviderError::Timeout),
      Ok(Err(tokio_tungstenite::tungstenite::Error::Http(resp))) => {
        let status = resp.status().as_u16();
        tracing::warn!(provider = "elevenlabs", status, "upgrade rejected");
        return match status {
          401 | 403 => Attempt::KeyFailed(LeaseOutcome::Auth),
          402 => Attempt::KeyFailed(LeaseOutcome::Quota),
          429 | 500..=599 => Attempt::KeyFailed(LeaseOutcome::Transient),
          _ => Attempt::Fatal(ProviderError::from_status(status)),
        };
      }
      Ok(Err(_)) => return Attempt::Fatal(ProviderError::Network),
      Ok(Ok((ws, _))) => ws,
    },
  };
  let (mut sink, mut stream) = ws.split();

  // Handshake: no audio until the server accepts the session. An auth or quota rejection
  // arrives here, before any send can race the server closing the socket. Captured audio
  // keeps waiting in the queue meanwhile.
  let handshake = tokio::time::sleep(Duration::from_secs(10));
  tokio::pin!(handshake);
  loop {
    tokio::select! {
      _ = ctx.cancel.cancelled() => return Attempt::Done,
      _ = &mut handshake => return Attempt::Fatal(ProviderError::Timeout),
      frame = stream.next() => match frame {
        Some(Ok(Message::Text(t))) => match parse_message(&t) {
          Parsed::Started => break,
          Parsed::Error(outcome, kind) => {
            tracing::warn!(provider = "elevenlabs", kind = %kind, "session rejected");
            return match outcome {
              LeaseOutcome::Auth | LeaseOutcome::Quota | LeaseOutcome::Transient => Attempt::KeyFailed(outcome),
              _ => Attempt::Fatal(outcome_error(outcome)),
            };
          }
          _ => {}
        },
        Some(Ok(_)) => {}
        _ => return Attempt::Fatal(ProviderError::Network),
      }
    }
  }
  if !s.connected_once {
    s.connected_once = true;
    tracing::info!(provider = "elevenlabs", ms = started.elapsed().as_millis() as u64, language = %ctx.options.language, keyterms = ctx.options.keyterms.len(), "connected");
    ctx.emit(SttEvent::Connected).await;
  }

  // Replay audio the previous key did not commit.
  if !s.replay.is_empty() {
    let pending: Vec<i16> = s.replay.iter().copied().collect();
    for chunk in pending.chunks(SAMPLE_RATE as usize / 10) {
      if sink
        .send(Message::Text(chunk_message(chunk, false).into()))
        .await
        .is_err()
      {
        return failure_reason(&mut stream).await;
      }
    }
  }
  if s.finishing {
    let _ = sink.send(Message::Text(chunk_message(&[], true).into())).await;
  }

  loop {
    tokio::select! {
      _ = ctx.cancel.cancelled() => {
        let _ = sink.send(Message::Close(None)).await;
        return Attempt::Done;
      }
      msg = ctx.audio.recv(), if !s.audio_closed && !s.finishing => match msg {
        Some(AudioMsg::Pcm(pcm)) => {
          s.replay.extend(pcm.iter().copied());
          while s.replay.len() > REPLAY_CAP {
            s.replay.pop_front();
          }
          if sink.send(Message::Text(chunk_message(&pcm, false).into())).await.is_err() {
            return failure_reason(&mut stream).await;
          }
        }
        Some(AudioMsg::Finish) | None => {
          s.audio_closed = true;
          s.finishing = true;
          if s.replay.is_empty() {
            let _ = sink.send(Message::Close(None)).await;
            ctx.emit(SttEvent::Finalized).await;
            return Attempt::Done;
          }
          let _ = sink.send(Message::Text(chunk_message(&[], true).into())).await;
        }
      },
      frame = stream.next() => match frame {
        Some(Ok(Message::Text(t))) => match parse_message(&t) {
          Parsed::Started => {}
          Parsed::Partial(text) => ctx.emit(SttEvent::Interim(text)).await,
          Parsed::Committed(text) => {
            s.replay.clear();
            ctx.emit(SttEvent::Final(text)).await;
            if s.finishing {
              let _ = sink.send(Message::Close(None)).await;
              ctx.emit(SttEvent::Finalized).await;
              return Attempt::Done;
            }
          }
          Parsed::Error(outcome, kind) => {
            tracing::warn!(provider = "elevenlabs", kind = %kind, "provider error message");
            return match outcome {
              LeaseOutcome::Auth | LeaseOutcome::Quota | LeaseOutcome::Transient => Attempt::KeyFailed(outcome),
              _ => Attempt::Fatal(outcome_error(outcome)),
            };
          }
          Parsed::NoSpeech => {
            if s.finishing {
              let _ = sink.send(Message::Close(None)).await;
              ctx.emit(SttEvent::Finalized).await;
              return Attempt::Done;
            }
          }
          Parsed::Other => {}
        },
        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
          if s.finishing {
            ctx.emit(SttEvent::Finalized).await;
            return Attempt::Done;
          }
          return Attempt::Fatal(ProviderError::Network);
        }
        _ => {}
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn url_maps_language_and_filters_keyterms() {
    let o = SttOptions {
      model: "scribe_v2_realtime".into(),
      language: "bn".into(),
      keyterms: vec!["City Bank".into(), "a-very-long-keyterm-over-twenty".into()],
    };
    let u = build_url("wss://api.elevenlabs.io/v1/speech-to-text/realtime", &o);
    assert!(u.contains("language_code=ben"));
    assert!(u.contains("audio_format=pcm_16000"));
    assert!(u.contains("keyterms=City+Bank"));
    assert!(!u.contains("a-very-long"));
    assert!(!u.contains("token=") && !u.contains("xi-api-key"));
  }

  #[test]
  fn multi_omits_language_code() {
    let o = SttOptions {
      model: "scribe_v2_realtime".into(),
      language: "multi".into(),
      keyterms: vec![],
    };
    assert!(!build_url("wss://x.test/rt", &o).contains("language_code"));
  }

  #[test]
  fn parses_messages() {
    assert_eq!(
      parse_message(r#"{"message_type":"session_started","session_id":"s"}"#),
      Parsed::Started
    );
    assert_eq!(
      parse_message(r#"{"message_type":"partial_transcript","text":"hi"}"#),
      Parsed::Partial("hi".into())
    );
    assert_eq!(
      parse_message(r#"{"message_type":"committed_transcript","text":"Hi."}"#),
      Parsed::Committed("Hi.".into())
    );
    assert!(matches!(
      parse_message(r#"{"message_type":"auth_error","error":"x"}"#),
      Parsed::Error(LeaseOutcome::Auth, _)
    ));
    assert!(matches!(
      parse_message(r#"{"message_type":"quota_exceeded"}"#),
      Parsed::Error(LeaseOutcome::Quota, _)
    ));
    assert!(matches!(
      parse_message(r#"{"message_type":"rate_limited"}"#),
      Parsed::Error(LeaseOutcome::Transient, _)
    ));
    assert!(matches!(
      parse_message(r#"{"message_type":"error","error":"Insufficient credits"}"#),
      Parsed::Error(LeaseOutcome::Quota, _)
    ));
  }

  #[test]
  fn error_detail_is_kept_but_masks_tokens() {
    let m = r#"{"message_type":"quota_exceeded","error":"Key   abcdefghijklmnopqrstuvwxyz0123 has a credit limit"}"#;
    let Parsed::Error(LeaseOutcome::Quota, kind) = parse_message(m) else {
      panic!("quota error expected");
    };
    assert_eq!(kind, "quota_exceeded: Key *** has a credit limit");
    assert!(provider_detail(&"x ".repeat(200)).chars().count() <= 160);
  }

  #[test]
  fn chunk_message_is_base64_pcm() {
    let m: serde_json::Value = serde_json::from_str(&chunk_message(&[1, -2], true)).unwrap();
    assert_eq!(m["audio_base_64"], "AQD+/w==");
    assert_eq!(m["commit"], true);
    assert_eq!(m["sample_rate"], 16000);
  }
}
