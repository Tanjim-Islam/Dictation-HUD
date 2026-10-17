use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use super::host::Host;
use super::machine::{Effect, Input, Machine, Snapshot};

enum Msg {
  Input(Input),
  Snapshot(oneshot::Sender<Snapshot>),
  Shutdown(oneshot::Sender<()>),
}

/// Cheap, cloneable handle to the session actor.
#[derive(Clone)]
pub struct SessionHandle {
  tx: mpsc::Sender<Msg>,
}

impl SessionHandle {
  /// Sends an input from async code. Inputs are dropped only if the actor is gone.
  pub async fn send(&self, input: Input) {
    let _ = self.tx.send(Msg::Input(input)).await;
  }

  /// Sends an input from sync code (hotkey/tray handlers). Under extreme spam a full mailbox
  /// drops the intent, which is the desired behavior for duplicate key presses.
  pub fn try_send(&self, input: Input) {
    if self.tx.try_send(Msg::Input(input)).is_err() {
      tracing::warn!("session mailbox full or closed; intent dropped");
    }
  }

  pub async fn snapshot(&self) -> Option<Snapshot> {
    let (tx, rx) = oneshot::channel();
    self.tx.send(Msg::Snapshot(tx)).await.ok()?;
    rx.await.ok()
  }

  /// Stops any active session safely. Returns when the machine confirms or after `wait`.
  pub async fn shutdown(&self, wait: Duration) {
    let (tx, rx) = oneshot::channel();
    if self.tx.send(Msg::Shutdown(tx)).await.is_ok() {
      let _ = tokio::time::timeout(wait, rx).await;
    }
  }
}

pub fn spawn<H: Host>(host: Arc<H>) -> SessionHandle {
  spawn_with(host, Machine::default())
}

pub fn spawn_with<H: Host>(host: Arc<H>, mut machine: Machine) -> SessionHandle {
  let (tx, mut rx) = mpsc::channel::<Msg>(256);
  let handle = SessionHandle { tx };
  let me = handle.clone();
  tauri::async_runtime::spawn(async move {
    let mut shutdown_waiters: Vec<oneshot::Sender<()>> = Vec::new();
    while let Some(msg) = rx.recv().await {
      let input = match msg {
        Msg::Input(input) => input,
        Msg::Snapshot(reply) => {
          let _ = reply.send(machine.snapshot());
          continue;
        }
        Msg::Shutdown(reply) => {
          shutdown_waiters.push(reply);
          Input::Shutdown
        }
      };
      let label = input_label(&input);
      let before = machine.phase();
      let effects = machine.step(input);
      if machine.phase() != before {
        tracing::info!(from = ?before, to = ?machine.phase(), input = label, session = ?machine.active_id(), "session transition");
      }
      for effect in effects {
        match effect {
          Effect::Publish => host.publish(&machine.snapshot()),
          Effect::StartEngine(id) => host.start_engine(id, &me),
          Effect::FinishEngine(id) => host.finish_engine(id),
          Effect::CancelEngine(id) => host.cancel_engine(id),
          Effect::StartTimer(id, kind) => {
            let h = me.clone();
            tauri::async_runtime::spawn(async move {
              tokio::time::sleep(Duration::from_millis(kind.millis())).await;
              h.send(Input::Timeout(id, kind)).await;
            });
          }
          Effect::Process(id, text) => host.process(id, text, &me),
          Effect::Deliver(id, text) => host.deliver(id, text, &me),
          Effect::Record {
            id,
            outcome,
            transcript,
          } => host.record(id, &outcome, &transcript),
          Effect::ShutdownComplete => {
            for w in shutdown_waiters.drain(..) {
              let _ = w.send(());
            }
          }
        }
      }
    }
  });
  handle
}

fn input_label(input: &Input) -> &'static str {
  match input {
    Input::Start(_) => "start",
    Input::Toggle(_) => "toggle",
    Input::Finish(_) => "finish",
    Input::Cancel(_) => "cancel",
    Input::Shutdown => "shutdown",
    Input::Engine(..) => "engine",
    Input::Processed(..) => "processed",
    Input::Delivered(..) => "delivered",
    Input::Timeout(..) => "timeout",
  }
}

