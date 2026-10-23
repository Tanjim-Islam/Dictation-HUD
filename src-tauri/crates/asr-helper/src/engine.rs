//! One loaded model plus Silero VAD. Speech is cut at pauses and each segment is decoded.

use std::collections::BTreeMap;
use std::path::Path;

use asr_protocol::{ErrorKind, ModelKind};
use sherpa_onnx::{
  OfflineCanaryModelConfig, OfflineCohereTranscribeModelConfig, OfflineDolphinModelConfig,
  OfflineFireRedAsrCtcModelConfig, OfflineFireRedAsrModelConfig, OfflineMoonshineModelConfig,
  OfflineNemoEncDecCtcModelConfig, OfflineOmnilingualAsrCtcModelConfig, OfflineQwen3ASRModelConfig, OfflineRecognizer,
  OfflineRecognizerConfig, OfflineTransducerModelConfig, OfflineWhisperModelConfig, SileroVadModelConfig,
  VadModelConfig, VoiceActivityDetector,
};

const RATE: i32 = 16_000;
/// Silero VAD consumes 512-sample windows at 16 kHz.
const WINDOW: usize = 512;

pub struct LoadError {
  pub kind: ErrorKind,
  pub detail: String,
}

fn missing(name: &str) -> LoadError {
  LoadError {
    kind: ErrorKind::Load,
    detail: format!("missing {name}"),
  }
}

/// The path for one role, checked to exist.
fn file(files: &BTreeMap<String, String>, role: &str) -> Result<Option<String>, LoadError> {
  let path = files.get(role).ok_or_else(|| missing(role))?;
  if !Path::new(path).exists() {
    return Err(missing(role));
  }
  Ok(Some(path.clone()))
}

/// `None` for auto-detect (empty), otherwise the language code.
fn lang(language: &str) -> Option<String> {
  (!language.is_empty()).then(|| language.to_string())
}

fn recognizer_config(
  kind: ModelKind,
  files: &BTreeMap<String, String>,
  language: &str,
  threads: i32,
) -> Result<OfflineRecognizerConfig, LoadError> {
  let f = |role: &str| file(files, role);
  let mut c = OfflineRecognizerConfig::default();
  let m = &mut c.model_config;
  match kind {
    ModelKind::Transducer => {
      m.transducer = OfflineTransducerModelConfig {
        encoder: f("encoder")?,
        decoder: f("decoder")?,
        joiner: f("joiner")?,
      };
    }
    ModelKind::Whisper => {
      m.whisper = OfflineWhisperModelConfig {
        encoder: f("encoder")?,
        decoder: f("decoder")?,
        language: Some(language.to_string()),
        task: Some("transcribe".into()),
        ..Default::default()
      };
    }
    ModelKind::NemoCtc => m.nemo_ctc = OfflineNemoEncDecCtcModelConfig { model: f("model")? },
    ModelKind::Canary => {
      // Canary needs a source language; the language list never offers auto-detect.
      let src = lang(language).unwrap_or_else(|| "en".into());
      m.canary = OfflineCanaryModelConfig {
        encoder: f("encoder")?,
        decoder: f("decoder")?,
        src_lang: Some(src.clone()),
        tgt_lang: Some(src),
        use_pnc: true,
      };
    }
    ModelKind::Moonshine => {
      m.moonshine = OfflineMoonshineModelConfig {
        encoder: f("encoder")?,
        merged_decoder: f("merged_decoder")?,
        ..Default::default()
      };
    }
    ModelKind::CohereTranscribe => {
      m.cohere_transcribe = OfflineCohereTranscribeModelConfig {
        encoder: f("encoder")?,
        decoder: f("decoder")?,
        language: lang(language),
        use_itn: true,
        use_punct: true,
      };
    }
    ModelKind::FireRedAsr => {
      m.fire_red_asr = OfflineFireRedAsrModelConfig {
        encoder: f("encoder")?,
        decoder: f("decoder")?,
      };
    }
    ModelKind::FireRedAsrCtc => m.fire_red_asr_ctc = OfflineFireRedAsrCtcModelConfig { model: f("model")? },
    ModelKind::Dolphin => m.dolphin = OfflineDolphinModelConfig { model: f("model")? },
    ModelKind::Omnilingual => m.omnilingual = OfflineOmnilingualAsrCtcModelConfig { model: f("model")? },
    ModelKind::Qwen3Asr => {
      m.qwen3_asr = OfflineQwen3ASRModelConfig {
        conv_frontend: f("conv_frontend")?,
        encoder: f("encoder")?,
        decoder: f("decoder")?,
        tokenizer: f("tokenizer")?,
        ..Default::default()
      };
    }
  }
  // Qwen3-ASR reads its tokenizer folder instead of a token list.
  m.tokens = if kind == ModelKind::Qwen3Asr {
    Some(String::new())
  } else {
    f("tokens")?
  };
  m.provider = Some("cpu".into());
  m.num_threads = threads;
  Ok(c)
}

