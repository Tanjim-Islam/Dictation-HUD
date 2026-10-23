//! Frames between Dictation HUD and its local speech helper (`dictation-asr.exe`).
//!
//! A frame is a little-endian u32 header length, a u32 body length, the JSON header, then the
//! body. Audio bodies are 16 kHz mono i16 little-endian PCM. The helper answers every request
//! with zero or more `Segment` replies followed by exactly one terminal reply.

use std::collections::BTreeMap;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub const MAX_HEADER: usize = 64 * 1024;
pub const MAX_BODY: usize = 16 * 1024 * 1024;

/// Model architecture, which decides the file roles the helper needs (see `roles`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
  /// Parakeet TDT, FastConformer, GigaAM, Zipformer transducers.
  Transducer,
  Whisper,
  /// NeMo CTC models (Parakeet TDT-CTC Japanese).
  NemoCtc,
  Canary,
  /// Moonshine v2: encoder plus merged decoder.
  Moonshine,
  CohereTranscribe,
  FireRedAsr,
  FireRedAsrCtc,
  Dolphin,
  Omnilingual,
  Qwen3Asr,
}

impl ModelKind {
  /// File roles a model of this kind must provide.
  pub fn roles(self) -> &'static [&'static str] {
    match self {
      ModelKind::Transducer => &["encoder", "decoder", "joiner", "tokens"],
      ModelKind::Whisper | ModelKind::Canary | ModelKind::CohereTranscribe | ModelKind::FireRedAsr => {
        &["encoder", "decoder", "tokens"]
      }
      ModelKind::Moonshine => &["encoder", "merged_decoder", "tokens"],
      ModelKind::NemoCtc | ModelKind::FireRedAsrCtc | ModelKind::Dolphin | ModelKind::Omnilingual => {
        &["model", "tokens"]
      }
      ModelKind::Qwen3Asr => &["conv_frontend", "encoder", "decoder", "tokenizer"],
    }
  }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
  /// Loads a model, replacing any loaded one. `files` maps each role of `kind` to a path.
  /// `language` is a model code, or empty for auto.
  Load {
    kind: ModelKind,
    files: BTreeMap<String, String>,
    vad: String,
    language: String,
    threads: i32,
  },
  /// Body: PCM. Replies: `Segment`* then `Ack`.
  Audio,
  /// Ends the utterance. Replies: `Segment`* then `Flushed`.
  Flush,
  /// Drops buffered audio before a new session. Reply: `Ack`.
  Reset,
  Ping,
  Shutdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reply {
  Loaded { ms: u64 },
  Segment { text: String },
  Ack,
  Flushed,
  Pong,
  Error { kind: ErrorKind, detail: String },
}

impl Reply {
  pub fn is_terminal(&self) -> bool {
    !matches!(self, Reply::Segment { .. })
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
  /// Missing or unreadable model files, or the runtime refused them.
  Load,
  /// Audio arrived before a model was loaded.
  NotLoaded,
  BadRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
  TooLarge,
  BadHeader,
}

pub fn encode<T: Serialize>(header: &T, body: &[u8]) -> Vec<u8> {
  let h = serde_json::to_vec(header).expect("frame headers always serialize");
  let mut out = Vec::with_capacity(8 + h.len() + body.len());
  out.extend_from_slice(&(h.len() as u32).to_le_bytes());
  out.extend_from_slice(&(body.len() as u32).to_le_bytes());
  out.extend_from_slice(&h);
  out.extend_from_slice(body);
  out
}

/// A decoded frame: JSON header bytes and body bytes.
pub type Frame = (Vec<u8>, Vec<u8>);

/// Incremental decoder: push bytes as they arrive, pop whole frames.
#[derive(Default)]
pub struct Decoder {
  buf: Vec<u8>,
}

impl Decoder {
  pub fn push(&mut self, bytes: &[u8]) {
    self.buf.extend_from_slice(bytes);
  }

