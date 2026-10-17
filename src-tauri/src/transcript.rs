//! Aggregates streaming transcript events into one text.
//!
//! Providers send interim hypotheses that replace each other and final segments that never
//! change again. The full transcript is every final segment in order plus the latest interim
//! tail that has not been finalized yet.

#[derive(Debug, Default, Clone)]
pub struct Aggregator {
  finals: Vec<String>,
  pending: Option<String>,
}

impl Aggregator {
  pub fn interim(&mut self, text: &str) {
    let t = text.trim();
    self.pending = if t.is_empty() { None } else { Some(t.to_string()) };
  }

  pub fn finalize(&mut self, text: &str) {
    let t = text.trim();
    if !t.is_empty() {
      self.finals.push(t.to_string());
    }
    self.pending = None;
  }

  pub fn text(&self) -> String {
    let mut parts: Vec<&str> = self.finals.iter().map(String::as_str).collect();
    if let Some(p) = &self.pending {
      parts.push(p);
    }
    parts.join(" ")
  }

  pub fn is_empty(&self) -> bool {
    self.finals.is_empty() && self.pending.is_none()
  }

  pub fn final_segments(&self) -> &[String] {
    &self.finals
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn interims_replace_and_finals_append() {
    let mut a = Aggregator::default();
    a.interim("hel");
    a.interim("hello wor");
    a.finalize("hello world");
    a.interim("how");
    assert_eq!(a.text(), "hello world how");
    a.finalize("how are you");
    assert_eq!(a.text(), "hello world how are you");
    assert_eq!(a.final_segments().len(), 2);
  }

  #[test]
  fn empty_inputs_are_ignored() {
    let mut a = Aggregator::default();
    a.interim("   ");
    a.finalize("");
    assert!(a.is_empty());
    assert_eq!(a.text(), "");
  }

  #[test]
  fn final_clears_pending_tail() {
    let mut a = Aggregator::default();
    a.interim("draft words");
    a.finalize("draft words.");
    assert_eq!(a.text(), "draft words.");
  }
}
