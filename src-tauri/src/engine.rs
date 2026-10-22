//! Per-session dictation engine: native capture + provider stream, reporting to the session
//! actor. The engine never decides lifecycle; it only reports events tagged with its session id.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::audio::capture::{Capture, CaptureConfig, CaptureOutputs, CaptureStats, FrameSink};
use crate::audio::levels::{LevelFrame, LevelMeter};
use crate::providers::{AudioMsg, Registry, StreamCtx, SttEvent};
use crate::session::machine::{EngineError, EngineEvent, Input, SessionId, Trigger};
use crate::session::SessionHandle;

/// Provider audio queue: 100 ms messages, so 3000 is five minutes of buffered speech while a
/// slow connection catches up.
const PROVIDER_QUEUE: usize = 3_000;

#[derive(Debug, Clone)]
pub struct EngineConfig {
  pub provider: String,
  pub model: String,
  pub language: String,
  pub keyterms: Vec<String>,
  pub capture: CaptureConfig,
}

#[derive(Serialize, Clone)]
pub struct LevelEvent {
  pub session_id: String,
  #[serde(flatten)]
  pub level: LevelFrame,
}

pub type LevelEmitter = Arc<dyn Fn(LevelEvent) + Send + Sync>;
pub type CaptureEndHook = Arc<dyn Fn(SessionId, &CaptureStats) + Send + Sync>;
pub type SinkFactory = Arc<dyn Fn(SessionId) -> Vec<FrameSink> + Send + Sync>;

/// Side channels from the engine to the host. Called off the session actor.
#[derive(Clone)]
pub struct EngineHooks {
  /// Display-rate microphone levels for the Flow Bar.
  pub levels: LevelEmitter,
  /// Capture stopped (finish, cancel, failure). Stats describe the captured audio. All frame
  /// sinks have been dropped (flushed) before this runs.
  pub on_capture_end: CaptureEndHook,
  /// Extra consumers of every processed frame, created per session before capture starts
  /// (the durable local recording).
  pub frame_sinks: SinkFactory,
}

impl EngineHooks {
  pub fn none() -> Self {
    Self {
      levels: Arc::new(|_| {}),
      on_capture_end: Arc::new(|_, _| {}),
      frame_sinks: Arc::new(|_| Vec::new()),
    }
  }
}

pub struct EngineHandle {
  pub session_id: SessionId,
  finish: Option<oneshot::Sender<()>>,
  cancel: CancellationToken,
}

impl EngineHandle {
  pub fn finish(&mut self) {
    if let Some(tx) = self.finish.take() {
      let _ = tx.send(());
    }
  }

  pub fn cancel(&self) {
    self.cancel.cancel();
  }
}

pub fn start(
  registry: &Registry,
  cfg: EngineConfig,
  id: SessionId,
  session: SessionHandle,
  hooks: EngineHooks,
) -> EngineHandle {
  let (finish_tx, finish_rx) = oneshot::channel();
  let cancel = CancellationToken::new();
  let handle = EngineHandle {
    session_id: id,
    finish: Some(finish_tx),
    cancel: cancel.clone(),
  };
  let adapter = registry.get(&cfg.provider);
  tauri::async_runtime::spawn(run(adapter, cfg, id, session, hooks, finish_rx, cancel));
  handle
}

async fn report(session: &SessionHandle, id: SessionId, ev: EngineEvent) {
  session.send(Input::Engine(id, ev)).await;
}

