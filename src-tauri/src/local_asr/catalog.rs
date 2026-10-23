//! Preset local speech models (sherpa-onnx exports, int8 where available), pinned to exact
//! revisions and hashes. Each lists only the languages it handles well (checked 2026-10-23):
//! - Parakeet TDT v3: NVIDIA model card, 25 European languages.
//! - Whisper Large v3, Turbo, Medium: languages at or below about 10% WER/CER on FLEURS for
//!   large-v3; smaller Whisper sizes list fewer.
//! - Distil-Whisper Large v3 and v3.5 are English-only distillations.
//! - Qwen3-ASR 0.6B: Qwen model card, 30 languages. Cohere Transcribe: Cohere docs, 14 languages.
//! - Dolphin: DataoceanAI language list, 40 Eastern languages.
//! - Omnilingual ASR: Meta, 1,600+ languages; the list below covers widely used ones. English is
//!   left out: tested 2026-10-23, it mixed scripts and dropped punctuation.
//! - Moonshine v2, FastConformer, GigaAM, Zipformer, Canary: their model cards.
//! - FireRed ASR 2: Chinese only here; its English came out all caps with errors in testing.
//!
//! `multi` means automatic language detection. Generated from pinned Hugging Face revisions and
//! GitHub release digests; regenerate rather than hand-edit hashes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub use asr_protocol::ModelKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFile {
  /// What the helper uses the file for (see `ModelKind::roles`). `external_data` marks weights
  /// stored next to a model file; ONNX Runtime opens them itself.
  pub role: &'static str,
  pub name: &'static str,
  /// 0: only checked to exist (a folder, or a file inside a verified archive).
  pub size: u64,
  /// Empty for files inside a verified archive.
  pub sha256: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
  /// Individual files from a Hugging Face repo at a fixed commit.
  HfFiles { repo: &'static str, rev: &'static str },
  /// A `.tar.bz2` on GitHub (path after `https://github.com/`), unpacked from `root`.
  Archive {
    path: &'static str,
    size: u64,
    sha256: &'static str,
    root: &'static str,
  },
}

#[derive(Debug)]
pub struct CatalogModel {
  pub id: &'static str,
  pub name: &'static str,
  pub kind: ModelKind,
  pub languages: &'static [&'static str],
  pub source: Source,
  pub files: &'static [ModelFile],
  pub license: &'static str,
}

impl CatalogModel {
  pub fn download_bytes(&self) -> u64 {
    match self.source {
      Source::HfFiles { .. } => self.files.iter().map(|f| f.size).sum(),
      Source::Archive { size, .. } => size,
    }
  }

  /// Each file role mapped to its path inside `dir`.
  pub fn files_in(&self, dir: &Path) -> BTreeMap<String, PathBuf> {
    self
      .files
      .iter()
      .map(|f| (f.role.to_string(), dir.join(f.name)))
      .collect()
  }
}

pub const DEFAULT_MODEL: &str = "parakeet-v3";

const PARAKEET_V3_LANGS: &[&str] = &[
  "multi", "bg", "cs", "da", "de", "el", "en", "es", "et", "fi", "fr", "hr", "hu", "it", "lt", "lv", "mt", "nl", "pl",
  "pt", "ro", "ru", "sk", "sl", "sv", "uk",
];
const WHISPER_LARGE_LANGS: &[&str] = &[
  "multi", "en", "es", "it", "de", "pt", "fr", "nl", "pl", "ru", "uk", "fi", "ro", "sk", "id", "ja", "zh",
];
const WHISPER_SMALL_LANGS: &[&str] = &["multi", "en", "es", "it", "de", "pt", "fr"];
const QWEN3_ASR_LANGS: &[&str] = &[
  "multi", "zh", "en", "yue", "ar", "de", "fr", "es", "pt", "id", "it", "ko", "ru", "th", "vi", "ja", "tr", "hi", "ms",
  "nl", "sv", "da", "fi", "pl", "cs", "fil", "fa", "el", "hu", "mk", "ro",
];
const COHERE_LANGS: &[&str] = &[
  "en", "de", "fr", "it", "es", "pt", "el", "nl", "pl", "vi", "zh", "ar", "ja", "ko",
];
const DOLPHIN_LANGS: &[&str] = &[
  "multi", "zh", "ja", "th", "ru", "ko", "id", "vi", "yue", "hi", "ur", "ms", "uz", "ar", "fa", "bn", "ta", "te", "ug",
  "gu", "my", "fil", "kk", "or", "ne", "mn", "km", "jv", "lo", "si", "ps", "pa", "kab", "ba", "ks", "tg", "su", "mr",
  "ky", "az",
];
const OMNILINGUAL_LANGS: &[&str] = &[
  "multi", "es", "fr", "de", "it", "pt", "nl", "pl", "ru", "uk", "cs", "ro", "hu", "el", "sv", "da", "fi", "no", "tr",
  "ar", "fa", "he", "hi", "bn", "ur", "pa", "gu", "mr", "ta", "te", "kn", "ml", "ne", "si", "th", "vi", "id", "ms",
  "fil", "zh", "ja", "ko", "sw", "am", "ha", "yo", "ig", "zu",
];

const fn file(role: &'static str, name: &'static str, size: u64, sha256: &'static str) -> ModelFile {
  ModelFile {
    role,
    name,
    size,
    sha256,
  }
}

/// A file inside a verified archive: checked by name and size only.
const fn sized(role: &'static str, name: &'static str, size: u64) -> ModelFile {
  ModelFile {
    role,
    name,
    size,
    sha256: "",
  }
}