#[cfg(test)]
mod tests {
  use super::super::machine::{DeliveryOutcome, EngineEvent, Outcome, Phase, Processed, SessionId, Trigger};
  use super::*;
  use std::sync::Mutex;

  #[derive(Default)]
  struct FakeHost {
    log: Mutex<Vec<String>>,
  }

  impl FakeHost {
    fn push(&self, s: String) {
      self.log.lock().unwrap().push(s);
    }
    fn entries(&self) -> Vec<String> {
      self.log.lock().unwrap().clone()
    }
  }

  impl Host for FakeHost {
    fn publish(&self, snap: &Snapshot) {
      self.push(format!("publish:{:?}", snap.phase));
    }
    fn start_engine(&self, id: SessionId, session: &SessionHandle) {
      self.push("start".into());
      let s = session.clone();
      tauri::async_runtime::spawn(async move {
        s.send(Input::Engine(id, EngineEvent::Ready)).await;
        s.send(Input::Engine(id, EngineEvent::Final("hello".into()))).await;
      });
    }
    fn finish_engine(&self, _id: SessionId) {
      self.push("finish".into());
    }
    fn cancel_engine(&self, _id: SessionId) {
      self.push("cancel".into());
    }
    fn process(&self, id: SessionId, text: String, session: &SessionHandle) {
      let s = session.clone();
      tauri::async_runtime::spawn(async move {
        s.send(Input::Processed(id, Processed::Text(format!("{text}.")))).await;
      });
    }
    fn deliver(&self, id: SessionId, text: String, session: &SessionHandle) {
      self.push(format!("deliver:{text}"));
      let s = session.clone();
      tauri::async_runtime::spawn(async move {
        s.send(Input::Delivered(id, DeliveryOutcome::Pasted)).await;
      });
    }
    fn record(&self, _id: SessionId, outcome: &Outcome, _t: &str) {
      self.push(format!("record:{outcome:?}"));
    }
  }

  async fn wait_for_phase(h: &SessionHandle, phase: Phase) {
    for _ in 0..200 {
      if h.snapshot().await.map(|s| s.phase) == Some(phase) {
        return;
      }
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("phase {phase:?} not reached");
  }

  #[test]
  fn actor_runs_full_session_through_host() {
    tauri::async_runtime::block_on(async {
      let host = Arc::new(FakeHost::default());
      let h = spawn(host.clone());
      h.send(Input::Toggle(Trigger::Hotkey)).await;
      wait_for_phase(&h, Phase::Listening).await;
      // Hotkey spam while listening: first toggle finishes, the rest are ignored.
      for _ in 0..5 {
        h.try_send(Input::Toggle(Trigger::Hotkey));
      }
      tokio::time::sleep(Duration::from_millis(20)).await;
      // Engine flushed.
      let id = h
        .snapshot()
        .await
        .unwrap()
        .session_id
        .unwrap()
        .parse::<SessionId>()
        .unwrap();
      h.send(Input::Engine(id, EngineEvent::Finalized)).await;
      wait_for_phase(&h, Phase::Idle).await;
      let log = host.entries();
      assert_eq!(log.iter().filter(|e| *e == "start").count(), 1);
      assert_eq!(log.iter().filter(|e| *e == "finish").count(), 1);
      assert!(log.contains(&"deliver:hello.".to_string()));
      assert!(log.iter().any(|e| e.starts_with("record:Completed")));
    });
  }

  #[test]
  fn shutdown_returns_while_idle_and_while_active() {
    tauri::async_runtime::block_on(async {
      let host = Arc::new(FakeHost::default());
      let h = spawn(host.clone());
      h.shutdown(Duration::from_secs(1)).await;
      let h2 = spawn(host.clone());
      h2.send(Input::Start(Trigger::Tray)).await;
      wait_for_phase(&h2, Phase::Listening).await;
      let started = std::time::Instant::now();
      h2.shutdown(Duration::from_secs(1)).await;
      assert!(started.elapsed() < Duration::from_millis(900));
      assert!(host.entries().contains(&"record:Interrupted".to_string()));
    });
  }
}