async fn run(
  adapter: Option<Arc<dyn crate::providers::SttAdapter>>,
  cfg: EngineConfig,
  id: SessionId,
  session: SessionHandle,
  hooks: EngineHooks,
  mut finish_rx: oneshot::Receiver<()>,
  cancel: CancellationToken,
) {
  let Some(adapter) = adapter else {
    return report(
      &session,
      id,
      EngineEvent::Failed(EngineError::Provider("unknown provider".into())),
    )
    .await;
  };
  let options = match adapter
    .descriptor()
    .resolve(&cfg.model, &cfg.language, cfg.keyterms.clone())
  {
    Ok(o) => o,
    Err(e) => return report(&session, id, EngineEvent::Failed(e.to_engine())).await,
  };

  // 1. Microphone first: speech is captured and buffered before the provider connects.
  let _activity = adapter.activity_guard();
  let (audio_tx, audio_rx) = mpsc::channel::<AudioMsg>(PROVIDER_QUEUE);
  let levels = Arc::new(LevelMeter::default());
  let lost_session = session.clone();
  let outputs = CaptureOutputs {
    provider: Some(audio_tx.clone()),
    levels: levels.clone(),
    frame_sinks: (hooks.frame_sinks)(id),
    on_device_lost: Box::new(move || {
      tracing::warn!("microphone disconnected; finishing dictation");
      lost_session.try_send(Input::Finish(Trigger::System));
    }),
  };
  let capture_cfg = cfg.capture.clone();
  let capture = match tauri::async_runtime::spawn_blocking(move || Capture::start(capture_cfg, outputs)).await {
    Ok(Ok(c)) => c,
    Ok(Err(e)) => {
      tracing::warn!(error = %e, "microphone failed to open");
      return report(&session, id, EngineEvent::Failed(EngineError::Microphone)).await;
    }
    Err(_) => return report(&session, id, EngineEvent::Failed(EngineError::Microphone)).await,
  };
  if cancel.is_cancelled() {
    if let Ok(stats) = tauri::async_runtime::spawn_blocking(move || capture.stop()).await {
      (hooks.on_capture_end)(id, &stats);
    }
    return;
  }
  report(&session, id, EngineEvent::Ready).await;

  // 2. Provider stream.
  let (events_tx, mut events_rx) = mpsc::channel::<(SessionId, SttEvent)>(256);
  let provider_cancel = cancel.child_token();
  adapter.spawn(StreamCtx {
    session_id: id,
    options,
    audio: audio_rx,
    events: events_tx,
    cancel: provider_cancel.clone(),
  });

  // 3. Level pump at ~30 fps.
  let level_cancel = cancel.child_token();
  let pump_levels = levels.clone();
  let pump_cancel = level_cancel.clone();
  let levels_out = hooks.levels.clone();
  tauri::async_runtime::spawn(async move {
    let mut tick = tokio::time::interval(Duration::from_millis(33));
    loop {
      tokio::select! {
        _ = pump_cancel.cancelled() => break,
        _ = tick.tick() => {
          if let Some(level) = pump_levels.take() {
            levels_out(LevelEvent { session_id: id.to_string(), level });
          }
        }
      }
    }
  });

  let mut capture = Some(capture);
  let capture_loss = Arc::new(AtomicBool::new(false));
  let loss = capture_loss.clone();
  let on_end = hooks.on_capture_end.clone();
  let stop_capture = |c: &mut Option<Capture>| {
    let c = c.take();
    let on_end = on_end.clone();
    let loss = loss.clone();
    async move {
      if let Some(c) = c {
        if let Ok(stats) = tauri::async_runtime::spawn_blocking(move || c.stop()).await {
          loss.store(stats.provider_drops > 0 || stats.ring_overruns > 0, Ordering::SeqCst);
          on_end(id, &stats);
        }
      }
    }
  };

  let mut finishing = false;
  loop {
    tokio::select! {
      _ = cancel.cancelled() => {
        stop_capture(&mut capture).await;
        break;
      }
      _ = &mut finish_rx, if !finishing => {
        finishing = true;
        // Drain every captured sample into the provider queue, then mark the end.
        stop_capture(&mut capture).await;
        level_cancel.cancel();
        if capture_loss.load(Ordering::SeqCst) {
          report(&session, id, EngineEvent::Failed(EngineError::Incomplete)).await;
          break;
        }
        tokio::select! {
          _ = cancel.cancelled() => break,
          _ = audio_tx.send(AudioMsg::Finish) => {},
        }
      }
      ev = events_rx.recv() => match ev {
        Some((sid, ev)) if sid == id => match ev {
          SttEvent::Connected => report(&session, id, EngineEvent::Connected).await,
          SttEvent::Reconnecting => report(&session, id, EngineEvent::Reconnecting).await,
          SttEvent::Interim(t) => report(&session, id, EngineEvent::Interim(t)).await,
          SttEvent::Final(t) => report(&session, id, EngineEvent::Final(t)).await,
          SttEvent::Finalized => {
            report(&session, id, if capture_loss.load(Ordering::SeqCst) { EngineEvent::Failed(EngineError::Incomplete) } else { EngineEvent::Finalized }).await;
            break;
          }
          SttEvent::Failed(e) => {
            tracing::warn!(error = %e, "provider stream failed");
            stop_capture(&mut capture).await;
            report(&session, id, EngineEvent::Failed(e.to_engine())).await;
            break;
          }
        },
        Some(_) => {} // never from another session; ignored defensively
        None => {
          stop_capture(&mut capture).await;
          report(&session, id, EngineEvent::Failed(EngineError::Lost)).await;
          break;
        }
      }
    }
  }
  level_cancel.cancel();
  provider_cancel.cancel();
  stop_capture(&mut capture).await;
}