pub const CATALOG: &[CatalogModel] = &[
  CatalogModel {
    id: "parakeet-v3",
    name: "Parakeet v3",
    kind: ModelKind::Transducer,
    languages: PARAKEET_V3_LANGS,
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8",
      rev: "2bda32ec70b097a55adaa07d9a7173915b43cc78",
    },
    files: &[
      file("encoder", "encoder.int8.onnx", 652_184_281, "acfc2b4456377e15d04f0243af540b7fe7c992f8d898d751cf134c3a55fd2247"),
      file("decoder", "decoder.int8.onnx", 11_845_275, "179e50c43d1a9de79c8a24149a2f9bac6eb5981823f2a2ed88d655b24248db4e"),
      file("joiner", "joiner.int8.onnx", 6_355_277, "3164c13fc2821009440d20fcb5fdc78bff28b4db2f8d0f0b329101719c0948b3"),
      file("tokens", "tokens.txt", 93_939, "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d"),
    ],
    license: "CC-BY-4.0",
  },
  CatalogModel {
    id: "parakeet-v2",
    name: "Parakeet v2 (English)",
    kind: ModelKind::Transducer,
    languages: &["en"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8",
      rev: "1ab9323565ddb038682214b292f588070a538ce2",
    },
    files: &[
      file("encoder", "encoder.int8.onnx", 652_184_296, "a32b12d17bbbc309d0686fbbcc2987b5e9b8333a7da83fa6b089f0a2acd651ab"),
      file("decoder", "decoder.int8.onnx", 7_257_753, "b6bb64963457237b900e496ee9994b59294526439fbcc1fecf705b31a15c6b4e"),
      file("joiner", "joiner.int8.onnx", 1_739_080, "7946164367946e7f9f29a122407c3252b680dbae9a51343eb2488d057c3c43d2"),
      file("tokens", "tokens.txt", 9_384, "ec182b70dd42113aff6c5372c75cac58c952443eb22322f57bbd7f53977d497d"),
    ],
    license: "CC-BY-4.0",
  },
  CatalogModel {
    id: "parakeet-unified-en",
    name: "Parakeet Unified (English)",
    kind: ModelKind::Transducer,
    languages: &["en"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-non-streaming.tar.bz2",
      size: 501_350_460,
      sha256: "99f63605b3a85a54c250c0869670a687b7d6598a47bf2421515e1f839a76e150",
      root: "sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-non-streaming",
    },
    files: &[
      sized("encoder", "encoder.int8.onnx", 654_040_552),
      sized("decoder", "decoder.int8.onnx", 7_257_753),
      sized("joiner", "joiner.int8.onnx", 1_735_860),
      sized("tokens", "tokens.txt", 8_952),
    ],
    license: "CC-BY-4.0",
  },
  CatalogModel {
    id: "parakeet-110m-en",
    name: "Parakeet 110M (English)",
    kind: ModelKind::Transducer,
    languages: &["en"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet_tdt_transducer_110m-en-36000-int8.tar.bz2",
      size: 108_035_095,
      sha256: "f628312e9fdf8686374cb01a69425c41732529d540860311f16f37cbc32cfe9b",
      root: "sherpa-onnx-nemo-parakeet_tdt_transducer_110m-en-36000-int8",
    },
    files: &[
      sized("encoder", "encoder.int8.onnx", 131_113_202),
      sized("decoder", "decoder.int8.onnx", 3_955_863),
      sized("joiner", "joiner.int8.onnx", 1_411_403),
      sized("tokens", "tokens.txt", 9_953),
    ],
    license: "CC-BY-4.0",
  },
  CatalogModel {
    id: "parakeet-ja",
    name: "Parakeet (Japanese)",
    kind: ModelKind::NemoCtc,
    languages: &["ja"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-nemo-parakeet-tdt_ctc-0.6b-ja-35000-int8",
      rev: "bef18eb066808c90bd0f5df5be685767b0732de8",
    },
    files: &[
      file("model", "model.int8.onnx", 655_542_604, "3addd00ef5bd1742078389e540b77394e4a508bdf2f4c9ad1b4a76d93e76598e"),
      file("tokens", "tokens.txt", 28_557, "732f64c53909f2620c713f4106b487d92e6f54a6915b3cd3d1dbd32f9f4f392a"),
    ],
    license: "CC-BY-4.0",
  },
  CatalogModel {
    id: "canary-180m",
    name: "Canary 180M Flash",
    kind: ModelKind::Canary,
    languages: &["en", "de", "es", "fr"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-nemo-canary-180m-flash-en-es-de-fr-int8",
      rev: "9077164e0d3dd1d5353743e89ceaa1d3a770838c",
    },
    files: &[
      file("encoder", "encoder.int8.onnx", 132_678_643, "7a75b4e2a5857a6dcc0819503bbe3fad66943db4a3ccf21d3f27c633667d303f"),
      file("decoder", "decoder.int8.onnx", 74_437_848, "e41a2ab9c0c2fe81a1e8ade5a45fb02a74bc4db7d1f91b89a54a25e2cf79cba2"),
      file("tokens", "tokens.txt", 53_555, "2dae6fc7815f9640645e0c765522b278ee0cef49b482d91f6913e334628d3e77"),
    ],
    license: "CC-BY-4.0",
  },
  CatalogModel {
    id: "fastconformer-eu10",
    name: "FastConformer (10 European)",
    kind: ModelKind::Transducer,
    languages: &["multi", "be", "de", "en", "es", "fr", "hr", "it", "pl", "ru", "uk"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-fast-conformer-transducer-be-de-en-es-fr-hr-it-pl-ru-uk-20k-int8.tar.bz2",
      size: 106_546_673,
      sha256: "06072bad277f0f4c29cc866d7c62b0e47936da39afafeae453faa925025ccad6",
      root: "sherpa-onnx-nemo-fast-conformer-transducer-be-de-en-es-fr-hr-it-pl-ru-uk-20k-int8",
    },
    files: &[
      sized("encoder", "encoder.int8.onnx", 131_113_588),
      sized("decoder", "decoder.int8.onnx", 4_938_903),
      sized("joiner", "joiner.int8.onnx", 2_397_368),
      sized("tokens", "tokens.txt", 24_057),
    ],
    license: "CC-BY-4.0",
  },
  CatalogModel {
    id: "fastconformer-eu4",
    name: "FastConformer (EN, DE, ES, FR)",
    kind: ModelKind::Transducer,
    languages: &["multi", "en", "de", "es", "fr"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-fast-conformer-transducer-en-de-es-fr-14288-int8.tar.bz2",
      size: 106_902_947,
      sha256: "69416bd0845a8c1e78a110e7a6434fcf94af0c0c905d4d4d288e42d7b013af74",
      root: "sherpa-onnx-nemo-fast-conformer-transducer-en-de-es-fr-14288-int8",
    },
    files: &[
      sized("encoder", "encoder.int8.onnx", 131_113_682),
      sized("decoder", "decoder.int8.onnx", 4_938_904),
      sized("joiner", "joiner.int8.onnx", 2_397_367),
      sized("tokens", "tokens.txt", 24_057),
    ],
    license: "CC-BY-4.0",
  },
  CatalogModel {
    id: "fastconformer-en",
    name: "FastConformer (English)",
    kind: ModelKind::Transducer,
    languages: &["en"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-fast-conformer-transducer-en-24500-int8.tar.bz2",
      size: 107_976_970,
      sha256: "0a3323333a70360b6d85bc88b076ccaeeb31e90ce8cc1b72f322ba85d8d0632d",
      root: "sherpa-onnx-nemo-fast-conformer-transducer-en-24500-int8",
    },
    files: &[
      sized("encoder", "encoder.int8.onnx", 131_113_521),
      sized("decoder", "decoder.int8.onnx", 3_955_864),
      sized("joiner", "joiner.int8.onnx", 1_408_182),
      sized("tokens", "tokens.txt", 11_433),
    ],
    license: "CC-BY-4.0",
  },
  CatalogModel {
    id: "fastconformer-es",
    name: "FastConformer (Spanish)",
    kind: ModelKind::Transducer,
    languages: &["es"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-fast-conformer-transducer-es-1424-int8.tar.bz2",
      size: 107_246_685,
      sha256: "d6d29ffd560c366da1b177b71e79d8733a9a9910dd1ace282d9bfae94e056210",
      root: "sherpa-onnx-nemo-fast-conformer-transducer-es-1424-int8",
    },
    files: &[
      sized("encoder", "encoder.int8.onnx", 131_113_475),
      sized("decoder", "decoder.int8.onnx", 3_955_863),
      sized("joiner", "joiner.int8.onnx", 1_408_182),
      sized("tokens", "tokens.txt", 10_871),
    ],
    license: "CC-BY-4.0",
  },
  CatalogModel {
    id: "fastconformer-de",
    name: "FastConformer (German)",
    kind: ModelKind::Transducer,
    languages: &["de"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-nemo-transducer-stt_de_fastconformer_hybrid_large_pc-int8",
      rev: "df32693bc4328136c455af24035a5ee1ec708df0",
    },
    files: &[
      file("encoder", "encoder.int8.onnx", 131_114_014, "10fbb959c36c461afb02ea87c109119eb001f30e91a130663bd6d5bbcba74ba9"),
      file("decoder", "decoder.int8.onnx", 3_955_863, "4633a4b0a3f21f7ace4df02e772f5b5e63d45f84a71ee1e2660998fc4beb46ad"),
      file("joiner", "joiner.int8.onnx", 1_408_183, "80f493f2885f6ccbd9cf9c20f4b6c616c2eb6dae48df6a5d4a0653354f7fe66b"),
      file("tokens", "tokens.txt", 10_686, "abb1136142604d6d1766ad5060bd4f4b1048d7a096cd094b2d40eec3e666be9f"),
    ],
    license: "CC-BY-4.0",
  },
  CatalogModel {
    id: "fastconformer-pt",
    name: "FastConformer (Portuguese)",
    kind: ModelKind::Transducer,
    languages: &["pt"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-nemo-transducer-stt_pt_fastconformer_hybrid_large_pc-int8",
      rev: "8f00b3b3689a0899c9a0fb84aaf28a534e104432",
    },
    files: &[
      file("encoder", "encoder.int8.onnx", 131_201_478, "6765b0fbe07f9912b8cc6d330c6cf6a7f98473b08df4d6054c43295da9b48d69"),
      file("decoder", "decoder.int8.onnx", 3_382_423, "cbb4384f315043caec22a44e76739cd727a24ff61e0045f9cacf42e4f5e937f6"),
      file("joiner", "joiner.int8.onnx", 831_155, "51cc441a2289bf281850106ebdc2d5226caa459fde1510f6f936cc94198369b5"),
      file("tokens", "tokens.txt", 795, "c79cd022b6b04e090901e1acce380ad9476bb6a3694112199b835256bb079b9b"),
    ],
    license: "CC-BY-4.0",
  },
  CatalogModel {
    id: "whisper-turbo",
    name: "Whisper Turbo",
    kind: ModelKind::Whisper,
    languages: WHISPER_LARGE_LANGS,
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-whisper-turbo",
      rev: "2ca6ff69fc878651b770880507669577ac41c2ff",
    },
    files: &[
      file("encoder", "turbo-encoder.int8.onnx", 674_716_297, "b02dcdf54f348741e93fe732b67d933c8dcb6735655f710640143081db38878b"),
      file("decoder", "turbo-decoder.int8.onnx", 361_080_764, "20accd02388482eb3a46bd615631adfdc85e1eb2c7db9ea3f02a40ffe6b81547"),
      file("tokens", "turbo-tokens.txt", 816_730, "b34b360dbb493e781e479794586d661700670d65564001f23024971d1f2fa126"),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "whisper-large-v3",
    name: "Whisper Large v3",
    kind: ModelKind::Whisper,
    languages: WHISPER_LARGE_LANGS,
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-whisper-large-v3",
      rev: "2a6507094dd6020d939d78e3f1834a1d06267fca",
    },
    files: &[
      file("encoder", "large-v3-encoder.int8.onnx", 766_671_985, "d531cf17248acc43e8c09b472a0877055e770877857a5332fc1304b36534ec85"),
      file("decoder", "large-v3-decoder.int8.onnx", 1_008_265_203, "ebc6bfd88e162a46cb3edee8a7e727e1dcbc65cabecb19e2573695e4d495e1af"),
      file("tokens", "large-v3-tokens.txt", 816_730, "b34b360dbb493e781e479794586d661700670d65564001f23024971d1f2fa126"),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "whisper-medium",
    name: "Whisper Medium",
    kind: ModelKind::Whisper,
    languages: WHISPER_LARGE_LANGS,
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-whisper-medium",
      rev: "8c31d28503847560985df21f90e14f0c736e075e",
    },
    files: &[
      file("encoder", "medium-encoder.int8.onnx", 374_196_283, "1c54582b4d829de0089f6cb63bbbdb3bf7555398bacaf855fbecf1a84dfd193e"),
      file("decoder", "medium-decoder.int8.onnx", 571_059_257, "595d00a338a365a7bfa0ca7f296cabc639583bef770ab6130df90f49a6412747"),
      file("tokens", "medium-tokens.txt", 816_730, "b34b360dbb493e781e479794586d661700670d65564001f23024971d1f2fa126"),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "whisper-medium-en",
    name: "Whisper Medium (English)",
    kind: ModelKind::Whisper,
    languages: &["en"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-whisper-medium.en",
      rev: "251ab4521f354490e8f2c206fbd0b7f3f6b0a7ec",
    },
    files: &[
      file("encoder", "medium.en-encoder.int8.onnx", 374_196_283, "5a8e3a36619e0b67db9320eef3152db59d4b440f5ce0212d2c162a61b750bf80"),
      file("decoder", "medium.en-decoder.int8.onnx", 571_055_161, "7303be339ed4e51f4ffb7ae84f3803b10cf8e67e1dcf8a98cb4d843f0dea0141"),
      file("tokens", "medium.en-tokens.txt", 835_554, "306cd27f03c1a714eca7108e03d66b7dc042abe8c258b44c199a7ed9838dd930"),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "whisper-small",
    name: "Whisper Small",
    kind: ModelKind::Whisper,
    languages: WHISPER_SMALL_LANGS,
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-whisper-small",
      rev: "8f3c18b358db4d1f2fc1eae49d75cd20989e4309",
    },
    files: &[
      file("encoder", "small-encoder.int8.onnx", 112_442_483, "4cbe7b22fa9026b843b60a68640c747de05bafb1a11b57edc0e66c232d9f33a9"),
      file("decoder", "small-decoder.int8.onnx", 262_226_114, "acad50b5c782696e91b55914cc5ab4f756f1532f76e22aa6fc615f39fb69a8ee"),
      file("tokens", "small-tokens.txt", 816_730, "b34b360dbb493e781e479794586d661700670d65564001f23024971d1f2fa126"),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "whisper-small-en",
    name: "Whisper Small (English)",
    kind: ModelKind::Whisper,
    languages: &["en"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-whisper-small.en",
      rev: "d9533f69affd85061aee349af7fea5cb2996dbbe",
    },
    files: &[
      file("encoder", "small.en-encoder.int8.onnx", 112_442_483, "8bdac288f369aa94ee2194059238c465ed82ea9d47ee8fa4a8c0a891873e462f"),
      file("decoder", "small.en-decoder.int8.onnx", 262_223_042, "710ccf890e10f3faa15f51ec346081a2723c9f3adb6e4da81c6573a5a6f877fb"),
      file("tokens", "small.en-tokens.txt", 835_554, "306cd27f03c1a714eca7108e03d66b7dc042abe8c258b44c199a7ed9838dd930"),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "whisper-base",
    name: "Whisper Base",
    kind: ModelKind::Whisper,
    languages: &["multi", "en", "es", "it"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-whisper-base",
      rev: "bb53ee204431c90d314c1cc08d28d23e5b7927cc",
    },
    files: &[
      file("encoder", "base-encoder.int8.onnx", 29_120_534, "0b8fb1304b6109976038efff5ace81720e00386f3ff6b54ee8c75291ca0a1e11"),
      file("decoder", "base-decoder.int8.onnx", 130_672_026, "9759d217388a01b3a4c7c15533201067b48ae819c4daafc8624e64b9409dc02d"),
      file("tokens", "base-tokens.txt", 816_730, "b34b360dbb493e781e479794586d661700670d65564001f23024971d1f2fa126"),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "whisper-base-en",
    name: "Whisper Base (English)",
    kind: ModelKind::Whisper,
    languages: &["en"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-whisper-base.en",
      rev: "59eea950fc76df2453efb57e6c0fd334548e8ffe",
    },
    files: &[
      file("encoder", "base.en-encoder.int8.onnx", 29_120_534, "ef6b936f4c9b1d90a3b68634b60c4ed8576b26172b33c2535ec0e933c9edb823"),
      file("decoder", "base.en-decoder.int8.onnx", 130_669_978, "f7162ad6db2dbef16cfaeaa7f945b9d7dd9c1b8d472f6aca82f2273d185e4d41"),
      file("tokens", "base.en-tokens.txt", 835_554, "306cd27f03c1a714eca7108e03d66b7dc042abe8c258b44c199a7ed9838dd930"),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "whisper-tiny",
    name: "Whisper Tiny",
    kind: ModelKind::Whisper,
    languages: &["multi", "en", "es"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-whisper-tiny",
      rev: "65176e2deb88badc814a94058666cadccc29b61c",
    },
    files: &[
      file("encoder", "tiny-encoder.int8.onnx", 12_937_772, "d24fb083ae3b1041fc24e97971d60e280c9342201fbb67b0ab428a8b4a51a434"),
      file("decoder", "tiny-decoder.int8.onnx", 89_855_401, "d2fece8dd42771f1df975c6c0445770d0c292bf7547c2cae04a6c0cc57540925"),
      file("tokens", "tiny-tokens.txt", 816_730, "b34b360dbb493e781e479794586d661700670d65564001f23024971d1f2fa126"),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "whisper-tiny-en",
    name: "Whisper Tiny (English)",
    kind: ModelKind::Whisper,
    languages: &["en"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-whisper-tiny.en",
      rev: "d026532c022fa99fd789d6b32446a1df7b6bfc43",
    },
    files: &[
      file("encoder", "tiny.en-encoder.int8.onnx", 12_937_772, "0ce578b827c94a961aacb8fa14b02f096504b337e5c94be37c36238cbe3e8bc6"),
      file("decoder", "tiny.en-decoder.int8.onnx", 89_853_865, "06c0e6ff6348d427e51839219d1c886c18cfdf411e629e33f5e1679bff9c1527"),
      file("tokens", "tiny.en-tokens.txt", 835_554, "306cd27f03c1a714eca7108e03d66b7dc042abe8c258b44c199a7ed9838dd930"),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "distil-whisper-large-v3.5",
    name: "Distil-Whisper Large v3.5 (English)",
    kind: ModelKind::Whisper,
    languages: &["en"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-whisper-distil-large-v3.5",
      rev: "2933155a51e982b9ebd9ca3b7fb80fc78162fa64",
    },
    files: &[
      file("encoder", "distil-large-v3.5-encoder.int8.onnx", 668_147_593, "c30727100ba2dfbe64f2309a2ec44a5049b005d8a6c0acce49952f635083cbdb"),
      file("decoder", "distil-large-v3.5-decoder.int8.onnx", 314_871_757, "7cef98c7b5491ba52a520fd61da957a62e6e64c702c6c8aeac8cdfb146cbdb6a"),
      file("tokens", "distil-large-v3.5-tokens.txt", 816_730, "b34b360dbb493e781e479794586d661700670d65564001f23024971d1f2fa126"),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "distil-whisper-large-v3",
    name: "Distil-Whisper Large v3 (English)",
    kind: ModelKind::Whisper,
    languages: &["en"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-whisper-distil-large-v3",
      rev: "589fe9a7ce0ca4f026841dd4ca073f8dbd09d5b8",
    },
    files: &[
      file("encoder", "distil-large-v3-encoder.int8.onnx", 668_147_591, "b4220a4b837a6d32d2b1e37fbe01123f511655bce92321691385728b49f5ea76"),
      file("decoder", "distil-large-v3-decoder.int8.onnx", 314_871_757, "c7b673da3cda2a0ef3f97c1c26a47ed9879a387cf15a59aca907ee08eb6a6a5b"),
      file("tokens", "distil-large-v3-tokens.txt", 816_730, "b34b360dbb493e781e479794586d661700670d65564001f23024971d1f2fa126"),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "distil-whisper-medium-en",
    name: "Distil-Whisper Medium (English)",
    kind: ModelKind::Whisper,
    languages: &["en"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-whisper-distil-medium.en",
      rev: "7cefcb28aa6bcb1f6946066b5fad5fea7098c99e",
    },
    files: &[
      file("encoder", "distil-medium.en-encoder.int8.onnx", 327_915_521, "8d80f145ebcfb1a34f6bf3d27be0f298eed4305531c8a31cf6b5d8eff266a384"),
      file("decoder", "distil-medium.en-decoder.int8.onnx", 244_743_630, "d2b315f45ee321ef6f21eb08c8c24d0855d8933fbaaa3301219bf5f710ad7df2"),
      file("tokens", "distil-medium.en-tokens.txt", 835_554, "306cd27f03c1a714eca7108e03d66b7dc042abe8c258b44c199a7ed9838dd930"),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "distil-whisper-small-en",
    name: "Distil-Whisper Small (English)",
    kind: ModelKind::Whisper,
    languages: &["en"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-whisper-distil-small.en",
      rev: "0492324bca9e12a6fca0089bb846f2dd723b50d8",
    },
    files: &[
      file("encoder", "distil-small.en-encoder.int8.onnx", 102_961_431, "397a76d2308c2c1ec91a4ecc12f20fede69bb17be41a1cef050993520328beca"),
      file("decoder", "distil-small.en-decoder.int8.onnx", 195_079_097, "3074092bca078786ecda9c9e88449f14e9ebde1d60be4d41de8cacda55e065e0"),
      file("tokens", "distil-small.en-tokens.txt", 835_554, "306cd27f03c1a714eca7108e03d66b7dc042abe8c258b44c199a7ed9838dd930"),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "moonshine-base-en",
    name: "Moonshine Base (English)",
    kind: ModelKind::Moonshine,
    languages: &["en"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-moonshine-base-en-quantized-2026-02-27.tar.bz2",
      size: 111_266_225,
      sha256: "43232c1d13013d37317163baec3135bd771a186a4356f28c889bab453bb0e891",
      root: "sherpa-onnx-moonshine-base-en-quantized-2026-02-27",
    },
    files: &[
      sized("encoder", "encoder_model.ort", 31_326_816),
      sized("merged_decoder", "decoder_model_merged.ort", 109_424_400),
      sized("tokens", "tokens.txt", 549_350),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "moonshine-tiny-en",
    name: "Moonshine Tiny (English)",
    kind: ModelKind::Moonshine,
    languages: &["en"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-moonshine-tiny-en-quantized-2026-02-27.tar.bz2",
      size: 29_858_559,
      sha256: "9ec31b342d8fa3240c3b81b8f82e1cf7e3ac467c93ca5a999b741d5887164f8d",
      root: "sherpa-onnx-moonshine-tiny-en-quantized-2026-02-27",
    },
    files: &[
      sized("encoder", "encoder_model.ort", 13_281_600),
      sized("merged_decoder", "decoder_model_merged.ort", 30_412_256),
      sized("tokens", "tokens.txt", 549_350),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "moonshine-base-es",
    name: "Moonshine (Spanish)",
    kind: ModelKind::Moonshine,
    languages: &["es"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-moonshine-base-es-quantized-2026-02-27.tar.bz2",
      size: 50_846_902,
      sha256: "850c3dcc5dfccc8b1feb10bb221b11d6039b6f5c626241729f46863771016383",
      root: "sherpa-onnx-moonshine-base-es-quantized-2026-02-27",
    },
    files: &[
      sized("encoder", "encoder_model.ort", 20_964_320),
      sized("merged_decoder", "decoder_model_merged.ort", 43_612_200),
      sized("tokens", "tokens.txt", 532_090),
    ],
    license: "Moonshine Community License",
  },
  CatalogModel {
    id: "moonshine-base-ar",
    name: "Moonshine (Arabic)",
    kind: ModelKind::Moonshine,
    languages: &["ar"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-moonshine-base-ar-quantized-2026-02-27.tar.bz2",
      size: 119_143_068,
      sha256: "4d3f16fe354d8536d3323d3eaf4c12fafb1976b5dc0a12862ab25934da94285b",
      root: "sherpa-onnx-moonshine-base-ar-quantized-2026-02-27",
    },
    files: &[
      sized("encoder", "encoder_model.ort", 31_326_824),
      sized("merged_decoder", "decoder_model_merged.ort", 109_424_552),
      sized("tokens", "tokens.txt", 549_350),
    ],
    license: "Moonshine Community License",
  },
  CatalogModel {
    id: "moonshine-base-ja",
    name: "Moonshine (Japanese)",
    kind: ModelKind::Moonshine,
    languages: &["ja"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-moonshine-base-ja-quantized-2026-02-27.tar.bz2",
      size: 104_162_109,
      sha256: "c66eb87d229a52d43bbe3b7c52a1d23f6208c0fd7c2e81802ecbda83b192798a",
      root: "sherpa-onnx-moonshine-base-ja-quantized-2026-02-27",
    },
    files: &[
      sized("encoder", "encoder_model.ort", 31_326_816),
      sized("merged_decoder", "decoder_model_merged.ort", 109_424_424),
      sized("tokens", "tokens.txt", 549_350),
    ],
    license: "Moonshine Community License",
  },
  CatalogModel {
    id: "moonshine-tiny-ja",
    name: "Moonshine Tiny (Japanese)",
    kind: ModelKind::Moonshine,
    languages: &["ja"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-moonshine-tiny-ja-quantized-2026-02-27.tar.bz2",
      size: 47_889_565,
      sha256: "880305c9a6c33572ab269ff9731977ea42ca34c8cffcdd5d99558a9ea2b47cc2",
      root: "sherpa-onnx-moonshine-tiny-ja-quantized-2026-02-27",
    },
    files: &[
      sized("encoder", "encoder_model.ort", 13_238_184),
      sized("merged_decoder", "decoder_model_merged.ort", 58_327_272),
      sized("tokens", "tokens.txt", 549_350),
    ],
    license: "Moonshine Community License",
  },
  CatalogModel {
    id: "moonshine-tiny-ko",
    name: "Moonshine Tiny (Korean)",
    kind: ModelKind::Moonshine,
    languages: &["ko"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-moonshine-tiny-ko-quantized-2026-02-27.tar.bz2",
      size: 49_153_415,
      sha256: "d3b6c5390a7859c9ef20ff4f20b0766fcbad1dc06c0f509fe4840a3a302112dc",
      root: "sherpa-onnx-moonshine-tiny-ko-quantized-2026-02-27",
    },
    files: &[
      sized("encoder", "encoder_model.ort", 13_238_176),
      sized("merged_decoder", "decoder_model_merged.ort", 58_327_336),
      sized("tokens", "tokens.txt", 549_350),
    ],
    license: "Moonshine Community License",
  },
  CatalogModel {
    id: "moonshine-base-uk",
    name: "Moonshine (Ukrainian)",
    kind: ModelKind::Moonshine,
    languages: &["uk"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-moonshine-base-uk-quantized-2026-02-27.tar.bz2",
      size: 111_037_143,
      sha256: "d8489b159afabf4cfb734002ae8a5361781ff8a381fa18d284d32d1eeb1c0376",
      root: "sherpa-onnx-moonshine-base-uk-quantized-2026-02-27",
    },
    files: &[
      sized("encoder", "encoder_model.ort", 31_326_816),
      sized("merged_decoder", "decoder_model_merged.ort", 109_424_424),
      sized("tokens", "tokens.txt", 549_350),
    ],
    license: "Moonshine Community License",
  },
  CatalogModel {
    id: "moonshine-base-vi",
    name: "Moonshine (Vietnamese)",
    kind: ModelKind::Moonshine,
    languages: &["vi"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-moonshine-base-vi-quantized-2026-02-27.tar.bz2",
      size: 106_719_129,
      sha256: "97b53f5fb75a2a9dd6327440a34f45fc06c927ed2fa7217e5ebe54058e269416",
      root: "sherpa-onnx-moonshine-base-vi-quantized-2026-02-27",
    },
    files: &[
      sized("encoder", "encoder_model.ort", 31_326_816),
      sized("merged_decoder", "decoder_model_merged.ort", 109_424_520),
      sized("tokens", "tokens.txt", 549_350),
    ],
    license: "Moonshine Community License",
  },
  CatalogModel {
    id: "moonshine-base-zh",
    name: "Moonshine (Chinese)",
    kind: ModelKind::Moonshine,
    languages: &["zh"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-moonshine-base-zh-quantized-2026-02-27.tar.bz2",
      size: 99_905_420,
      sha256: "6495d4240f66bcdd2bbbbfefaa687c463150d467854001270934e88940b733c1",
      root: "sherpa-onnx-moonshine-base-zh-quantized-2026-02-27",
    },
    files: &[
      sized("encoder", "encoder_model.ort", 31_326_816),
      sized("merged_decoder", "decoder_model_merged.ort", 109_424_520),
      sized("tokens", "tokens.txt", 549_350),
    ],
    license: "Moonshine Community License",
  },
  CatalogModel {
    id: "qwen3-asr",
    name: "Qwen3-ASR",
    kind: ModelKind::Qwen3Asr,
    languages: QWEN3_ASR_LANGS,
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2",
      size: 878_702_423,
      sha256: "393f8a14e2f5fb96746aaab342997a40641001fbd5bf9592a080a8329178ee96",
      root: "sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25",
    },
    files: &[
      sized("conv_frontend", "conv_frontend.onnx", 0),
      sized("encoder", "encoder.int8.onnx", 0),
      sized("decoder", "decoder.int8.onnx", 0),
      sized("tokenizer", "tokenizer", 0),
    ],
    license: "Apache-2.0",
  },
  CatalogModel {
    id: "cohere-transcribe",
    name: "Cohere Transcribe",
    kind: ModelKind::CohereTranscribe,
    languages: COHERE_LANGS,
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-cohere-transcribe-14-lang-int8-2026-04-01.tar.bz2",
      size: 1_699_791_751,
      sha256: "bd582588d50685a795dcd2807ab77e11361b8312d96c53884682def45ab4206d",
      root: "sherpa-onnx-cohere-transcribe-14-lang-int8-2026-04-01",
    },
    files: &[
      sized("encoder", "encoder.int8.onnx", 3_090_822),
      sized("decoder", "decoder.int8.onnx", 153_250_705),
      sized("tokens", "tokens.txt", 207_437),
      sized("external_data", "encoder.int8.onnx.data", 2_731_503_072),
    ],
    license: "Apache-2.0",
  },
  CatalogModel {
    id: "omnilingual-1b",
    name: "Omnilingual 1B",
    kind: ModelKind::Omnilingual,
    languages: OMNILINGUAL_LANGS,
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-omnilingual-asr-1600-languages-1B-ctc-int8-2025-11-12",
      rev: "17db76eab583b0b868ffb0df104ab879145087e5",
    },
    files: &[
      file("model", "model.int8.onnx", 1_031_628_252, "f7b74c964039162423b83e3fa950ce24810c9a635d9ff8468b5f4d142b7c1e8c"),
      file("tokens", "tokens.txt", 86_423, "a7a044c52cb29cbe8b0dc1953e92cefd4ca16b0ed968177b6beab21f9a7d0b31"),
    ],
    license: "Apache-2.0",
  },
  CatalogModel {
    id: "omnilingual-300m",
    name: "Omnilingual 300M",
    kind: ModelKind::Omnilingual,
    languages: OMNILINGUAL_LANGS,
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-omnilingual-asr-1600-languages-300M-ctc-int8-2025-11-12",
      rev: "6abf1ece20cd2308bdb7d13cd78ec1c44fa4c094",
    },
    files: &[
      file("model", "model.int8.onnx", 365_352_120, "e7c4e54ee4c4c47829cc6667d5d00ed8ea7bef1dcfeef0fce766f77752a2726c"),
      file("tokens", "tokens.txt", 86_423, "a7a044c52cb29cbe8b0dc1953e92cefd4ca16b0ed968177b6beab21f9a7d0b31"),
    ],
    license: "Apache-2.0",
  },
  CatalogModel {
    id: "dolphin-small",
    name: "Dolphin Small",
    kind: ModelKind::Dolphin,
    languages: DOLPHIN_LANGS,
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-dolphin-small-ctc-multi-lang-int8-2025-04-02",
      rev: "c8b6689509acfcd744c04e5e169164f9ac4cae32",
    },
    files: &[
      file("model", "model.int8.onnx", 249_658_954, "c1afcb9265de0ebd853eb8f570b371f399a6f9b2b9af9a3cb17c2e509171e697"),
      file("tokens", "tokens.txt", 504_662, "c3788261a51df1899ea4b210b552cd42139204de72c0ad60f6cebb199078872e"),
    ],
    license: "Apache-2.0",
  },
  CatalogModel {
    id: "dolphin-base",
    name: "Dolphin Base",
    kind: ModelKind::Dolphin,
    languages: DOLPHIN_LANGS,
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-dolphin-base-ctc-multi-lang-int8-2025-04-02",
      rev: "1f3a53d0ecf658f8b0974e2cfde368eee40732fa",
    },
    files: &[
      file("model", "model.int8.onnx", 103_729_802, "a3aa46c97f3f60f135ff949793cb05fabe7a0b3c484dc2e3cc699d354ee11b76"),
      file("tokens", "tokens.txt", 504_662, "c3788261a51df1899ea4b210b552cd42139204de72c0ad60f6cebb199078872e"),
    ],
    license: "Apache-2.0",
  },
  CatalogModel {
    id: "fire-red-asr2",
    name: "FireRed ASR 2",
    kind: ModelKind::FireRedAsr,
    languages: &["zh"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-fire-red-asr2-zh_en-int8-2026-02-26.tar.bz2",
      size: 838_589_068,
      sha256: "43015b3f1643a5688b4821e8ed323473d38b798c4ec291471fe00df1bcfc4f1c",
      root: "sherpa-onnx-fire-red-asr2-zh_en-int8-2026-02-26",
    },
    files: &[
      sized("encoder", "encoder.int8.onnx", 817_286_833),
      sized("decoder", "decoder.int8.onnx", 417_291_928),
      sized("tokens", "tokens.txt", 79_172),
    ],
    license: "Apache-2.0",
  },
  CatalogModel {
    id: "fire-red-asr2-ctc",
    name: "FireRed ASR 2 CTC",
    kind: ModelKind::FireRedAsrCtc,
    languages: &["zh"],
    source: Source::Archive {
      path: "k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-fire-red-asr2-ctc-zh_en-int8-2026-02-25.tar.bz2",
      size: 520_516_278,
      sha256: "1da8b737ecc5e29f36759a4460c754863e7c919a4ba325aea187331fbfc83274",
      root: "sherpa-onnx-fire-red-asr2-ctc-zh_en-int8-2026-02-25",
    },
    files: &[
      sized("model", "model.int8.onnx", 775_861_420),
      sized("tokens", "tokens.txt", 79_172),
    ],
    license: "Apache-2.0",
  },
  CatalogModel {
    id: "gigaam-v3-ru",
    name: "GigaAM v3 (Russian)",
    kind: ModelKind::Transducer,
    languages: &["ru"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-nemo-transducer-giga-am-v3-russian-2025-12-16",
      rev: "0424d62637b37d8b7b1eb0f75a04d78dec55fdcc",
    },
    files: &[
      file("encoder", "encoder.int8.onnx", 224_570_814, "0ec54e5f130a91c0f228d21795ede10b31c992a1f73e45dcd827e843a71a9b50"),
      file("decoder", "decoder.onnx", 3_331_651, "5d1cfef155d8e07f213bd2f3229b5f6c5c29355c7fa5c1b9eb62cb1925725254"),
      file("joiner", "joiner.onnx", 1_440_448, "fd1d02f45c2ad3d6b67cc149811ad794ab4b020ed49a0a9e2790a8619d1cddd8"),
      file("tokens", "tokens.txt", 196, "17cc514451bcceac9c280068c71502f8448f99e9fb1456b8d0761651fd0392f2"),
    ],
    license: "MIT",
  },
  CatalogModel {
    id: "zipformer-ru",
    name: "Zipformer (Russian)",
    kind: ModelKind::Transducer,
    languages: &["ru"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-zipformer-ru-int8-2025-04-20",
      rev: "641de8d322c05b9087ad2927ccda4bda3cccc159",
    },
    files: &[
      file("encoder", "encoder.int8.onnx", 70_876_638, "eb6c12fbad810d5bc3e427802e604604c69b5943a91feebc43424dd09d9ec407"),
      file("decoder", "decoder.onnx", 2_093_080, "dcbe1ffa0211e77ca6d3a80164df13fbda3ec00e47d12b9f449f89572df12136"),
      file("joiner", "joiner.int8.onnx", 259_417, "93f2e1d12b78d53e7802f1606488c14bb3d764b15fadf5ef6c022f6ba1fa40f7"),
      file("tokens", "tokens.txt", 6_388, "93bbbc0bae6b78c0bbb743d4aa9fded3bb5ff3aac5f0200e3a769a5a05e0fdf6"),
    ],
    license: "Apache-2.0",
  },
  CatalogModel {
    id: "zipformer-vi",
    name: "Zipformer (Vietnamese)",
    kind: ModelKind::Transducer,
    languages: &["vi"],
    source: Source::HfFiles {
      repo: "csukuangfj/sherpa-onnx-zipformer-vi-int8-2025-04-20",
      rev: "b2745a435379992ad3f299635468db0c34918e1e",
    },
    files: &[
      file("encoder", "encoder-epoch-12-avg-8.int8.onnx", 70_876_129, "b3abdef7a660fea7faf5e076b3c7613b0fc98406707103784d018189bb522124"),
      file("decoder", "decoder-epoch-12-avg-8.onnx", 5_165_084, "d1d27cca84c824a8acf5ce6edf0f2c0880cfe295d2e69b95134de1707e1d9998"),
      file("joiner", "joiner-epoch-12-avg-8.int8.onnx", 1_033_417, "38ec49e1c18e4feb0cad4de13e25c83a866cf56f4a66f22e8ff579d591a69a46"),
      file("tokens", "tokens.txt", 25_847, "f536d03c2e95ebd2930cf0abec88e823bd17d3c1933da7ae6a82db3b80605e15"),
    ],
    license: "Apache-2.0",
  },
];

pub fn get(id: &str) -> Option<&'static CatalogModel> {
  CATALOG.iter().find(|m| m.id == id)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
  }

  #[test]
  fn catalog_entries_are_complete() {
    let mut ids = std::collections::HashSet::new();
    for m in CATALOG {
      assert!(ids.insert(m.id), "duplicate id {}", m.id);
      assert!(!m.languages.is_empty(), "{} has no languages", m.id);
      let roles: Vec<&str> = m
        .files
        .iter()
        .map(|f| f.role)
        .filter(|r| *r != "external_data")
        .collect();
      assert_eq!(roles, m.kind.roles(), "{} file roles", m.id);
      match m.source {
        Source::HfFiles { repo, rev } => {
          assert!(repo.contains('/') && rev.len() == 40, "{} source", m.id);
          for f in m.files {
            assert!(f.size > 0 && is_hex64(f.sha256), "{} file {} unpinned", m.id, f.name);
          }
        }
        Source::Archive {
          path,
          size,
          sha256,
          root,
        } => {
          assert!(path.ends_with(".tar.bz2") && size > 0 && is_hex64(sha256) && !root.is_empty());
        }
      }
    }
    assert!(get(DEFAULT_MODEL).is_some());
  }

  #[test]
  fn download_sizes_match_the_spec() {
    let mb = |id: &str| get(id).unwrap().download_bytes() / 1_000_000;
    assert_eq!(mb("parakeet-v3"), 670);
    assert_eq!(mb("parakeet-v2"), 661);
    assert_eq!(mb("whisper-turbo"), 1036);
    assert_eq!(mb("qwen3-asr"), 878);
    assert_eq!(mb("whisper-tiny"), 103);
  }

  #[test]
  fn english_only_models_have_no_auto_detect() {
    assert_eq!(get("parakeet-v2").unwrap().languages, &["en"]);
    assert_eq!(get("whisper-tiny-en").unwrap().languages, &["en"]);
    assert!(get("parakeet-v3").unwrap().languages.contains(&"multi"));
    assert!(get("sensevoice").is_none());
  }

  #[test]
  fn whisper_files_follow_the_model_size() {
    let m = get("whisper-tiny").unwrap();
    let files = m.files_in(Path::new("d"));
    assert_eq!(files["encoder"], Path::new("d").join("tiny-encoder.int8.onnx"));
    assert_eq!(files["tokens"], Path::new("d").join("tiny-tokens.txt"));
  }
}
