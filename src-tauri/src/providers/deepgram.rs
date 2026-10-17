//! Deepgram live streaming adapter.
//! Docs: https://developers.deepgram.com/reference/speech-to-text/listen-streaming

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

use super::{
  languages, pcm_bytes, AudioMsg, ModelInfo, ProviderDescriptor, ProviderError, StreamCtx, SttAdapter, SttEvent,
  SttOptions, SAMPLE_RATE,
};

pub type KeySource = Arc<dyn Fn() -> Option<String> + Send + Sync>;

pub fn descriptor() -> ProviderDescriptor {
  ProviderDescriptor {
    id: "deepgram".into(),
    name: "Deepgram".into(),
    models: vec![
      ModelInfo {
        id: "nova-3".into(),
        name: "Nova-3".into(),
        languages: languages::DEEPGRAM_NOVA3.iter().map(|s| s.to_string()).collect(),
        keyterms: true,
      },
      ModelInfo {
        id: "nova-2".into(),
        name: "Nova-2".into(),
        languages: languages::DEEPGRAM_NOVA2.iter().map(|s| s.to_string()).collect(),
        keyterms: false,
      },
    ],
    default_model: "nova-3".into(),
    multi_key: false,
  }
}

pub struct Deepgram {
  key: KeySource,
  base_url: String,
}

impl Deepgram {
  pub fn new(key: KeySource) -> Self {
    Self {
      key,
      base_url: "wss://api.deepgram.com/v1/listen".into(),
    }
  }

  #[cfg(test)]
  pub fn with_base_url(key: KeySource, base_url: String) -> Self {
    Self { key, base_url }
  }
}

pub fn build_url(base: &str, o: &SttOptions) -> String {
  let mut url = url::Url::parse(base).expect("static base url");
  {
    let mut q = url.query_pairs_mut();
    q.append_pair("model", &o.model)
      .append_pair("language", &o.language)
      .append_pair("encoding", "linear16")
      .append_pair("sample_rate", &SAMPLE_RATE.to_string())
      .append_pair("channels", "1")
      .append_pair("interim_results", "true")
      .append_pair("punctuate", "true")
      .append_pair("smart_format", "true")
      .append_pair("endpointing", "300");
    // Keyterm prompting (nova-3): repeated param, 500 token budget. Keep a safe subset.
    let mut budget = 0usize;
    for term in &o.keyterms {
      let cost = term.split_whitespace().count() + 1;
      if budget + cost > 400 {
        break;
      }
      budget += cost;
      q.append_pair("keyterm", term);
    }
  }
  url.to_string()
}

#[derive(Debug, PartialEq)]
pub enum Parsed {
  Transcript { text: String, is_final: bool },
  Metadata,
  Error(String),
  Other,
}

pub fn parse_message(text: &str) -> Parsed {
  let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
    return Parsed::Other;
  };
  match v.get("type").and_then(|t| t.as_str()) {
    Some("Results") => Parsed::Transcript {
      text: v["channel"]["alternatives"][0]["transcript"]
        .as_str()
        .unwrap_or("")
        .to_string(),
      // is_final: this span will not change. speech_final only marks a pause.
      is_final: v["is_final"].as_bool().unwrap_or(false),
    },
    Some("Metadata") => Parsed::Metadata,
    Some("Error") => Parsed::Error(
      v["variant"]
        .as_str()
        .or(v["err_code"].as_str())
        .unwrap_or("error")
        .to_string(),
    ),
    _ => Parsed::Other,
  }
}

fn map_ws_error(e: &tokio_tungstenite::tungstenite::Error) -> ProviderError {
  use tokio_tungstenite::tungstenite::Error as E;
  match e {
    E::Http(resp) => ProviderError::from_status(resp.status().as_u16()),
    E::Io(_) | E::Tls(_) | E::ConnectionClosed | E::AlreadyClosed => ProviderError::Network,
    _ => ProviderError::Protocol("handshake".into()),
  }
}

impl SttAdapter for Deepgram {
  fn descriptor(&self) -> ProviderDescriptor {
    descriptor()
  }

  fn configured(&self) -> bool {
    (self.key)().is_some()
  }

  fn spawn(&self, ctx: StreamCtx) {
    let key = (self.key)();
    let base = self.base_url.clone();
    tauri::async_runtime::spawn(async move {
      match key {
        None => ctx.emit(SttEvent::Failed(ProviderError::MissingKey)).await,
        Some(key) => run(base, key, ctx).await,
      }
    });
  }
}

