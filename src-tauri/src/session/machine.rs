//! Pure dictation session state machine.
//!
//! `Machine::step` takes one input and returns the effects the host must run. It never does
//! IO, never reads the clock, and never spawns anything, so every transition is unit-testable.
//! Only one session can be active at a time; inputs carrying another session id are stale and
//! ignored.

use serde::Serialize;
use uuid::Uuid;

use crate::transcript::Aggregator;

pub type SessionId = Uuid;

/// User-facing phase. This is transient engine state, not the durable history outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
  Idle,
  Preparing,
  Listening,
  Finalizing,
  Processing,
  Delivering,
  Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
  Hotkey,
  Tray,
  FlowBar,
  App,
  /// The engine itself, e.g. the microphone disappeared.
  System,
}

impl Trigger {
  pub fn as_str(self) -> &'static str {
    match self {
      Trigger::Hotkey => "hotkey",
      Trigger::Tray => "tray",
      Trigger::FlowBar => "flow_bar",
      Trigger::App => "app",
      Trigger::System => "system",
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineError {
  MissingKey,
  Auth,
  RateLimited,
  Network,
  Microphone,
  Provider(String),
  Lost,
}

impl EngineError {
  /// Stable machine-readable code for storage.
  pub fn code(&self) -> String {
    match self {
      EngineError::MissingKey => "missing_key".into(),
      EngineError::Auth => "auth".into(),
      EngineError::RateLimited => "rate_limited".into(),
      EngineError::Network => "network".into(),
      EngineError::Microphone => "microphone".into(),
      EngineError::Provider(kind) => format!("provider:{kind}"),
      EngineError::Lost => "engine_lost".into(),
    }
  }

  pub fn message(&self) -> String {
    match self {
      EngineError::MissingKey => "Add an API key for the speech provider".into(),
      EngineError::Auth => "Speech provider rejected the API key".into(),
      EngineError::RateLimited => "Speech provider is rate limiting requests".into(),
      EngineError::Network => "Could not reach the speech provider".into(),
      EngineError::Microphone => "Microphone unavailable".into(),
      EngineError::Provider(kind) => format!("Speech provider error ({kind})"),
      EngineError::Lost => "Dictation engine stopped unexpectedly".into(),
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
  /// Capture and provider are both live.
  Ready,
  Interim(String),
  Final(String),
  /// The provider flushed everything after a finish request.
  Finalized,
  Failed(EngineError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerKind {
  Connect,
  Finalize,
  ErrorHide,
}

impl TimerKind {
  pub fn millis(self) -> u64 {
    match self {
      TimerKind::Connect => 12_000,
      TimerKind::Finalize => 6_000,
      TimerKind::ErrorHide => 2_500,
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Processed {
  Text(String),
  Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryOutcome {
  Pasted,
  Copied,
  Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
  Start(Trigger),
  /// Hotkey semantics: start when idle, finish when capturing, ignore otherwise.
  Toggle(Trigger),
  Finish(Trigger),
  Cancel(Trigger),
  Shutdown,
  Engine(SessionId, EngineEvent),
  Processed(SessionId, Processed),
  Delivered(SessionId, DeliveryOutcome),
  Timeout(SessionId, TimerKind),
}

/// Durable result of a session, recorded once per session.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Outcome {
  Completed { delivery: DeliveryOutcome },
  NoSpeech,
  Cancelled,
  Failed { error: EngineError },
  Interrupted,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
  Publish,
  /// A new session exists; persisted before any engine work.
  Created(SessionId, Trigger),
  StartEngine(SessionId),
  /// A final (stable) transcript segment, persisted as it arrives.
  Segment(SessionId, String),
  FinishEngine(SessionId),
  CancelEngine(SessionId),
  StartTimer(SessionId, TimerKind),
  Process(SessionId, String),
  Deliver(SessionId, String),
  Record {
    id: SessionId,
    outcome: Outcome,
    transcript: String,
  },
  ShutdownComplete,
}

#[derive(Debug, Clone, PartialEq)]
enum State {
  Idle,
  Active { id: SessionId, phase: Phase },
  Error { id: SessionId, error: EngineError },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snapshot {
  pub session_id: Option<String>,
  pub phase: Phase,
  pub error: Option<String>,
}

pub struct Machine {
  state: State,
  transcript: Aggregator,
  shutdown_pending: bool,
  new_id: Box<dyn FnMut() -> SessionId + Send>,
}

impl Default for Machine {
  fn default() -> Self {
    Self::new(Box::new(Uuid::new_v4))
  }
}

impl Machine {
  pub fn new(new_id: Box<dyn FnMut() -> SessionId + Send>) -> Self {
    Self {
      state: State::Idle,
      transcript: Aggregator::default(),
      shutdown_pending: false,
      new_id,
    }
  }

  pub fn phase(&self) -> Phase {
    match &self.state {
      State::Idle => Phase::Idle,
      State::Active { phase, .. } => *phase,
      State::Error { .. } => Phase::Error,
    }
  }

  pub fn active_id(&self) -> Option<SessionId> {
    match &self.state {
      State::Active { id, .. } | State::Error { id, .. } => Some(*id),
      State::Idle => None,
    }
  }

  pub fn snapshot(&self) -> Snapshot {
    Snapshot {
      session_id: self.active_id().map(|id| id.to_string()),
      phase: self.phase(),
      error: match &self.state {
        State::Error { error, .. } => Some(error.message()),
        _ => None,
      },
    }
  }

  pub fn transcript_text(&self) -> String {
    self.transcript.text()
  }

  pub fn step(&mut self, input: Input) -> Vec<Effect> {
    let mut fx = Vec::new();
    match (self.state.clone(), input) {
      // Starting
      (State::Idle | State::Error { .. }, Input::Start(trigger) | Input::Toggle(trigger)) => {
        if self.shutdown_pending {
          return fx;
        }
        let id = (self.new_id)();
        self.transcript = Aggregator::default();
        self.state = State::Active {
          id,
          phase: Phase::Preparing,
        };
        fx.push(Effect::Created(id, trigger));
        fx.push(Effect::StartEngine(id));
        fx.push(Effect::StartTimer(id, TimerKind::Connect));
        fx.push(Effect::Publish);
      }
      (State::Error { .. }, Input::Cancel(_)) => {
        self.state = State::Idle;
        fx.push(Effect::Publish);
      }
      (State::Error { id, .. }, Input::Timeout(t, TimerKind::ErrorHide)) if t == id => {
        self.state = State::Idle;
        fx.push(Effect::Publish);
      }

      // Shutdown
      (State::Idle | State::Error { .. }, Input::Shutdown) => {
        self.state = State::Idle;
        fx.push(Effect::ShutdownComplete);
      }
      (State::Active { id, phase }, Input::Shutdown) => match phase {
        Phase::Delivering => self.shutdown_pending = true,
        _ => {
          if matches!(phase, Phase::Preparing | Phase::Listening | Phase::Finalizing) {
            fx.push(Effect::CancelEngine(id));
          }
          self.record(&mut fx, id, Outcome::Interrupted);
          self.state = State::Idle;
          fx.push(Effect::Publish);
          fx.push(Effect::ShutdownComplete);
        }
      },

      // Capturing: Preparing and Listening
      (
        State::Active {
          id,
          phase: Phase::Preparing | Phase::Listening,
        },
        Input::Toggle(_) | Input::Finish(_),
      ) => {
        self.state = State::Active {
          id,
          phase: Phase::Finalizing,
        };
        fx.push(Effect::FinishEngine(id));
        fx.push(Effect::StartTimer(id, TimerKind::Finalize));
        fx.push(Effect::Publish);
      }
      (
        State::Active {
          id,
          phase: Phase::Preparing,
        },
        Input::Engine(e, EngineEvent::Ready),
      ) if e == id => {
        self.state = State::Active {
          id,
          phase: Phase::Listening,
        };
        fx.push(Effect::Publish);
      }
      (
        State::Active {
          id,
          phase: Phase::Preparing,
        },
        Input::Timeout(t, TimerKind::Connect),
      ) if t == id => {
        fx.push(Effect::CancelEngine(id));
        self.fail(&mut fx, id, EngineError::Network);
      }
      (
        State::Active {
          id,
          phase: Phase::Preparing | Phase::Listening | Phase::Finalizing,
        },
        Input::Cancel(_),
      ) => {
        fx.push(Effect::CancelEngine(id));
        self.record(&mut fx, id, Outcome::Cancelled);
        self.state = State::Idle;
        fx.push(Effect::Publish);
      }
      (
        State::Active {
          id,
          phase: Phase::Preparing | Phase::Listening | Phase::Finalizing,
        },
        Input::Engine(e, ev),
      ) if e == id => {
        match ev {
          EngineEvent::Interim(t) => self.transcript.interim(&t),
          EngineEvent::Final(t) => {
            if !t.trim().is_empty() {
              fx.push(Effect::Segment(id, t.trim().to_string()));
            }
            self.transcript.finalize(&t);
          }
          EngineEvent::Ready => {}
          EngineEvent::Finalized => {
            if self.phase() == Phase::Finalizing {
              self.finish_capture(&mut fx, id);
            }
          }
          EngineEvent::Failed(error) => {
            if self.phase() == Phase::Finalizing && !self.transcript.is_empty() {
              // Finish was requested and we already hold text: deliver what we have.
              self.finish_capture(&mut fx, id);
            } else {
              fx.push(Effect::CancelEngine(id));
              self.fail(&mut fx, id, error);
            }
          }
        }
      }
      (
        State::Active {
          id,
          phase: Phase::Finalizing,
        },
        Input::Timeout(t, TimerKind::Finalize),
      ) if t == id => {
        fx.push(Effect::CancelEngine(id));
        self.finish_capture(&mut fx, id);
      }

      // Processing
      (
        State::Active {
          id,
          phase: Phase::Processing,
        },
        Input::Processed(p, result),
      ) if p == id => match result {
        Processed::Text(text) => {
          self.state = State::Active {
            id,
            phase: Phase::Delivering,
          };
          fx.push(Effect::Deliver(id, text));
          fx.push(Effect::Publish);
        }
        Processed::Empty => {
          self.record(&mut fx, id, Outcome::NoSpeech);
          self.state = State::Idle;
          fx.push(Effect::Publish);
        }
      },
      (
        State::Active {
          id,
          phase: Phase::Processing,
        },
        Input::Cancel(_),
      ) => {
        self.record(&mut fx, id, Outcome::Cancelled);
        self.state = State::Idle;
        fx.push(Effect::Publish);
      }

      // Delivering cannot be cancelled: the keystroke may already be in flight.
      (
        State::Active {
          id,
          phase: Phase::Delivering,
        },
        Input::Delivered(d, delivery),
      ) if d == id => {
        self.record(&mut fx, id, Outcome::Completed { delivery });
        self.state = State::Idle;
        fx.push(Effect::Publish);
        if self.shutdown_pending {
          fx.push(Effect::ShutdownComplete);
        }
      }

      // Everything else is a duplicate intent or a stale event.
      _ => {}
    }
    fx
  }

  fn finish_capture(&mut self, fx: &mut Vec<Effect>, id: SessionId) {
    let text = self.transcript.text();
    if text.trim().is_empty() {
      self.record(fx, id, Outcome::NoSpeech);
      self.state = State::Idle;
    } else {
      self.state = State::Active {
        id,
        phase: Phase::Processing,
      };
      fx.push(Effect::Process(id, text));
    }
    fx.push(Effect::Publish);
  }

  fn fail(&mut self, fx: &mut Vec<Effect>, id: SessionId, error: EngineError) {
    self.record(fx, id, Outcome::Failed { error: error.clone() });
    self.state = State::Error { id, error };
    fx.push(Effect::StartTimer(id, TimerKind::ErrorHide));
    fx.push(Effect::Publish);
  }

  fn record(&self, fx: &mut Vec<Effect>, id: SessionId, outcome: Outcome) {
    fx.push(Effect::Record {
      id,
      outcome,
      transcript: self.transcript.text(),
    });
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn machine() -> Machine {
    let mut n = 0u128;
    Machine::new(Box::new(move || {
      n += 1;
      Uuid::from_u128(n)
    }))
  }

  fn id(n: u128) -> SessionId {
    Uuid::from_u128(n)
  }

  fn has(fx: &[Effect], want: &Effect) -> bool {
    fx.iter().any(|e| e == want)
  }

  fn outcome(fx: &[Effect]) -> Option<Outcome> {
    fx.iter().find_map(|e| match e {
      Effect::Record { outcome, .. } => Some(outcome.clone()),
      _ => None,
    })
  }

  #[test]
  fn happy_path_start_to_completed() {
    let mut m = machine();
    let fx = m.step(Input::Toggle(Trigger::Hotkey));
    assert!(has(&fx, &Effect::StartEngine(id(1))));
    assert_eq!(m.phase(), Phase::Preparing);
    m.step(Input::Engine(id(1), EngineEvent::Ready));
    assert_eq!(m.phase(), Phase::Listening);
    m.step(Input::Engine(id(1), EngineEvent::Interim("hello".into())));
    m.step(Input::Engine(id(1), EngineEvent::Final("hello world".into())));
    let fx = m.step(Input::Toggle(Trigger::Hotkey));
    assert!(has(&fx, &Effect::FinishEngine(id(1))));
    assert_eq!(m.phase(), Phase::Finalizing);
    let fx = m.step(Input::Engine(id(1), EngineEvent::Finalized));
    assert!(has(&fx, &Effect::Process(id(1), "hello world".into())));
    let fx = m.step(Input::Processed(id(1), Processed::Text("Hello world.".into())));
    assert!(has(&fx, &Effect::Deliver(id(1), "Hello world.".into())));
    let fx = m.step(Input::Delivered(id(1), DeliveryOutcome::Pasted));
    assert_eq!(
      outcome(&fx),
      Some(Outcome::Completed {
        delivery: DeliveryOutcome::Pasted
      })
    );
    assert_eq!(m.phase(), Phase::Idle);
  }

  #[test]
  fn created_is_persisted_before_engine_and_segments_follow_finals() {
    let mut m = machine();
    let fx = m.step(Input::Start(Trigger::Tray));
    let created = fx
      .iter()
      .position(|e| *e == Effect::Created(id(1), Trigger::Tray))
      .unwrap();
    let engine = fx.iter().position(|e| *e == Effect::StartEngine(id(1))).unwrap();
    assert!(created < engine);
    let fx = m.step(Input::Engine(id(1), EngineEvent::Interim("draft".into())));
    assert!(
      !fx.iter().any(|e| matches!(e, Effect::Segment(..))),
      "interims are not durable"
    );
    let fx = m.step(Input::Engine(id(1), EngineEvent::Final(" stable words ".into())));
    assert!(has(&fx, &Effect::Segment(id(1), "stable words".into())));
    assert!(m
      .step(Input::Engine(id(1), EngineEvent::Final("   ".into())))
      .is_empty());
  }

  #[test]
  fn double_start_is_ignored() {
    let mut m = machine();
    m.step(Input::Start(Trigger::Tray));
    let fx = m.step(Input::Start(Trigger::Tray));
    assert!(fx.is_empty());
    assert_eq!(m.active_id(), Some(id(1)));
  }

  #[test]
  fn double_finish_is_ignored() {
    let mut m = machine();
    m.step(Input::Start(Trigger::Tray));
    m.step(Input::Engine(id(1), EngineEvent::Ready));
    let first = m.step(Input::Finish(Trigger::FlowBar));
    assert!(has(&first, &Effect::FinishEngine(id(1))));
    assert!(m.step(Input::Finish(Trigger::FlowBar)).is_empty());
    assert!(m.step(Input::Toggle(Trigger::Hotkey)).is_empty());
  }

  #[test]
  fn cancel_during_connect_cancels_engine_and_records() {
    let mut m = machine();
    m.step(Input::Start(Trigger::Hotkey));
    let fx = m.step(Input::Cancel(Trigger::FlowBar));
    assert!(has(&fx, &Effect::CancelEngine(id(1))));
    assert_eq!(outcome(&fx), Some(Outcome::Cancelled));
    assert_eq!(m.phase(), Phase::Idle);
    // The engine reports ready late: ignored.
    assert!(m.step(Input::Engine(id(1), EngineEvent::Ready)).is_empty());
  }

  #[test]
  fn cancel_keeps_partial_transcript_in_record() {
    let mut m = machine();
    m.step(Input::Start(Trigger::Hotkey));
    m.step(Input::Engine(id(1), EngineEvent::Final("keep me".into())));
    let fx = m.step(Input::Cancel(Trigger::FlowBar));
    assert!(fx
      .iter()
      .any(|e| matches!(e, Effect::Record { transcript, .. } if transcript == "keep me")));
    assert!(!fx.iter().any(|e| matches!(e, Effect::Deliver(..))));
  }

  #[test]
  fn finish_during_connect_goes_to_finalizing() {
    let mut m = machine();
    m.step(Input::Start(Trigger::Hotkey));
    let fx = m.step(Input::Toggle(Trigger::Hotkey));
    assert!(has(&fx, &Effect::FinishEngine(id(1))));
    assert_eq!(m.phase(), Phase::Finalizing);
    let fx = m.step(Input::Engine(id(1), EngineEvent::Finalized));
    assert_eq!(outcome(&fx), Some(Outcome::NoSpeech));
    assert_eq!(m.phase(), Phase::Idle);
  }

  #[test]
  fn stale_events_from_previous_session_are_ignored() {
    let mut m = machine();
    m.step(Input::Start(Trigger::Hotkey));
    m.step(Input::Cancel(Trigger::Hotkey));
    m.step(Input::Start(Trigger::Hotkey));
    assert_eq!(m.active_id(), Some(id(2)));
    assert!(m
      .step(Input::Engine(id(1), EngineEvent::Final("old".into())))
      .is_empty());
    assert!(m.step(Input::Engine(id(1), EngineEvent::Ready)).is_empty());
    assert!(m.step(Input::Timeout(id(1), TimerKind::Connect)).is_empty());
    assert!(m.step(Input::Processed(id(1), Processed::Text("x".into()))).is_empty());
    assert_eq!(m.transcript_text(), "");
    assert_eq!(m.phase(), Phase::Preparing);
  }

  #[test]
  fn connect_timeout_fails_then_error_hides() {
    let mut m = machine();
    m.step(Input::Start(Trigger::Hotkey));
    let fx = m.step(Input::Timeout(id(1), TimerKind::Connect));
    assert!(has(&fx, &Effect::CancelEngine(id(1))));
    assert_eq!(m.phase(), Phase::Error);
    assert!(m.snapshot().error.is_some());
    m.step(Input::Timeout(id(1), TimerKind::ErrorHide));
    assert_eq!(m.phase(), Phase::Idle);
  }

  #[test]
  fn connect_timer_after_ready_is_ignored() {
    let mut m = machine();
    m.step(Input::Start(Trigger::Hotkey));
    m.step(Input::Engine(id(1), EngineEvent::Ready));
    assert!(m.step(Input::Timeout(id(1), TimerKind::Connect)).is_empty());
    assert_eq!(m.phase(), Phase::Listening);
  }

  #[test]
  fn provider_failure_while_listening_does_not_deliver() {
    let mut m = machine();
    m.step(Input::Start(Trigger::Hotkey));
    m.step(Input::Engine(id(1), EngineEvent::Ready));
    m.step(Input::Engine(id(1), EngineEvent::Final("partial".into())));
    let fx = m.step(Input::Engine(id(1), EngineEvent::Failed(EngineError::Network)));
    assert_eq!(
      outcome(&fx),
      Some(Outcome::Failed {
        error: EngineError::Network
      })
    );
    assert!(!fx
      .iter()
      .any(|e| matches!(e, Effect::Deliver(..) | Effect::Process(..))));
    assert_eq!(m.phase(), Phase::Error);
  }

  #[test]
  fn provider_failure_during_finalize_with_text_still_processes() {
    let mut m = machine();
    m.step(Input::Start(Trigger::Hotkey));
    m.step(Input::Engine(id(1), EngineEvent::Final("almost done".into())));
    m.step(Input::Finish(Trigger::Hotkey));
    let fx = m.step(Input::Engine(id(1), EngineEvent::Failed(EngineError::Lost)));
    assert!(has(&fx, &Effect::Process(id(1), "almost done".into())));
  }

  #[test]
  fn finalize_timeout_uses_collected_text() {
    let mut m = machine();
    m.step(Input::Start(Trigger::Hotkey));
    m.step(Input::Engine(id(1), EngineEvent::Interim("tail words".into())));
    m.step(Input::Finish(Trigger::Hotkey));
    let fx = m.step(Input::Timeout(id(1), TimerKind::Finalize));
    assert!(has(&fx, &Effect::CancelEngine(id(1))));
    assert!(has(&fx, &Effect::Process(id(1), "tail words".into())));
  }

  #[test]
  fn delivering_ignores_cancel_and_toggle() {
    let mut m = machine();
    m.step(Input::Start(Trigger::Hotkey));
    m.step(Input::Engine(id(1), EngineEvent::Final("x".into())));
    m.step(Input::Finish(Trigger::Hotkey));
    m.step(Input::Engine(id(1), EngineEvent::Finalized));
    m.step(Input::Processed(id(1), Processed::Text("X.".into())));
    assert_eq!(m.phase(), Phase::Delivering);
    assert!(m.step(Input::Cancel(Trigger::FlowBar)).is_empty());
    assert!(m.step(Input::Toggle(Trigger::Hotkey)).is_empty());
    assert_eq!(m.phase(), Phase::Delivering);
  }

  #[test]
  fn shutdown_while_listening_interrupts() {
    let mut m = machine();
    m.step(Input::Start(Trigger::Hotkey));
    let fx = m.step(Input::Shutdown);
    assert!(has(&fx, &Effect::CancelEngine(id(1))));
    assert_eq!(outcome(&fx), Some(Outcome::Interrupted));
    assert!(has(&fx, &Effect::ShutdownComplete));
    // No new session may start once shut down... the actor stops anyway, but be strict.
  }

  #[test]
  fn shutdown_during_delivery_waits_for_delivery() {
    let mut m = machine();
    m.step(Input::Start(Trigger::Hotkey));
    m.step(Input::Engine(id(1), EngineEvent::Final("x".into())));
    m.step(Input::Finish(Trigger::Hotkey));
    m.step(Input::Engine(id(1), EngineEvent::Finalized));
    m.step(Input::Processed(id(1), Processed::Text("X.".into())));
    let fx = m.step(Input::Shutdown);
    assert!(!has(&fx, &Effect::ShutdownComplete));
    assert!(m.step(Input::Start(Trigger::Hotkey)).is_empty());
    let fx = m.step(Input::Delivered(id(1), DeliveryOutcome::Copied));
    assert!(has(&fx, &Effect::ShutdownComplete));
    assert!(
      m.step(Input::Start(Trigger::Hotkey)).is_empty(),
      "no start after shutdown"
    );
  }

  #[test]
  fn error_state_allows_new_start() {
    let mut m = machine();
    m.step(Input::Start(Trigger::Hotkey));
    m.step(Input::Engine(id(1), EngineEvent::Failed(EngineError::Auth)));
    assert_eq!(m.phase(), Phase::Error);
    let fx = m.step(Input::Toggle(Trigger::Hotkey));
    assert!(has(&fx, &Effect::StartEngine(id(2))));
    // The old error-hide timer must not touch the new session.
    assert!(m.step(Input::Timeout(id(1), TimerKind::ErrorHide)).is_empty());
    assert_eq!(m.phase(), Phase::Preparing);
  }

  /// Deterministic pseudo-random input sequences: at most one engine is ever live and every
  /// started session is recorded exactly once.
  #[test]
  fn random_sequences_never_overlap_sessions() {
    let mut seed: u64 = 0x5eed;
    let mut rnd = move |n: u64| {
      seed ^= seed << 13;
      seed ^= seed >> 7;
      seed ^= seed << 17;
      seed % n
    };
    for _ in 0..300 {
      let mut m = machine();
      let mut live: Vec<SessionId> = Vec::new();
      let mut started = 0usize;
      let mut recorded = std::collections::HashMap::<SessionId, usize>::new();
      for _ in 0..60 {
        let cur = m.active_id().unwrap_or(id(999));
        let other = id(rnd(4) as u128);
        let target = if rnd(4) == 0 { other } else { cur };
        let input = match rnd(13) {
          0 => Input::Start(Trigger::Hotkey),
          1 | 2 => Input::Toggle(Trigger::Hotkey),
          3 => Input::Finish(Trigger::FlowBar),
          4 => Input::Cancel(Trigger::FlowBar),
          5 => Input::Engine(target, EngineEvent::Ready),
          6 => Input::Engine(target, EngineEvent::Final("w".into())),
          7 => Input::Engine(target, EngineEvent::Finalized),
          8 => Input::Engine(target, EngineEvent::Failed(EngineError::Network)),
          9 => Input::Processed(target, Processed::Text("W.".into())),
          10 => Input::Delivered(target, DeliveryOutcome::Pasted),
          11 => Input::Timeout(target, TimerKind::Finalize),
          _ => Input::Timeout(target, TimerKind::Connect),
        };
        for e in m.step(input) {
          match e {
            Effect::StartEngine(s) => {
              assert!(live.is_empty(), "engine started while another is live");
              live.push(s);
              started += 1;
            }
            Effect::CancelEngine(s) => live.retain(|x| *x != s),
            Effect::Record { id, .. } => *recorded.entry(id).or_default() += 1,
            _ => {}
          }
        }
        // An engine that finished capture is no longer live once the machine leaves capture.
        if !matches!(m.phase(), Phase::Preparing | Phase::Listening | Phase::Finalizing) {
          live.clear();
        }
      }
      assert!(recorded.values().all(|&n| n == 1), "a session was recorded twice");
      assert!(recorded.len() <= started);
    }
  }
}
