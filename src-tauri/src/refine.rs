//! AI refinement of transcripts. Any failure falls back to the deterministic text.

use std::time::Duration;

use tauri::AppHandle;

use crate::{config, logging::text_shape, prompt};

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

/// Basic deterministic cleanup used when AI refinement is off or fails.
pub fn basic_punctuation_cleanup(text: &str) -> String {
  let mut result = text.trim().to_string();
  if let Some(first) = result.chars().next() {
    if first.is_lowercase() {
      let upper: String = first.to_uppercase().collect();
      result = upper + &result[first.len_utf8()..];
    }
  }
  if let Some(last) = result.chars().last() {
    if !matches!(last, '.' | '!' | '?' | ',' | ';' | ':' | '।') {
      result.push('.');
    }
  }
  result
}

struct Endpoint {
  url: &'static str,
  name: &'static str,
}

const OPENROUTER: Endpoint = Endpoint {
  url: "https://openrouter.ai/api/v1/chat/completions",
  name: "openrouter",
};
const MEGALLM: Endpoint = Endpoint {
  url: "https://ai.megallm.io/v1/chat/completions",
  name: "megallm",
};

async fn call_chat(endpoint: &Endpoint, key: &str, model: &str, text: &str) -> Result<String, RefineError> {
  let body = serde_json::json!({
    "model": model,
    "messages": [
      {"role": "system", "content": prompt::get_system_prompt()},
      {"role": "user", "content": text}
    ]
  });
  let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(8))
    .build()
    .map_err(|_| RefineError::Network)?;
  let resp = client
    .post(endpoint.url)
    .bearer_auth(key)
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

/// Refines `text` with the chosen provider. Returns the provider error on failure so the
/// caller can decide on the fallback.
pub async fn refine_with_provider(app: &AppHandle, provider: &str, text: &str) -> Result<String, RefineError> {
  let (endpoint, key, model) = if provider == "megallm" {
    let key = config::get_megallm_key(app).await.ok_or(RefineError::MissingKey)?;
    let model = config::get_megallm_model(app).await.unwrap_or_else(|| "gpt-4".into());
    (&MEGALLM, key, model)
  } else {
    let key = config::get_openrouter_key(app).await.ok_or(RefineError::MissingKey)?;
    let model = config::get_model(app)
      .await
      .unwrap_or_else(|| "openai/gpt-oss-20b:free".into());
    (&OPENROUTER, key, model)
  };
  let started = std::time::Instant::now();
  let result = call_chat(endpoint, &key, &model, text)
    .await
    .and_then(|r| validate_refined(&r, text));
  match &result {
    Ok(out) => {
      tracing::info!(provider = endpoint.name, ms = started.elapsed().as_millis() as u64, output = %text_shape(out), "refinement ok")
    }
    Err(e) => {
      tracing::warn!(provider = endpoint.name, ms = started.elapsed().as_millis() as u64, error = %e, "refinement failed")
    }
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

  #[test]
  fn cleanup_capitalizes_unicode_and_keeps_bengali_danda() {
    assert_eq!(basic_punctuation_cleanup("hello world"), "Hello world.");
    assert_eq!(basic_punctuation_cleanup("émile"), "Émile.");
    assert_eq!(basic_punctuation_cleanup("আমি ভালো আছি।"), "আমি ভালো আছি।");
  }
}