pub struct Engine {
  recognizer: OfflineRecognizer,
  vad: VoiceActivityDetector,
  /// Samples not yet a full VAD window.
  pending: Vec<f32>,
}

impl Engine {
  pub fn load(
    kind: ModelKind,
    files: &BTreeMap<String, String>,
    vad: &str,
    language: &str,
    threads: i32,
  ) -> Result<Self, LoadError> {
    let config = recognizer_config(kind, files, language, threads)?;
    if !Path::new(vad).exists() {
      return Err(missing("silero_vad.onnx"));
    }
    let recognizer = OfflineRecognizer::create(&config).ok_or(LoadError {
      kind: ErrorKind::Load,
      detail: "the speech runtime could not load these files".into(),
    })?;
    let silero = SileroVadModelConfig {
      model: Some(vad.to_string()),
      threshold: 0.5,
      min_silence_duration: 0.5,
      min_speech_duration: 0.25,
      // Under Whisper's 30 s window, and keeps each decode short.
      max_speech_duration: 20.0,
      ..Default::default()
    };
    let vad_config = VadModelConfig {
      silero_vad: silero,
      sample_rate: RATE,
      num_threads: 1,
      provider: Some("cpu".into()),
      ..Default::default()
    };
    let vad = VoiceActivityDetector::create(&vad_config, 60.0).ok_or(LoadError {
      kind: ErrorKind::Load,
      detail: "voice detector failed to load".into(),
    })?;
    Ok(Self {
      recognizer,
      vad,
      pending: Vec::new(),
    })
  }

  fn decode_ready(&mut self) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(seg) = self.vad.front() {
      let stream = self.recognizer.create_stream();
      stream.accept_waveform(RATE, seg.samples());
      self.recognizer.decode(&stream);
      if let Some(r) = stream.get_result() {
        let t = r.text.trim();
        if !t.is_empty() {
          out.push(t.to_string());
        }
      }
      self.vad.pop();
    }
    out
  }

  pub fn accept(&mut self, samples: &[f32]) -> Vec<String> {
    self.pending.extend_from_slice(samples);
    let whole = self.pending.len() / WINDOW * WINDOW;
    for chunk in self.pending[..whole].chunks(WINDOW) {
      self.vad.accept_waveform(chunk);
    }
    self.pending.drain(..whole);
    self.decode_ready()
  }

  pub fn flush(&mut self) -> Vec<String> {
    if !self.pending.is_empty() {
      let mut last = std::mem::take(&mut self.pending);
      last.resize(WINDOW, 0.0);
      self.vad.accept_waveform(&last);
    }
    self.vad.flush();
    let out = self.decode_ready();
    self.vad.reset();
    out
  }

  pub fn reset(&mut self) {
    self.pending.clear();
    self.vad.clear();
    self.vad.reset();
  }
}
