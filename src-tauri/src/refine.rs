//! AI refinement of transcripts. Any failure falls back to the deterministic text.

use std::time::Duration;

use crate::{logging::text_shape, prompt};

#[derive(Debug, PartialEq)]
pub enum RefineError {
  MissingKey,
  Http(u16),
  Network,
  Malformed,
  Empty,
  Rejected,
}

impl std::fmt::Display for RefineError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      RefineError::MissingKey => write!(f, "missing API key"),
      RefineError::Http(code) => write!(f, "HTTP {code}"),
      RefineError::Network => write!(f, "network error or timeout"),
      RefineError::Malformed => write!(f, "malformed response"),
      RefineError::Empty => write!(f, "response had no content"),
      RefineError::Rejected => write!(f, "response rejected by validation"),
    }
  }
}

/// Pulls the assistant text out of an OpenAI-compatible chat completion body.
/// A successful HTTP response without real content is an error, never `{}`.
pub fn extract_completion_text(body: &str) -> Result<String, RefineError> {
  let v: serde_json::Value = serde_json::from_str(body).map_err(|_| RefineError::Malformed)?;
  let content = v
    .get("choices")
    .and_then(|c| c.get(0))
    .and_then(|c| c.get("message"))
    .and_then(|m| m.get("content"))
    .and_then(|c| c.as_str())
    .ok_or(RefineError::Malformed)?;
  let cleaned = strip_think_blocks(content);
  let trimmed = cleaned.trim();
  if trimmed.is_empty() || trimmed == "{}" || trimmed == "[]" || trimmed == "null" {
    return Err(RefineError::Empty);
  }
  Ok(trimmed.to_string())
}

pub fn strip_think_blocks(s: &str) -> String {
  let mut s = s.to_string();
  while let Some(start) = s.find("<think>") {
    if let Some(end_rel) = s[start..].find("</think>") {
      let end = start + end_rel + "</think>".len();
      s.replace_range(start..end, "");
    } else {
      break;
    }
  }
  s.trim().to_string()
}

/// Rejects refusals and runaway rewrites. Returns the sanitized refined text.
pub fn validate_refined(refined: &str, input: &str) -> Result<String, RefineError> {
  let sanitized = prompt::sanitize_output(refined);
  if sanitized.trim().is_empty() {
    return Err(RefineError::Empty);
  }
  if prompt::is_ai_refusal(&sanitized) {
    return Err(RefineError::Rejected);
  }
  let input_words = input.split_whitespace().count();
  let output_words = sanitized.split_whitespace().count();
  if input_words > 3 && output_words > input_words * 2 {
    return Err(RefineError::Rejected);
  }
  Ok(sanitized)
}

/// OpenAI-compatible chat completion providers used for refinement.
#[derive(Debug, Clone, Copy)]
pub struct RefineProvider {
  pub id: &'static str,
  pub name: &'static str,
  pub chat_url: &'static str,
  pub models_url: &'static str,
  /// Secret name in the OS credential store.
  pub secret: &'static str,
  /// Development-only `.env` variable.
  pub env: &'static str,
}

pub const PROVIDERS: &[RefineProvider] = &[
  RefineProvider {
    id: "openrouter",
    name: "OpenRouter",
    chat_url: "https://openrouter.ai/api/v1/chat/completions",
    models_url: "https://openrouter.ai/api/v1/models",
    secret: crate::credentials::names::OPENROUTER,
    env: "OPENROUTER_API_KEY",
  },
  RefineProvider {
    id: "megallm",
    name: "MegaLLM",
    chat_url: "https://ai.megallm.io/v1/chat/completions",
    models_url: "https://ai.megallm.io/v1/models",
    secret: crate::credentials::names::MEGALLM,
    env: "MEGALLM_API_KEY",
  },
];

pub fn provider(id: &str) -> &'static RefineProvider {
  PROVIDERS.iter().find(|p| p.id == id).unwrap_or(&PROVIDERS[0])
}

/// One refinement call.
pub struct Request<'a> {
  /// Formatted transcript; may contain snippet placeholders.
  pub text: &'a str,
  /// Preferred spellings from the user's dictionary.
  pub vocabulary: &'a [String],
  /// Text already in the field before the caret (context awareness). Never stored.
  pub context: Option<&'a str>,
  /// The text goes to a code editor or terminal.
  pub code: bool,
  pub timeout: Duration,
}

