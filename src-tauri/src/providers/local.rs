//! Local speech to text through the bundled helper (`dictation-asr.exe`). Audio never leaves this
//! computer, and a failure never falls back to a cloud provider: the session fails, the recording
//! stays, and Retry from History can run it again.

use std::path::PathBuf;
use std::sync::Arc;

use super::{AudioMsg, ModelInfo, ProviderDescriptor, ProviderError, StreamCtx, SttAdapter, SttEvent};
use crate::local_asr::catalog::{self, CATALOG, DEFAULT_MODEL};
use crate::local_asr::client::{Helper, HelperError, LoadSpec};

pub trait ModelLocator: Send + Sync {
  /// Folder of the model when it is installed, linked, or found in the Hugging Face cache.
  fn model_dir(&self, id: &str) -> Option<PathBuf>;
  /// The bundled Silero VAD model.
  fn vad_path(&self) -> Option<PathBuf>;
  fn enough_memory(&self, model_bytes: u64) -> bool;
}

pub struct LocalAdapter {
  helper: Arc<Helper>,
  locator: Arc<dyn ModelLocator>,
}

impl LocalAdapter {
  pub fn new(helper: Arc<Helper>, locator: Arc<dyn ModelLocator>) -> Self {
    Self { helper, locator }
  }
}

/// App language code to the model's: `multi` means auto-detect (empty), regions are dropped.
pub fn model_language(app_code: &str) -> String {
  if app_code == "multi" {
    return String::new();
  }
  app_code.split('-').next().unwrap_or_default().to_string()
}

fn failed(e: HelperError) -> SttEvent {
  SttEvent::Failed(ProviderError::Protocol(e.kind()))
}

async fn emit_finals(ctx: &StreamCtx, segments: Vec<String>) {
  for s in segments {
    let t = s.trim();
    if !t.is_empty() {
      ctx.emit(SttEvent::Final(t.to_string())).await;
    }
  }
}

impl SttAdapter for LocalAdapter {
  fn descriptor(&self) -> ProviderDescriptor {
    ProviderDescriptor {
      id: "local".into(),
      name: "Local".into(),
      models: CATALOG
        .iter()
        .map(|m| ModelInfo {
          id: m.id.into(),
          name: m.name.into(),
          languages: m.languages.iter().map(|l| l.to_string()).collect(),
          keyterms: false,
        })
        .collect(),
      default_model: DEFAULT_MODEL.into(),
      multi_key: false,
    }
  }

  fn configured(&self) -> bool {
    CATALOG.iter().any(|m| self.locator.model_dir(m.id).is_some())
  }

  fn spawn(&self, mut ctx: StreamCtx) {
    let helper = self.helper.clone();
    let locator = self.locator.clone();
    tauri::async_runtime::spawn(async move {
      // Audio buffers in the channel while the model loads, like a socket that is still opening.
      ctx.emit(SttEvent::Connected).await;
      let Some(model) = catalog::get(&ctx.options.model) else {
        return ctx.emit(failed(HelperError::NotInstalled)).await;
      };
      let (Some(dir), Some(vad)) = (locator.model_dir(model.id), locator.vad_path()) else {
        return ctx.emit(failed(HelperError::NotInstalled)).await;
      };
      if !locator.enough_memory(model.download_bytes()) {
        return ctx.emit(failed(HelperError::Oom)).await;
      }
      let spec = LoadSpec {
        kind: model.kind,
        files: model.files_in(&dir),
        vad,
        language: model_language(&ctx.options.language),
      };
      let mut session = tokio::select! {
        _ = ctx.cancel.cancelled() => return,
        s = helper.session(&spec) => match s {
          Ok(s) => s,
          Err(e) => {
            tracing::warn!(session = %ctx.session_id, model = model.id, error = %e.kind(), "local model unavailable");
            return ctx.emit(failed(e)).await;
          }
        },
      };
      loop {
        let msg = tokio::select! {
          _ = ctx.cancel.cancelled() => return,
          m = ctx.audio.recv() => m,
        };
        match msg {
          Some(AudioMsg::Pcm(pcm)) => match session.audio(&pcm).await {
            Ok(segments) => emit_finals(&ctx, segments).await,
            Err(e) => return ctx.emit(failed(e)).await,
          },
          Some(AudioMsg::Finish) | None => {
            return match session.flush().await {
              Ok(segments) => {
                emit_finals(&ctx, segments).await;
                ctx.emit(SttEvent::Finalized).await
              }
              Err(e) => ctx.emit(failed(e)).await,
            };
          }
        }
      }
    });
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn language_codes_map_to_model_codes() {
    assert_eq!(model_language("multi"), "");
    assert_eq!(model_language("en-US"), "en");
    assert_eq!(model_language("zh-CN"), "zh");
    assert_eq!(model_language("ja"), "ja");
  }
}
