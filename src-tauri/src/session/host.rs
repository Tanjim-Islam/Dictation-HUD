//! Side effects of the session machine, executed by the actor.

use super::actor::SessionHandle;
use super::machine::{Outcome, SessionId, Snapshot};

pub trait Host: Send + Sync + 'static {
  fn publish(&self, snapshot: &Snapshot);
  fn start_engine(&self, id: SessionId, session: &SessionHandle);
  fn finish_engine(&self, id: SessionId);
  fn cancel_engine(&self, id: SessionId);
  /// Runs formatting/refinement off the actor and reports `Input::Processed`.
  fn process(&self, id: SessionId, transcript: String, session: &SessionHandle);
  /// Delivers text off the actor and reports `Input::Delivered`.
  fn deliver(&self, id: SessionId, text: String, session: &SessionHandle);
  fn record(&self, id: SessionId, outcome: &Outcome, transcript: &str);
}
