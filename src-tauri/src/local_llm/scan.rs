//! Folder fallback: lists text models straight from disk when a server cannot start.
//! Entries are shown but not usable until the server answers.

use std::path::{Path, PathBuf};

use super::detect::Probe;
use super::{is_text_model_name, LocalApp, LocalModel};

/// Where the app keeps models, honoring the user's custom folder.
pub fn models_dir(app: LocalApp, p: &dyn Probe) -> Option<PathBuf> {
  let home = p.env("USERPROFILE").map(PathBuf::from);
  match app {
    LocalApp::Ollama => p
      .env("OLLAMA_MODELS")
      .map(PathBuf::from)
      .or_else(|| home.map(|h| h.join(r".ollama\models"))),
    LocalApp::LmStudio => {
      let home = home?;
      let configured = p
        .read(&home.join(r".lmstudio\settings.json"))
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["downloadsFolder"].as_str().map(PathBuf::from))
        .filter(|d| !d.as_os_str().is_empty());
      Some(configured.unwrap_or_else(|| home.join(r".lmstudio\models")))
    }
  }
}

pub fn scan(app: LocalApp, dir: &Path) -> Vec<LocalModel> {
  let mut out = match app {
    LocalApp::Ollama => scan_ollama(dir),
    LocalApp::LmStudio => scan_lmstudio(dir),
  };
  out.sort_by(|a, b| a.name.cmp(&b.name));
  out
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
  std::fs::read_dir(dir)
    .map(|it| it.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
    .unwrap_or_default()
}

fn files(dir: &Path) -> Vec<PathBuf> {
  std::fs::read_dir(dir)
    .map(|it| it.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect())
    .unwrap_or_default()
}

fn file_name(p: &Path) -> String {
  p.file_name()
    .map(|n| n.to_string_lossy().into_owned())
    .unwrap_or_default()
}

/// `manifests/<host>/<namespace>/<model>/<tag>`; `registry.ollama.ai/library` models are bare names.
fn scan_ollama(dir: &Path) -> Vec<LocalModel> {
  let mut out = Vec::new();
  for host in subdirs(&dir.join("manifests")) {
    for ns in subdirs(&host) {
      for model in subdirs(&ns) {
        for tag in files(&model) {
          let (h, n, m, t) = (file_name(&host), file_name(&ns), file_name(&model), file_name(&tag));
          if !is_text_model_name(&m) {
            continue;
          }
          let id = match (h.as_str(), n.as_str()) {
            ("registry.ollama.ai", "library") => format!("{m}:{t}"),
            ("registry.ollama.ai", _) => format!("{n}/{m}:{t}"),
            _ => format!("{h}/{n}/{m}:{t}"),
          };
          let size_bytes = std::fs::read_to_string(&tag)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
              v["layers"]
                .as_array()
                .map(|l| l.iter().filter_map(|x| x["size"].as_u64()).sum())
            })
            .unwrap_or(0);
          out.push(LocalModel {
            name: id.clone(),
            id,
            size_bytes,
            usable: false,
            thinking: false,
          });
        }
      }
    }
  }
  out
}

/// `<downloads>/<publisher>/<repo>/<file>.gguf`. Skips vision projectors, embedding models, and
/// every shard after the first of a split model.
fn scan_lmstudio(dir: &Path) -> Vec<LocalModel> {
  let mut out = Vec::new();
  for publisher in subdirs(dir) {
    for repo in subdirs(&publisher) {
      for file in files(&repo) {
        let name = file_name(&file);
        let lower = name.to_ascii_lowercase();
        if !lower.ends_with(".gguf") || lower.starts_with("mmproj") {
          continue;
        }
        if !is_text_model_name(&name) || !is_text_model_name(&file_name(&repo)) {
          continue;
        }
        if lower.contains("-of-") && !lower.contains("-00001-of-") {
          continue;
        }
        let id = format!("{}/{}/{}", file_name(&publisher), file_name(&repo), name);
        let stem = name[..name.len() - ".gguf".len()].to_string();
        let size_bytes = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
        out.push(LocalModel {
          id,
          name: stem,
          size_bytes,
          usable: false,
          thinking: false,
        });
      }
    }
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;

  fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
  }

  fn manifest(size: u64) -> String {
    format!(
      r#"{{"layers":[{{"mediaType":"application/vnd.ollama.image.model","size":{size}}},{{"mediaType":"application/vnd.ollama.image.params","size":10}}]}}"#
    )
  }

  #[test]
  fn ollama_manifests_become_model_ids() {
    let dir = tempfile::tempdir().unwrap();
    let m = dir.path().join("manifests");
    write(
      &m.join("registry.ollama.ai/library/gemma3/4b"),
      manifest(3_000).as_bytes(),
    );
    write(
      &m.join("registry.ollama.ai/someone/tiny/latest"),
      manifest(100).as_bytes(),
    );
    write(
      &m.join("registry.ollama.ai/library/nomic-embed-text/latest"),
      manifest(50).as_bytes(),
    );
    write(&m.join("hf.co/org/repo-GGUF/Q4_K_M"), manifest(700).as_bytes());
    let found = scan(LocalApp::Ollama, dir.path());
    let ids: Vec<_> = found.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
      ids,
      vec!["gemma3:4b", "hf.co/org/repo-GGUF:Q4_K_M", "someone/tiny:latest"]
    );
    assert_eq!(found[0].size_bytes, 3_010);
    assert!(found.iter().all(|m| !m.usable));
  }

  #[test]
  fn lmstudio_ggufs_skip_projectors_embeddings_and_extra_shards() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(
      &d.join("lmstudio-community/gemma-3-12b-it-GGUF/gemma-3-12b-it-Q4_K_M.gguf"),
      &[0; 30],
    );
    write(
      &d.join("lmstudio-community/gemma-3-12b-it-GGUF/mmproj-model-f16.gguf"),
      &[0; 5],
    );
    write(
      &d.join("nomic-ai/nomic-embed-text-v1.5-GGUF/nomic-embed-text-v1.5.Q8_0.gguf"),
      &[0; 5],
    );
    write(&d.join("big/model-GGUF/model-00001-of-00002.gguf"), &[0; 7]);
    write(&d.join("big/model-GGUF/model-00002-of-00002.gguf"), &[0; 7]);
    write(&d.join("notes/readme.txt"), b"x");
    let found = scan(LocalApp::LmStudio, d);
    let names: Vec<_> = found.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, vec!["gemma-3-12b-it-Q4_K_M", "model-00001-of-00002"]);
    assert_eq!(
      found[0].id,
      "lmstudio-community/gemma-3-12b-it-GGUF/gemma-3-12b-it-Q4_K_M.gguf"
    );
    assert_eq!(found[0].size_bytes, 30);
  }

  #[test]
  fn missing_dir_is_empty_not_an_error() {
    assert!(scan(LocalApp::Ollama, Path::new(r"Z:\definitely\missing")).is_empty());
  }
}
