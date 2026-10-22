//! ElevenLabs realtime speech-to-text adapter with key-pool failover.
//! Docs: https://elevenlabs.io/docs/api-reference/speech-to-text/v-1-speech-to-text-realtime
//!
//! Auth uses the `xi-api-key` header from Rust; no token or key reaches the WebView. Audio not
//! yet covered by a committed transcript is kept so a key failure mid-session can reconnect
//! with the next pooled key and replay it without losing speech.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

use super::elevenlabs_pool::{ElevenLabsPool, Lease, LeaseOutcome, Reservation};
use super::{
  languages, pcm_bytes, AudioMsg, ModelInfo, ProviderDescriptor, ProviderError, StreamCtx, SttAdapter, SttEvent,
  SttOptions, SAMPLE_RATE,
};

/// Uncommitted audio kept for replay after a failover (90 s at 16 kHz).
const REPLAY_CAP: usize = 90 * SAMPLE_RATE as usize;
const MAX_KEYTERMS: usize = 50;
const MAX_KEYTERM_CHARS: usize = 20;
/// How long one session keeps failing over to further pooled keys after the first key fails.
const FAILOVER_BUDGET: Duration = Duration::from_millis(super::RECOVERY_BUDGET_MS);

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
  timing: Timing,
}

impl ElevenLabs {
  pub fn new(pool: Arc<ElevenLabsPool>) -> Self {
    Self {
      pool,
      base_url: "wss://api.elevenlabs.io/v1/speech-to-text/realtime".into(),
      timing: Timing::default(),
    }
  }

  pub fn with_timing(mut self, timing: Timing) -> Self {
    self.timing = timing;
    self
  }

  /// Points the adapter at another endpoint (local protocol tests).
  pub fn with_base_url(pool: Arc<ElevenLabsPool>, base_url: impl Into<String>) -> Self {
    Self {
      pool,
      base_url: base_url.into(),
      timing: Timing::default(),
    }
  }
}

