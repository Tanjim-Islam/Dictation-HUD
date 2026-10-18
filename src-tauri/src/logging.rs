//! Structured logging.
//!
//! Rules for every log call in this crate:
//! - never log credentials or any part of a credential (no prefixes either);
//! - never log transcript text, context text, clipboard content, or raw provider payloads;
//! - log ids, states, sizes, durations, and error kinds instead.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Keeps the background log writer alive for the lifetime of the app.
pub struct LogGuard(#[allow(dead_code)] WorkerGuard);

pub fn init(log_dir: &Path) -> Option<LogGuard> {
  let _ = std::fs::create_dir_all(log_dir);
  let file_appender = tracing_appender::rolling::Builder::new()
    .rotation(tracing_appender::rolling::Rotation::DAILY)
    .filename_prefix("dictation-hud")
    .filename_suffix("log")
    .max_log_files(7)
    .build(log_dir)
    .ok()?;
  let (file_writer, guard) = tracing_appender::non_blocking(file_appender);

  let filter = EnvFilter::try_from_env("DICTATION_LOG").unwrap_or_else(|_| EnvFilter::new("info,dictation_hud=debug"));

  let result = tracing_subscriber::registry()
    .with(filter)
    .with(fmt::layer().with_writer(std::io::stderr).with_target(true))
    .with(fmt::layer().with_writer(file_writer).with_ansi(false).with_target(true))
    .try_init();

  if result.is_err() {
    // A subscriber is already installed (tests). Drop silently.
    return None;
  }
  Some(LogGuard(guard))
}

/// Short, content-free description of a text value for logs.
pub fn text_shape(text: &str) -> String {
  format!(
    "{} chars, {} words",
    text.chars().count(),
    text.split_whitespace().count()
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn text_shape_has_no_content() {
    let shape = text_shape("secret words here");
    assert_eq!(shape, "17 chars, 3 words");
    assert!(!shape.contains("secret"));
  }
}