async fn run(base: String, key: String, mut ctx: StreamCtx) {
  let url = build_url(&base, &ctx.options);
  let mut req = match url.as_str().into_client_request() {
    Ok(r) => r,
    Err(_) => {
      return ctx
        .emit(SttEvent::Failed(ProviderError::Protocol("request".into())))
        .await
    }
  };
  match format!("Token {key}").parse() {
    Ok(v) => {
      req.headers_mut().insert("Authorization", v);
    }
    Err(_) => return ctx.emit(SttEvent::Failed(ProviderError::Auth)).await,
  }
  let started = Instant::now();
  let connect = tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::connect_async(req));
  let ws = tokio::select! {
    _ = ctx.cancel.cancelled() => return,
    r = connect => match r {
      Err(_) => return ctx.emit(SttEvent::Failed(ProviderError::Timeout)).await,
      Ok(Err(e)) => {
        let err = map_ws_error(&e);
        tracing::warn!(provider = "deepgram", error = %err, "connect failed");
        return ctx.emit(SttEvent::Failed(err)).await;
      }
      Ok(Ok((ws, _))) => ws,
    },
  };
  tracing::info!(provider = "deepgram", ms = started.elapsed().as_millis() as u64, model = %ctx.options.model, language = %ctx.options.language, "connected");
  ctx.emit(SttEvent::Connected).await;

  let (mut sink, mut stream) = ws.split();
  let mut finishing = false;
  let mut last_send = Instant::now();
  let mut keepalive = tokio::time::interval(Duration::from_secs(4));
  loop {
    tokio::select! {
      _ = ctx.cancel.cancelled() => {
        let _ = sink.send(Message::Close(None)).await;
        return;
      }
      msg = ctx.audio.recv(), if !finishing => match msg {
        Some(AudioMsg::Pcm(pcm)) => {
          if sink.send(Message::Binary(pcm_bytes(&pcm).into())).await.is_err() {
            return ctx.emit(SttEvent::Failed(ProviderError::Network)).await;
          }
          last_send = Instant::now();
        }
        Some(AudioMsg::Finish) | None => {
          // Server flushes, sends final results and Metadata, then closes.
          let _ = sink.send(Message::Text(r#"{"type":"CloseStream"}"#.into())).await;
          finishing = true;
        }
      },
      _ = keepalive.tick() => {
        if !finishing && last_send.elapsed() >= Duration::from_secs(4) {
          let _ = sink.send(Message::Text(r#"{"type":"KeepAlive"}"#.into())).await;
          last_send = Instant::now();
        }
      }
      frame = stream.next() => match frame {
        Some(Ok(Message::Text(t))) => match parse_message(&t) {
          Parsed::Transcript { text, is_final } => {
            ctx.emit(if is_final { SttEvent::Final(text) } else { SttEvent::Interim(text) }).await;
          }
          Parsed::Metadata if finishing => return ctx.emit(SttEvent::Finalized).await,
          Parsed::Error(kind) => {
            tracing::warn!(provider = "deepgram", kind = %super::sanitize_for_log(&kind), "provider error message");
            return ctx.emit(SttEvent::Failed(ProviderError::Protocol(kind.chars().take(40).collect()))).await;
          }
          _ => {}
        },
        Some(Ok(Message::Close(_))) if finishing => return ctx.emit(SttEvent::Finalized).await,
        Some(Ok(Message::Close(frame))) => {
          let code = frame.as_ref().map(|f| u16::from(f.code)).unwrap_or(0);
          tracing::warn!(provider = "deepgram", code, "closed by server");
          let err = if code == 1008 { ProviderError::Auth } else { ProviderError::Network };
          return ctx.emit(SttEvent::Failed(err)).await;
        }
        None | Some(Err(_)) => {
          return ctx.emit(if finishing { SttEvent::Finalized } else { SttEvent::Failed(ProviderError::Network) }).await;
        }
        _ => {}
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn opts(model: &str, lang: &str, terms: &[&str]) -> SttOptions {
    SttOptions {
      model: model.into(),
      language: lang.into(),
      keyterms: terms.iter().map(|s| s.to_string()).collect(),
    }
  }

  #[test]
  fn url_carries_model_language_and_keyterms() {
    let u = build_url(
      "wss://api.deepgram.com/v1/listen",
      &opts("nova-3", "bn", &["City Bank", "src-tauri"]),
    );
    assert!(u.contains("model=nova-3"));
    assert!(u.contains("language=bn"));
    assert!(u.contains("encoding=linear16"));
    assert!(u.contains("sample_rate=16000"));
    assert!(u.contains("keyterm=City+Bank"));
    assert!(u.contains("keyterm=src-tauri"));
    assert!(!u.contains("token"), "credentials never go in the URL");
  }

  #[test]
  fn keyterm_budget_is_enforced() {
    let many: Vec<String> = (0..500).map(|i| format!("term{i}")).collect();
    let o = SttOptions {
      model: "nova-3".into(),
      language: "en".into(),
      keyterms: many,
    };
    let u = build_url("wss://x.test/v1/listen", &o);
    assert!(u.matches("keyterm=").count() <= 200);
  }

  #[test]
  fn parses_results_using_is_final() {
    let m = r#"{"type":"Results","is_final":true,"speech_final":false,"channel":{"alternatives":[{"transcript":"hello world"}]}}"#;
    assert_eq!(
      parse_message(m),
      Parsed::Transcript {
        text: "hello world".into(),
        is_final: true
      }
    );
    let m =
      r#"{"type":"Results","is_final":false,"speech_final":true,"channel":{"alternatives":[{"transcript":"hel"}]}}"#;
    assert_eq!(
      parse_message(m),
      Parsed::Transcript {
        text: "hel".into(),
        is_final: false
      }
    );
    assert_eq!(
      parse_message(r#"{"type":"Metadata","request_id":"x"}"#),
      Parsed::Metadata
    );
    assert_eq!(parse_message("not json"), Parsed::Other);
    assert!(matches!(
      parse_message(r#"{"type":"Error","variant":"DATA-0000"}"#),
      Parsed::Error(_)
    ));
  }
}
