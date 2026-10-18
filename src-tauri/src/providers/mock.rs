//! Deterministic adapter for tests (and a reference for writing new providers).

use super::{AudioMsg, ModelInfo, ProviderDescriptor, ProviderError, StreamCtx, SttAdapter, SttEvent};

#[derive(Clone, Debug)]
pub enum MockBehavior {
  /// Emits each phrase as interim then final when audio arrives, then flushes on finish.
  Transcribe(Vec<String>),
  /// Fails right after connecting.
  FailOnConnect(ProviderError),
  /// Fails after the first audio frame.
  FailMidStream(ProviderError),
  /// Never answers the finish request (exercises finalize timeouts).
  HangOnFinish,
}

pub struct MockAdapter {
  pub id: String,
  pub behavior: MockBehavior,
}

impl MockAdapter {
  pub fn new(id: &str, behavior: MockBehavior) -> Self {
    Self {
      id: id.into(),
      behavior,
    }
  }
}

impl SttAdapter for MockAdapter {
  fn descriptor(&self) -> ProviderDescriptor {
    ProviderDescriptor {
      id: self.id.clone(),
      name: format!("Mock {}", self.id),
      models: vec![ModelInfo {
        id: "mock-1".into(),
        name: "Mock".into(),
        languages: vec!["en".into(), "bn".into(), "multi".into()],
        keyterms: true,
      }],
      default_model: "mock-1".into(),
      multi_key: false,
    }
  }

  fn configured(&self) -> bool {
    true
  }

  fn spawn(&self, mut ctx: StreamCtx) {
    let behavior = self.behavior.clone();
    tauri::async_runtime::spawn(async move {
      if let MockBehavior::FailOnConnect(e) = &behavior {
        return ctx.emit(SttEvent::Failed(e.clone())).await;
      }
      ctx.emit(SttEvent::Connected).await;
      let mut phrases = match &behavior {
        MockBehavior::Transcribe(p) => p.clone().into_iter(),
        _ => Vec::new().into_iter(),
      };
      loop {
        tokio::select! {
          _ = ctx.cancel.cancelled() => return,
          msg = ctx.audio.recv() => match msg {
            Some(AudioMsg::Pcm(_)) => {
              if let MockBehavior::FailMidStream(e) = &behavior {
                return ctx.emit(SttEvent::Failed(e.clone())).await;
              }
              if let Some(p) = phrases.next() {
                ctx.emit(SttEvent::Interim(p.clone())).await;
                ctx.emit(SttEvent::Final(p)).await;
              }
            }
            Some(AudioMsg::Finish) | None => {
              if matches!(behavior, MockBehavior::HangOnFinish) {
                ctx.cancel.cancelled().await;
                return;
              }
              for p in phrases.by_ref() {
                ctx.emit(SttEvent::Final(p)).await;
              }
              return ctx.emit(SttEvent::Finalized).await;
            }
          }
        }
      }
    });
  }
}
