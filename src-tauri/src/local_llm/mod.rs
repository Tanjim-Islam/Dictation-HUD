//! Local refinement servers (Ollama, LM Studio): detection, endpoint, model lists, start, warm-up.
//! Only loopback endpoints are ever called, so transcripts stay on this computer.

pub mod detect;
pub mod scan;
pub mod server;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LocalApp {
  Ollama,
  LmStudio,
}

impl LocalApp {
  pub const ALL: [LocalApp; 2] = [LocalApp::Ollama, LocalApp::LmStudio];

  pub fn id(self) -> &'static str {
    match self {
      LocalApp::Ollama => "ollama",
      LocalApp::LmStudio => "lmstudio",
    }
  }

  pub fn name(self) -> &'static str {
    match self {
      LocalApp::Ollama => "Ollama",
      LocalApp::LmStudio => "LM Studio",
    }
  }

  pub fn from_id(id: &str) -> Option<Self> {
    Self::ALL.into_iter().find(|a| a.id() == id)
  }

  pub fn default_port(self) -> u16 {
    match self {
      LocalApp::Ollama => 11434,
      LocalApp::LmStudio => 1234,
    }
  }
}

/// A text model on this computer. `usable` is false for entries found only on disk while the
/// server is not answering.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LocalModel {
  pub id: String,
  pub name: String,
  pub size_bytes: u64,
  pub usable: bool,
  /// Reasoning model: thinking is turned off in refinement requests where the API allows.
  pub thinking: bool,
}

/// A small, fast text model offered when none is installed. Both picks are non-thinking models.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Pick {
  pub id: &'static str,
  pub name: &'static str,
  pub size_bytes: u64,
}

const OLLAMA_PICKS: &[Pick] = &[
  Pick {
    id: "gemma3:4b",
    name: "Gemma 3 4B",
    size_bytes: 3_350_272_259,
  },
  Pick {
    id: "qwen3:4b-instruct-2507-q4_K_M",
    name: "Qwen3 4B Instruct",
    size_bytes: 2_497_293_316,
  },
];

/// LM Studio sizes are the matching Q4_K_M downloads (approximate; LM Studio picks the variant).
const LMSTUDIO_PICKS: &[Pick] = &[
  Pick {
    id: "google/gemma-3-4b",
    name: "Gemma 3 4B",
    size_bytes: 3_350_000_000,
  },
  Pick {
    id: "qwen/qwen3-4b-2507",
    name: "Qwen3 4B Instruct",
    size_bytes: 2_500_000_000,
  },
];

pub fn picks(app: LocalApp) -> &'static [Pick] {
  match app {
    LocalApp::Ollama => OLLAMA_PICKS,
    LocalApp::LmStudio => LMSTUDIO_PICKS,
  }
}

/// Text models only: drops embedding and reranker models by name.
pub fn is_text_model_name(name: &str) -> bool {
  let n = name.to_ascii_lowercase();
  !["embed", "rerank", "bge-", "minilm"].iter().any(|k| n.contains(k))
}
