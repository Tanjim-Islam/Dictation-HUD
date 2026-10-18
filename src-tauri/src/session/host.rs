//! Side effects of the session machine, executed by the actor.

use super::actor::SessionHandle;
use super::machine::{Outcome, SessionId, Snapshot, Trigger};

pub trait Host: Send + Sync + 'static {
  fn publish(&self, snapshot: &Snapshot);
  /// Durable record of a new session. Runs before the engine starts.
  fn created(&self, _id: SessionId, _trigger: Trigger) {}
  /// Durable final transcript segment.
  fn segment(&self, _id: SessionId, _text: &str) {}
  fn start_engine(&self, id: SessionId, session: &SessionHandle);
  fn finish_engine(&self, id: SessionId);
  fn cancel_engine(&self, id: SessionId);
  /// Runs formatting/refinement off the actor and reports `Input::Processed`.
  fn process(&self, id: SessionId, transcript: String, session: &SessionHandle);
  /// Delivers text off the actor and reports `Input::Delivered`.
  fn deliver(&self, id: SessionId, text: String, session: &SessionHandle);
  fn record(&self, id: SessionId, outcome: &Outcome, transcript: &str);
}