  pub fn next_frame(&mut self) -> Result<Option<Frame>, FrameError> {
    if self.buf.len() < 8 {
      return Ok(None);
    }
    let h = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
    let b = u32::from_le_bytes([self.buf[4], self.buf[5], self.buf[6], self.buf[7]]) as usize;
    if h > MAX_HEADER || b > MAX_BODY {
      return Err(FrameError::TooLarge);
    }
    if self.buf.len() < 8 + h + b {
      return Ok(None);
    }
    let header = self.buf[8..8 + h].to_vec();
    let body = self.buf[8 + h..8 + h + b].to_vec();
    self.buf.drain(..8 + h + b);
    Ok(Some((header, body)))
  }
}

pub fn decode<T: DeserializeOwned>(header: &[u8]) -> Result<T, FrameError> {
  serde_json::from_slice(header).map_err(|_| FrameError::BadHeader)
}

pub fn pcm_to_bytes(pcm: &[i16]) -> Vec<u8> {
  pcm.iter().flat_map(|s| s.to_le_bytes()).collect()
}

pub fn bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
  bytes
    .as_chunks::<2>()
    .0
    .iter()
    .map(|c| i16::from_le_bytes(*c) as f32 / 32768.0)
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn frames_round_trip_across_partial_pushes() {
    let a = encode(&Request::Ping, &[]);
    let b = encode(&Request::Audio, &pcm_to_bytes(&[1, -2, 32767]));
    let mut bytes = a.clone();
    bytes.extend_from_slice(&b);
    let mut d = Decoder::default();
    d.push(&bytes[..5]);
    assert_eq!(d.next_frame(), Ok(None));
    d.push(&bytes[5..]);
    let (h, body) = d.next_frame().unwrap().unwrap();
    assert_eq!(decode::<Request>(&h), Ok(Request::Ping));
    assert!(body.is_empty());
    let (h, body) = d.next_frame().unwrap().unwrap();
    assert_eq!(decode::<Request>(&h), Ok(Request::Audio));
    assert_eq!(body, vec![1, 0, 254, 255, 255, 127]);
    assert_eq!(d.next_frame(), Ok(None));
  }

  #[test]
  fn replies_serialize_with_a_type_tag() {
    let r = Reply::Error {
      kind: ErrorKind::Load,
      detail: "missing tokens.txt".into(),
    };
    let json = String::from_utf8(serde_json::to_vec(&r).unwrap()).unwrap();
    assert_eq!(json, r#"{"type":"error","kind":"load","detail":"missing tokens.txt"}"#);
    assert!(!Reply::Segment { text: "x".into() }.is_terminal());
    assert!(Reply::Flushed.is_terminal());
  }

  #[test]
  fn oversized_frames_are_rejected() {
    let mut d = Decoder::default();
    let mut bad = (MAX_HEADER as u32 + 1).to_le_bytes().to_vec();
    bad.extend_from_slice(&0u32.to_le_bytes());
    d.push(&bad);
    assert_eq!(d.next_frame(), Err(FrameError::TooLarge));
  }

  #[test]
  fn every_kind_names_its_roles() {
    use ModelKind::*;
    for k in [
      Transducer,
      Whisper,
      NemoCtc,
      Canary,
      Moonshine,
      CohereTranscribe,
      FireRedAsr,
      FireRedAsrCtc,
      Dolphin,
      Omnilingual,
      Qwen3Asr,
    ] {
      assert!(!k.roles().is_empty(), "{k:?}");
    }
    let load = Request::Load {
      kind: Moonshine,
      files: [("encoder".to_string(), "e.ort".to_string())].into(),
      vad: "v".into(),
      language: String::new(),
      threads: 2,
    };
    assert_eq!(decode::<Request>(&encode(&load, &[])[8..]), Ok(load));
  }

  #[test]
  fn pcm_converts_to_unit_floats() {
    let f = bytes_to_f32(&pcm_to_bytes(&[0, 16384, -32768]));
    assert_eq!(f, vec![0.0, 0.5, -1.0]);
  }
}
