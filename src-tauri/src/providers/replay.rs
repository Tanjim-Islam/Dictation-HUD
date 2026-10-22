//! Transcribes stored audio through a provider's normal streaming adapter.
//!
//! Retry and recovery reuse the live path instead of provider-specific batch APIs, so every
//! registered adapter (including pooled-key failover) works unchanged. Audio is sent a few
//! times faster than real time.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::{AudioMsg, ProviderError, StreamCtx, SttAdapter, SttEvent, SttOptions, SAMPLE_RATE};
use crate::session::machine::SessionId;

/// 100 ms messages, like live capture.
const CHUNK: usize = SAMPLE_RATE as usize / 10;

#[derive(Debug, Clone, Copy)]
pub struct Pace {
  /// How many times faster than real time to send audio (0 = as fast as possible).
  pub speed: u32,
  /// Wait for finalization after the last chunk.
  pub finalize_timeout: Duration,
}

impl Default for Pace {
  fn default() -> Self {
    Self {
      speed: 4,
      finalize_timeout: Duration::from_secs(20),
    }
  }
}

/// Sends `pcm` (16 kHz mono) through `adapter` and returns the joined final segments.
pub async fn transcribe(
  adapter: Arc<dyn SttAdapter>,
  options: SttOptions,
  pcm: &[i16],
  pace: Pace,
  cancel: CancellationToken,
) -> Result<String, ProviderError> {
  let id = SessionId::new_v4();
  let (audio_tx, audio_rx) = mpsc::channel(pcm.len() / CHUNK + 2);
  let (events_tx, mut events_rx) = mpsc::channel(256);
  let stream_cancel = cancel.child_token();
  adapter.spawn(StreamCtx {
    session_id: id,
    options,
    audio: audio_rx,
    events: events_tx,
    cancel: stream_cancel.clone(),
  });
  // Stop the provider stream however this function returns.
  let _guard = stream_cancel.clone().drop_guard();

  let interval = if pace.speed == 0 {
    Duration::ZERO
  } else {
    Duration::from_millis(100 / pace.speed as u64)
  };
  let feeder = {
    let pcm: Vec<i16> = pcm.to_vec();
    let cancel = stream_cancel.clone();
    tauri::async_runtime::spawn(async move {
      for chunk in pcm.chunks(CHUNK) {
        if cancel.is_cancelled() || audio_tx.send(AudioMsg::Pcm(chunk.to_vec())).await.is_err() {
          return;
        }
        if !interval.is_zero() {
          tokio::time::sleep(interval).await;
        }
      }
      let _ = audio_tx.send(AudioMsg::Finish).await;
    })
  };

  let send_time = interval * (pcm.len() / CHUNK + 1) as u32;
  // Providers that confirm audio in checkpoints (ElevenLabs: every 20 s of audio) pause for each
  // acknowledgment, so long recordings get a few seconds per 20 s on top of the send time.
  let checkpoints = pcm.len() / (20 * SAMPLE_RATE as usize) + 1;
  let ack_allowance = Duration::from_secs(3) * checkpoints as u32;
  let deadline = tokio::time::Instant::now() + send_time + ack_allowance + pace.finalize_timeout;
  let hard_deadline = deadline + Duration::from_millis(super::RECOVERY_BUDGET_MS);
  let mut recovering = false;
  let mut finals: Vec<String> = Vec::new();
  let result = loop {
    tokio::select! {
      _ = cancel.cancelled() => break Err(ProviderError::Protocol("cancelled".into())),
      _ = tokio::time::sleep_until(if recovering { hard_deadline } else { deadline }) => break Err(ProviderError::Timeout),
      ev = events_rx.recv() => match ev {
        Some((sid, _)) if sid != id => {}
        Some((_, SttEvent::Final(text))) => {
          let t = text.trim();
          if !t.is_empty() {
            finals.push(t.to_string());
          }
        }
        Some((_, SttEvent::Finalized)) => break Ok(finals.join(" ")),
        // Failover to another pooled key: give it time to take the replayed audio.
        Some((_, SttEvent::Reconnecting)) => {
          recovering = true;
        }
        Some((_, SttEvent::Failed(e))) => break Err(e),
        Some(_) => {}
        None => break Err(ProviderError::Protocol("stream ended".into())),
      }
    }
  };
  feeder.abort();
  result
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::providers::mock::{MockAdapter, MockBehavior};

  fn opts() -> SttOptions {
    SttOptions {
      model: "mock-1".into(),
      language: "en".into(),
      keyterms: vec![],
    }
  }

  fn fast() -> Pace {
    Pace {
      speed: 0,
      finalize_timeout: Duration::from_millis(500),
    }
  }

  #[tokio::test]
  async fn joins_final_segments() {
    let adapter = Arc::new(MockAdapter::new(
      "m",
      MockBehavior::Transcribe(vec!["hello".into(), "world".into()]),
    ));
    let pcm = vec![0i16; CHUNK * 3];
    let text = transcribe(adapter, opts(), &pcm, fast(), CancellationToken::new())
      .await
      .unwrap();
    assert_eq!(text, "hello world");
  }

  #[tokio::test]
  async fn reports_provider_failure() {
    let adapter = Arc::new(MockAdapter::new("m", MockBehavior::FailMidStream(ProviderError::Auth)));
    let err = transcribe(adapter, opts(), &vec![0i16; CHUNK], fast(), CancellationToken::new())
      .await
      .unwrap_err();
    assert_eq!(err, ProviderError::Auth);
  }

  #[tokio::test]
  async fn times_out_when_provider_never_finalizes() {
    let adapter = Arc::new(MockAdapter::new("m", MockBehavior::HangOnFinish));
    let err = transcribe(adapter, opts(), &vec![0i16; CHUNK], fast(), CancellationToken::new())
      .await
      .unwrap_err();
    assert_eq!(err, ProviderError::Timeout);
  }
}
