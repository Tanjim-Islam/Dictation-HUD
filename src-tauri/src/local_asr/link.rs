//! "Use a model on this PC": decides which catalog model a folder holds. A match must be exact:
//! every file name and size, plus the SHA-256 of the small files (token lists). Large files are
//! not hashed here, so linking stays instant; a load test follows before the path is saved.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::catalog::CatalogModel;
use super::store::layout_ok;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
  NotAModel,
  /// A speech model in a format this app cannot run.
  WrongFormat(&'static str),
  /// The right model family, but different files.
  OtherVersion(&'static str),
}

impl LinkError {
  pub fn message(&self) -> String {
    match self {
      LinkError::NotAModel => "This folder doesn't have a supported speech model.".into(),
      LinkError::WrongFormat(f) => format!("This is a {f} model. It can't be used here."),
      LinkError::OtherVersion(name) => format!("This looks like {name}, but a different version."),
    }
  }
}

/// Files under 2 MB are hashed during linking.
const SMALL: u64 = 2 * 1024 * 1024;

/// A folder can only be matched to a model whose files tell it apart: a pinned size, or a
/// folder entry (like Qwen3-ASR's `tokenizer`) that other models do not have.
fn verifiable(m: &CatalogModel) -> bool {
  m.files.iter().any(|f| f.size > 0 || !f.name.contains('.'))
}

fn exact(m: &CatalogModel, dir: &Path) -> bool {
  verifiable(m)
    && layout_ok(m, dir)
    && m
      .files
      .iter()
      .filter(|f| f.size > 0 && f.size < SMALL && !f.sha256.is_empty())
      .all(|f| std::fs::read(dir.join(f.name)).is_ok_and(|b| format!("{:x}", Sha256::digest(&b)) == f.sha256))
}

fn names_present(m: &CatalogModel, dir: &Path) -> bool {
  m.files.iter().all(|f| dir.join(f.name).exists())
}

/// How many pinned file sizes match; picks the closest family member (Parakeet v2 and v3 share
/// file names).
fn size_matches(m: &CatalogModel, dir: &Path) -> usize {
  m.files
    .iter()
    .filter(|f| f.size > 0 && std::fs::metadata(dir.join(f.name)).is_ok_and(|md| md.len() == f.size))
    .count()
}

fn foreign_format(dir: &Path) -> Option<&'static str> {
  let names: Vec<String> = std::fs::read_dir(dir)
    .ok()?
    .flatten()
    .map(|e| e.file_name().to_string_lossy().to_ascii_lowercase())
    .collect();
  let has = |n: &str| names.iter().any(|x| x == n);
  if has("model.bin")
    && has("config.json")
    && (has("vocabulary.txt") || has("vocabulary.json") || has("tokenizer.json"))
  {
    return Some("faster-whisper");
  }
  if names.iter().any(|n| n.starts_with("ggml") && n.ends_with(".bin")) {
    return Some("whisper.cpp");
  }
  if names.iter().any(|n| n.ends_with(".gguf")) {
    return Some("GGUF");
  }
  if names.iter().any(|n| n.ends_with(".nemo")) {
    return Some("NeMo");
  }
  if has("model.safetensors") {
    return Some("Hugging Face Transformers");
  }
  None
}

/// The folder itself, then each folder directly inside it (a picked parent folder still works).
fn candidates(dir: &Path) -> Vec<PathBuf> {
  let mut out = vec![dir.to_path_buf()];
  if let Ok(it) = std::fs::read_dir(dir) {
    out.extend(it.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
  }
  out
}

pub fn identify<'a>(dir: &Path, catalog: &'a [CatalogModel]) -> Result<(&'a CatalogModel, PathBuf), LinkError> {
  let mut other: Option<(usize, &'static str)> = None;
  for d in candidates(dir) {
    for m in catalog {
      if exact(m, &d) {
        return Ok((m, d));
      }
      if names_present(m, &d) {
        let score = size_matches(m, &d);
        if other.is_none_or(|(best, _)| score > best) {
          other = Some((score, m.name));
        }
      }
    }
  }
  if let Some((_, name)) = other {
    return Err(LinkError::OtherVersion(name));
  }
  Err(
    candidates(dir)
      .iter()
      .find_map(|d| foreign_format(d))
      .map_or(LinkError::NotAModel, LinkError::WrongFormat),
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::local_asr::catalog::{get, CATALOG};

  fn fill(dir: &Path, m: &CatalogModel) {
    std::fs::create_dir_all(dir).unwrap();
    for f in m.files {
      let p = dir.join(f.name);
      if f.size == 0 && !f.name.contains('.') {
        std::fs::create_dir_all(p).unwrap();
      } else {
        std::fs::File::create(p).unwrap().set_len(f.size).unwrap();
      }
    }
  }

  #[test]
  fn wrong_formats_get_a_reason() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    std::fs::write(d.join("model.bin"), b"x").unwrap();
    std::fs::write(d.join("config.json"), b"{}").unwrap();
    std::fs::write(d.join("vocabulary.txt"), b"x").unwrap();
    assert_eq!(
      identify(d, CATALOG).unwrap_err(),
      LinkError::WrongFormat("faster-whisper")
    );
    let g = tmp.path().join("g");
    std::fs::create_dir_all(&g).unwrap();
    std::fs::write(g.join("ggml-base.en.bin"), b"x").unwrap();
    assert_eq!(
      identify(&g, CATALOG).unwrap_err(),
      LinkError::WrongFormat("whisper.cpp")
    );
    let e = tmp.path().join("empty");
    std::fs::create_dir_all(&e).unwrap();
    assert_eq!(identify(&e, CATALOG).unwrap_err(), LinkError::NotAModel);
  }

  #[test]
  fn same_names_with_other_sizes_is_another_version() {
    let tmp = tempfile::tempdir().unwrap();
    let m = get("parakeet-v2").unwrap();
    fill(tmp.path(), m);
    std::fs::File::create(tmp.path().join("encoder.int8.onnx"))
      .unwrap()
      .set_len(42)
      .unwrap();
    assert_eq!(
      identify(tmp.path(), CATALOG).unwrap_err(),
      LinkError::OtherVersion("Parakeet v2 (English)")
    );
  }

  #[test]
  fn archive_layout_is_found_one_level_down() {
    let tmp = tempfile::tempdir().unwrap();
    let q = get("qwen3-asr").unwrap();
    let inner = tmp.path().join("sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25");
    fill(&inner, q);
    let (found, dir) = identify(tmp.path(), CATALOG).unwrap();
    assert_eq!(found.id, "qwen3-asr");
    assert_eq!(dir, inner);
  }
}