pub fn build_url(base: &str, o: &SttOptions) -> String {
  let mut url = url::Url::parse(base).expect("static base url");
  {
    let mut q = url.query_pairs_mut();
    q.append_pair("model_id", &o.model)
      .append_pair("audio_format", "pcm_16000")
      .append_pair("commit_strategy", "manual");
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
    // Only this message benches a key. Error text is never matched for words like "exceeded":
    // a client-side error would otherwise quarantine every key in turn.
    "quota_exceeded" => Parsed::Error(LeaseOutcome::Quota, kind(t)),
    // No speech in the audio: not a failure.
    "insufficient_audio_activity" => Parsed::NoSpeech,
    // Limits of the key or account: try another key for a while.
    "rate_limited" | "queue_overflow" | "resource_exhausted" => Parsed::Error(LeaseOutcome::Transient, kind(t)),
    // Limits of this socket, not the key: reconnect and replay without penalizing the key.
    "commit_throttled" | "session_time_limit_exceeded" | "transcriber_error" => {
      Parsed::Error(LeaseOutcome::Network, kind(t))
    }
    // Our request was malformed: retrying another key cannot help, and no key is at fault.
    other if other.contains("error") || other == "invalid_request" || other == "chunk_size_exceeded" => {
      Parsed::Error(LeaseOutcome::Other, kind(other))
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
    LeaseOutcome::Network => ProviderError::Network,
    LeaseOutcome::Cancelled => ProviderError::Protocol("cancelled".into()),
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

  fn activity_guard(&self) -> Option<Box<dyn Send + Sync>> {
    Some(Box::new(self.pool.begin_session()))
  }

  fn spawn(&self, ctx: StreamCtx) {
    let pool = self.pool.clone();
    let base = self.base_url.clone();
    let timing = self.timing;
    tauri::async_runtime::spawn(async move { run(pool, base, ctx, timing).await });
  }
}

/// The hard recovery bound includes acquisition, connecting, sending, and commit acknowledgment.
#[derive(Clone, Copy)]
pub struct Timing {
  pub failover: Duration,
  pub connect: Duration,
  pub acknowledgment: Duration,
  pub chunk_interval: Duration,
  pub checkpoint_samples: usize,
  /// First wait before reconnecting after a network failure; doubles up to 8x.
  pub network_backoff: Duration,
}

impl Default for Timing {
  fn default() -> Self {
    Self {
      failover: FAILOVER_BUDGET,
      connect: Duration::from_secs(8),
      acknowledgment: Duration::from_secs(10),
      chunk_interval: Duration::from_millis(25),
      checkpoint_samples: 20 * SAMPLE_RATE as usize,
      network_backoff: Duration::from_millis(250),
    }
  }
}

enum Attempt {
  Done,
  Cancelled,
  KeyFailed(LeaseOutcome),
  Fatal(ProviderError),
}

struct Shared {
  replay: VecDeque<i16>,
  finishing: bool,
  connected_once: bool,
  recovery_deadline: Option<tokio::time::Instant>,
}

impl Shared {
  fn deadline(&self, wait: Duration) -> tokio::time::Instant {
    let local = tokio::time::Instant::now() + wait;
    self.recovery_deadline.map_or(local, |d| d.min(local))
  }
}

/// Sixteen seconds catches observed delayed refusals that short silent probes miss.
/// The idle caller cancels this future as soon as a dictation begins.
pub async fn probe_access(secret: String) -> Result<(), LeaseOutcome> {
  let (audio_tx, audio) = tokio::sync::mpsc::channel(162);
  for _ in 0..160 {
    audio_tx.try_send(AudioMsg::Pcm(vec![0; 1600])).unwrap();
  }
  audio_tx.try_send(AudioMsg::Finish).unwrap();
  let (events, _events_rx) = tokio::sync::mpsc::channel(256);
  let mut ctx = StreamCtx {
    session_id: uuid::Uuid::new_v4(),
    options: SttOptions {
      model: "scribe_v2_realtime".into(),
      language: "multi".into(),
      keyterms: vec![],
    },
    audio,
    events,
    cancel: tokio_util::sync::CancellationToken::new(),
  };
  let mut s = Shared {
    replay: VecDeque::new(),
    finishing: false,
    connected_once: false,
    recovery_deadline: None,
  };
  let lease = Lease {
    key_id: "idle-probe".into(),
    secret,
  };
  match attempt(
    "wss://api.elevenlabs.io/v1/speech-to-text/realtime",
    &lease,
    &mut ctx,
    &mut s,
    Timing::default(),
  )
  .await
  {
    Attempt::Done => Ok(()),
    Attempt::KeyFailed(o) => Err(o),
    _ => Err(LeaseOutcome::Other),
  }
}

async fn run(pool: Arc<ElevenLabsPool>, base: String, mut ctx: StreamCtx, timing: Timing) {
  let _activity = pool.begin_session();
  let mut tried = Vec::new();
  let mut last_failure = None;
  let mut s = Shared {
    replay: VecDeque::new(),
    finishing: false,
    connected_once: false,
    recovery_deadline: None,
  };
  loop {
    // Each key is excluded once tried. New keys may join, but the recovery deadline never moves.
    let lease = tokio::select! {
      biased;
      _ = ctx.cancel.cancelled() => return,
      _ = tokio::time::sleep_until(s.deadline(timing.failover)) => {
        return ctx.emit(SttEvent::Failed(ProviderError::Timeout)).await;
      }
      l = pool.acquire(&tried) => match l {
        Ok(l) => l,
        Err(e) => return ctx.emit(SttEvent::Failed(last_failure.map(outcome_error).unwrap_or(e))).await,
      },
    };
    tried.push(lease.key_id.clone());
    let mut reservation = Reservation::new(pool.clone(), lease);
    let mut network_failures = 0u32;
    let result = loop {
      let result = attempt(&base, &reservation.lease, &mut ctx, &mut s, timing).await;
      if !matches!(result, Attempt::KeyFailed(LeaseOutcome::Network)) {
        break result;
      }
      // A dropped connection or an offline PC says nothing about the key. Keep this key and
      // retry with backoff until the recovery deadline instead of running through the pool.
      s.recovery_deadline
        .get_or_insert_with(|| tokio::time::Instant::now() + timing.failover);
      ctx.emit(SttEvent::Reconnecting).await;
      let backoff = timing.network_backoff * 2u32.pow(network_failures.min(3));
      network_failures += 1;
      tracing::warn!(provider = "elevenlabs", key = %reservation.lease.key_id, attempt = network_failures, "connection lost; retrying the same key");
      tokio::select! {
        biased;
        _ = ctx.cancel.cancelled() => return,
        _ = tokio::time::sleep_until(s.deadline(timing.failover)) => {
          return ctx.emit(SttEvent::Failed(ProviderError::Network)).await;
        }
        _ = tokio::time::sleep(backoff) => {}
      }
    };
    match result {
      Attempt::Done => {
        reservation.outcome(LeaseOutcome::Ok);
        let id = reservation.lease.key_id.clone();
        drop(reservation);
        tauri::async_runtime::spawn(async move { pool.refresh(&id).await });
        return;
      }
      Attempt::Cancelled => return,
      Attempt::KeyFailed(outcome) => {
        tracing::warn!(provider = "elevenlabs", key = %reservation.lease.key_id, ?outcome, tried = tried.len(), "key failed; replaying on next key");
        reservation.outcome(outcome);
        drop(reservation);
        s.recovery_deadline
          .get_or_insert_with(|| tokio::time::Instant::now() + timing.failover);
        ctx.emit(SttEvent::Reconnecting).await;
        last_failure = Some(outcome);
      }
      Attempt::Fatal(error) => {
        reservation.outcome(LeaseOutcome::Other);
        return ctx.emit(SttEvent::Failed(error)).await;
      }
    }
  }
}

async fn failure_reason<S>(stream: &mut S) -> Attempt
where
  S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
  let deadline = tokio::time::sleep(Duration::from_millis(150));
  tokio::pin!(deadline);
  loop {
    tokio::select! {
      _ = &mut deadline => return Attempt::KeyFailed(LeaseOutcome::Network),
      frame = stream.next() => match frame {
        Some(Ok(Message::Text(t))) => if let Parsed::Error(outcome, _) = parse_message(&t) {
          return Attempt::KeyFailed(outcome);
        },
        Some(Ok(_)) => {},
        _ => return Attempt::KeyFailed(LeaseOutcome::Network),
      }
    }
  }
}

async fn attempt(base: &str, lease: &Lease, ctx: &mut StreamCtx, s: &mut Shared, timing: Timing) -> Attempt {
  let url = build_url(base, &ctx.options);
  let Ok(mut req) = url.as_str().into_client_request() else {
    return Attempt::Fatal(ProviderError::Protocol("request".into()));
  };
  let Ok(header) = lease.secret.parse() else {
    return Attempt::KeyFailed(LeaseOutcome::Auth);
  };
  req.headers_mut().insert("xi-api-key", header);
  let connect = tokio::time::timeout_at(s.deadline(timing.connect), tokio_tungstenite::connect_async(req));
  let ws = tokio::select! {
    _ = ctx.cancel.cancelled() => return Attempt::Cancelled,
    r = connect => match r {
      Err(_) => return Attempt::KeyFailed(LeaseOutcome::Network),
      Ok(Err(tokio_tungstenite::tungstenite::Error::Http(resp))) => {
        return match resp.status().as_u16() {
          401 | 403 => Attempt::KeyFailed(LeaseOutcome::Auth),
          402 => Attempt::KeyFailed(LeaseOutcome::Quota),
          408 => Attempt::KeyFailed(LeaseOutcome::Network),
          429 | 500..=599 => Attempt::KeyFailed(LeaseOutcome::Transient),
          s => Attempt::Fatal(ProviderError::from_status(s)),
        };
      }
      Ok(Err(_)) => return Attempt::KeyFailed(LeaseOutcome::Network),
      Ok(Ok((ws, _))) => ws,
    },
  };
  let (mut sink, mut stream) = ws.split();
  let handshake = tokio::time::sleep_until(s.deadline(timing.connect));
  tokio::pin!(handshake);
  loop {
    tokio::select! {
      _ = ctx.cancel.cancelled() => return Attempt::Cancelled,
      _ = &mut handshake => return Attempt::KeyFailed(LeaseOutcome::Network),
      frame = stream.next() => match frame {
        Some(Ok(Message::Text(t))) => match parse_message(&t) {
          Parsed::Started => break,
          Parsed::Error(outcome, _) => return Attempt::KeyFailed(outcome),
          _ => {},
        },
        Some(Ok(Message::Ping(p))) => {
          tokio::select! {
            _ = ctx.cancel.cancelled() => return Attempt::Cancelled,
            r = tokio::time::timeout_at(s.deadline(Duration::from_secs(2)), sink.send(Message::Pong(p))) => {
              if !matches!(r, Ok(Ok(()))) { return Attempt::KeyFailed(LeaseOutcome::Network); }
            }
          }
        }
        Some(Ok(_)) => {},
        _ => return Attempt::KeyFailed(LeaseOutcome::Network),
      }
    }
  }
  // The first connect, and every reconnect: the Flow Bar stops showing a reconnect as soon as
  // the new socket accepts audio, not only after the next commit (up to 20 s later).
  if !s.connected_once || s.recovery_deadline.is_some() {
    s.connected_once = true;
    ctx.emit(SttEvent::Connected).await;
  }
  // Manual commits are fences: only this socket's sent prefix is in flight. Capture after
  // the fence stays in the input queue until its acknowledgment. No timestamp guessing,
  // overlap matching, or promotion of an interim hypothesis is needed.
  let mut sent = 0;
  let mut committing = None;
  let mut tick = tokio::time::interval(timing.chunk_interval.max(Duration::from_millis(1)));
  tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
  loop {
    if s.recovery_deadline.is_some_and(|d| d <= tokio::time::Instant::now()) {
      return Attempt::Fatal(ProviderError::Timeout);
    }
    if s.finishing && s.replay.is_empty() && committing.is_none() {
      ctx.emit(SttEvent::Finalized).await;
      return Attempt::Done;
    }
    tokio::select! {
      biased;
      _ = ctx.cancel.cancelled() => return Attempt::Cancelled,
      _ = tokio::time::sleep_until(s.deadline(Duration::from_secs(86400))), if s.recovery_deadline.is_some() => {
        return Attempt::Fatal(ProviderError::Timeout);
      }
      _ = tokio::time::sleep_until(committing.unwrap_or_else(|| tokio::time::Instant::now() + timing.acknowledgment)), if committing.is_some() => {
        return Attempt::KeyFailed(LeaseOutcome::Network);
      }
      frame = stream.next() => match frame {
        Some(Ok(Message::Text(t))) => match parse_message(&t) {
          Parsed::Partial(text) => ctx.emit(SttEvent::Interim(text)).await,
          Parsed::Committed(text) => {
            if committing.is_none() {
              // An automatic commit we did not ask for: its audio coverage is unknown. Drop this
              // socket without using its text and replay everything unacknowledged.
              tracing::warn!(provider = "elevenlabs", "unexpected commit; reconnecting");
              return Attempt::KeyFailed(LeaseOutcome::Network);
            }
            // Nothing after this fence was sent or taken out of the capture queue.
            s.replay.drain(..sent);
            sent = 0;
            committing = None;
            ctx.emit(SttEvent::Final(text)).await;
            // Replayed audio is acknowledged: recovery is over and its time budget ends.
            s.recovery_deadline = None;
          }
          Parsed::NoSpeech => {
            if s.finishing && committing.is_some() {
              // The final fence held no speech (a silent or very short dictation): the audio is
              // accounted for and the session ends without text.
              s.replay.drain(..sent);
              sent = 0;
              committing = None;
            } else {
              // The socket idled out mid-dictation. Reconnect and replay what is unacknowledged.
              return Attempt::KeyFailed(LeaseOutcome::Network);
            }
          }
          Parsed::Error(outcome, _) => return match outcome {
            LeaseOutcome::Other => Attempt::Fatal(outcome_error(outcome)),
            _ => Attempt::KeyFailed(outcome),
          },
          _ => {},
        },
        Some(Ok(Message::Ping(p))) => {
          tokio::select! {
            _ = ctx.cancel.cancelled() => return Attempt::Cancelled,
            r = tokio::time::timeout_at(s.deadline(Duration::from_secs(2)), sink.send(Message::Pong(p))) => {
              if !matches!(r, Ok(Ok(()))) { return Attempt::KeyFailed(LeaseOutcome::Network); }
            }
          }
        }
        Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return Attempt::KeyFailed(LeaseOutcome::Network),
        _ => {},
      },
      _ = tick.tick(), if committing.is_none() && sent < s.replay.len() => {
        let end = (sent + SAMPLE_RATE as usize / 10).min(s.replay.len());
        let chunk: Vec<i16> = s.replay.range(sent..end).copied().collect();
        let r = tokio::select! {
          _ = ctx.cancel.cancelled() => return Attempt::Cancelled,
          r = tokio::time::timeout_at(s.deadline(Duration::from_secs(3)), sink.send(Message::Text(chunk_message(&chunk, false).into()))) => r,
        };
        if !matches!(r, Ok(Ok(()))) { return failure_reason(&mut stream).await; }
        sent = end;
        if sent == s.replay.len() && (s.finishing || sent >= timing.checkpoint_samples) {
          let r = tokio::select! {
              _ = ctx.cancel.cancelled() => return Attempt::Cancelled,
              r = tokio::time::timeout_at(s.deadline(Duration::from_secs(3)), sink.send(Message::Text(chunk_message(&[], true).into()))) => r,
            };
          if !matches!(r, Ok(Ok(()))) { return failure_reason(&mut stream).await; }
          committing = Some(s.deadline(timing.acknowledgment));
        }
      }
      msg = ctx.audio.recv(), if !s.finishing && committing.is_none() && s.replay.len() < timing.checkpoint_samples => match msg {
        Some(AudioMsg::Pcm(pcm)) => {
          if s.replay.len() + pcm.len() > REPLAY_CAP {
            return Attempt::Fatal(ProviderError::Protocol("audio backlog exceeds replay capacity".into()));
          }
          s.replay.extend(pcm);
        }
        Some(AudioMsg::Finish) | None => {
          s.finishing = true;
          if sent == s.replay.len() && sent > 0 {
            let r = tokio::select! {
              _ = ctx.cancel.cancelled() => return Attempt::Cancelled,
              r = tokio::time::timeout_at(s.deadline(Duration::from_secs(3)), sink.send(Message::Text(chunk_message(&[], true).into()))) => r,
            };
            if !matches!(r, Ok(Ok(()))) { return failure_reason(&mut stream).await; }
            committing = Some(s.deadline(timing.acknowledgment));
          }
        }
      },
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
    // Words in error text never bench a key; only the quota message type does.
    for m in [
      r#"{"message_type":"error","error":"Insufficient credits"}"#,
      r#"{"message_type":"input_error","error":"keyterms exceeded limit"}"#,
      r#"{"message_type":"chunk_size_exceeded","error":"chunk size exceeded"}"#,
    ] {
      assert!(matches!(parse_message(m), Parsed::Error(LeaseOutcome::Other, _)), "{m}");
    }
    for t in ["commit_throttled", "session_time_limit_exceeded", "transcriber_error"] {
      let m = format!(r#"{{"message_type":"{t}"}}"#);
      assert!(
        matches!(parse_message(&m), Parsed::Error(LeaseOutcome::Network, _)),
        "{t}"
      );
    }
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

  #[test]
  fn default_recovery_and_flush_fit_the_session_hard_limit() {
    let t = Timing::default();
    let replay = t.chunk_interval * (t.checkpoint_samples / (SAMPLE_RATE as usize / 10)) as u32;
    assert!(t.failover + replay + t.acknowledgment <= Duration::from_millis(super::super::FINALIZE_LIMIT_MS));
  }
}
