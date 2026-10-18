//! Authoritative dictation session manager.
//!
//! One long-lived actor task owns the [`machine::Machine`]. Every intent (hotkey, tray, Flow
//! Bar, app) and every engine/processing/delivery/timer event goes through its mailbox, so
//! session state has exactly one writer. The WebView only renders snapshots and sends intents.

pub mod actor;
pub mod host;
pub mod machine;

pub use actor::SessionHandle;
pub use machine::{EngineError, EngineEvent, Input, Phase, Snapshot, Trigger};