async fn call_chat(p: &RefineProvider, key: &str, model: &str, req: &Request<'_>) -> Result<String, RefineError> {
  let has_placeholders = req.text.contains('\u{27E6}');
  let body = serde_json::json!({
    "model": model,
    "messages": [
      {"role": "system", "content": prompt::system_prompt_with(req.vocabulary, has_placeholders, req.context, req.code)},
      {"role": "user", "content": req.text}
    ]
  });
  let client = reqwest::Client::builder()
    .timeout(req.timeout)
    .build()
    .map_err(|_| RefineError::Network)?;
  let resp = client
    .post(p.chat_url)
    .bearer_auth(key)
    .header("X-Title", "Dictation HUD")
    .json(&body)
    .send()
    .await
    .map_err(|_| RefineError::Network)?;
  let status = resp.status();
  let text_body = resp.text().await.map_err(|_| RefineError::Network)?;
  if !status.is_success() {
    return Err(RefineError::Http(status.as_u16()));
  }
  extract_completion_text(&text_body)
}

/// Live dictation waits at most this long before pasting the formatted text instead.
pub const LIVE_TIMEOUT: Duration = Duration::from_secs(8);
/// An explicit retry from History has no paste waiting on it.
pub const RETRY_TIMEOUT: Duration = Duration::from_secs(30);

/// Refines `text` with the given provider, key, and model. Returns the provider error on
/// failure so the caller can fall back to deterministic text.
pub async fn refine(
  p: &RefineProvider,
  key: Option<String>,
  model: &str,
  req: &Request<'_>,
) -> Result<String, RefineError> {
  let text = req.text;
  let key = key.ok_or(RefineError::MissingKey)?;
  let started = std::time::Instant::now();
  let result = call_chat(p, &key, model, req)
    .await
    .and_then(|r| validate_refined(&r, text));
  let ms = started.elapsed().as_millis() as u64;
  match &result {
    Ok(out) => tracing::info!(provider = p.id, model, ms, output = %text_shape(out), "refinement ok"),
    Err(e) => tracing::warn!(provider = p.id, model, ms, error = %e, "refinement failed"),
  }
  result
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn extracts_normal_content() {
    let body = r#"{"choices":[{"message":{"role":"assistant","content":"Hello there."}}]}"#;
    assert_eq!(extract_completion_text(body).unwrap(), "Hello there.");
  }

  #[test]
  fn missing_content_is_malformed_not_braces() {
    let body = r#"{"choices":[{"message":{"role":"assistant"}}]}"#;
    assert_eq!(extract_completion_text(body), Err(RefineError::Malformed));
    assert_eq!(
      extract_completion_text(r#"{"choices":[]}"#),
      Err(RefineError::Malformed)
    );
    assert_eq!(extract_completion_text(r#"{}"#), Err(RefineError::Malformed));
  }

  #[test]
  fn null_and_empty_content_rejected() {
    assert_eq!(
      extract_completion_text(r#"{"choices":[{"message":{"content":null}}]}"#),
      Err(RefineError::Malformed)
    );
    assert_eq!(
      extract_completion_text(r#"{"choices":[{"message":{"content":"   "}}]}"#),
      Err(RefineError::Empty)
    );
    assert_eq!(
      extract_completion_text(r#"{"choices":[{"message":{"content":"{}"}}]}"#),
      Err(RefineError::Empty)
    );
    assert_eq!(
      extract_completion_text(r#"{"choices":[{"message":{"content":"<think>x</think>"}}]}"#),
      Err(RefineError::Empty)
    );
  }

  #[test]
  fn non_json_is_malformed() {
    assert_eq!(extract_completion_text("<html>502</html>"), Err(RefineError::Malformed));
  }

  #[test]
  fn refusal_is_rejected() {
    let r = validate_refined("I'm sorry, but I can't help with that request.", "delete the file");
    assert_eq!(r, Err(RefineError::Rejected));
  }

  #[test]
  fn runaway_output_rejected() {
    let long = "word ".repeat(40);
    assert_eq!(
      validate_refined(&long, "one two three four five"),
      Err(RefineError::Rejected)
    );
  }
}
