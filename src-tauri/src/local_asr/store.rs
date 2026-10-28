//! Where local speech models live: the app's store (downloads), user-linked folders, and the
//! Hugging Face cache (exact repo and revision only). Deletes happen only inside the store.

use std::path::{Path, PathBuf};

use super::catalog::{CatalogModel, Source};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelState {
  Installed(PathBuf),
  Linked(PathBuf),
  /// Found in the Hugging Face cache; used in place, never deleted.
  Cached(PathBuf),
  /// Linked folder that no longer holds the model (moved, deleted, or drive unplugged).
  Missing(PathBuf),
  NotInstalled,
}

impl ModelState {
  pub fn dir(&self) -> Option<PathBuf> {
    match self {
      ModelState::Installed(d) | ModelState::Linked(d) | ModelState::Cached(d) => Some(d.clone()),
      _ => None,
    }
  }
}

/// Every expected file exists (directories for extension-less names) with the expected size.
pub fn layout_ok(m: &CatalogModel, dir: &Path) -> bool {
  m.files.iter().all(|f| {
    let p = dir.join(f.name);
    if f.size == 0 {
      p.exists()
    } else {
      std::fs::metadata(&p)
        .map(|md| md.is_file() && md.len() == f.size)
        .unwrap_or(false)
    }
  })
}

pub fn default_hf_hub(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
  env("HF_HUB_CACHE")
    .map(PathBuf::from)
    .or_else(|| env("HF_HOME").map(|h| PathBuf::from(h).join("hub")))
    .or_else(|| env("USERPROFILE").map(|h| PathBuf::from(h).join(r".cache\huggingface\hub")))
}

#[derive(Debug, Clone)]
pub struct ModelsDir {
  root: PathBuf,
  /// The default store while downloads go to a folder the user picked: models already there
  /// keep working.
  fallback: Option<PathBuf>,
  hf_hub: Option<PathBuf>,
}

fn inside(root: &Path, path: &Path) -> bool {
  let (Ok(root), Ok(p)) = (root.canonicalize(), path.canonicalize()) else {
    return false;
  };
  p.starts_with(&root) && p != root
}

impl ModelsDir {
  pub fn new(root: PathBuf, hf_hub: Option<PathBuf>) -> Self {
    Self {
      root,
      fallback: None,
      hf_hub,
    }
  }

  /// The store for the folder in settings: `folder` for downloads (this store still searched),
  /// or this store itself.
  pub fn select(&self, folder: Option<&Path>) -> Self {
    match folder {
      Some(f) if f != self.root => Self {
        root: f.to_path_buf(),
        fallback: Some(self.root.clone()),
        hf_hub: self.hf_hub.clone(),
      },
      _ => self.clone(),
    }
  }

  fn roots(&self) -> impl Iterator<Item = &Path> {
    std::iter::once(self.root.as_path()).chain(self.fallback.as_deref())
  }

  pub fn root(&self) -> &Path {
    &self.root
  }

  pub fn install_dir(&self, id: &str) -> PathBuf {
    self.root.join(id)
  }

  pub fn part_dir(&self, id: &str) -> PathBuf {
    self.root.join(format!("{id}.part"))
  }

  /// True only for paths strictly inside the store (after resolving `..` and links).
  pub fn contains(&self, path: &Path) -> bool {
    self.roots().any(|r| inside(r, path))
  }

  fn cached(&self, m: &CatalogModel) -> Option<PathBuf> {
    let Source::HfFiles { repo, rev } = m.source else {
      return None;
    };
    let dir = self
      .hf_hub
      .as_ref()?
      .join(format!("models--{}", repo.replace('/', "--")))
      .join("snapshots")
      .join(rev);
    layout_ok(m, &dir).then_some(dir)
  }

  pub fn resolve(&self, m: &CatalogModel, linked: Option<&Path>) -> ModelState {
    if let Some(p) = linked {
      return if layout_ok(m, p) {
        ModelState::Linked(p.to_path_buf())
      } else {
        ModelState::Missing(p.to_path_buf())
      };
    }
    if let Some(d) = self.roots().map(|r| r.join(m.id)).find(|d| layout_ok(m, d)) {
      return ModelState::Installed(d);
    }
    match self.cached(m) {
      Some(c) => ModelState::Cached(c),
      None => ModelState::NotInstalled,
    }
  }

  pub fn remove_install(&self, id: &str) -> std::io::Result<()> {
    let dir = self
      .roots()
      .map(|r| r.join(id))
      .find(|d| d.exists())
      .unwrap_or_else(|| self.install_dir(id));
    if !self.contains(&dir) {
      return Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "outside the model store",
      ));
    }
    std::fs::remove_dir_all(dir)
  }

  /// Removes `.part` folders an interrupted download left behind. Returns how many. Only
  /// catalog ids count, since a folder the user picked may hold other things.
  pub fn cleanup_parts(&self) -> usize {
    self
      .roots()
      .filter_map(|r| std::fs::read_dir(r).ok())
      .flat_map(|entries| entries.flatten().map(|e| e.path()))
      .filter(|p| {
        p.extension().is_some_and(|x| x == "part")
          && p
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|id| super::catalog::get(id).is_some())
          && self.contains(p)
      })
      .filter(|p| std::fs::remove_dir_all(p).is_ok())
      .count()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::local_asr::catalog::{get, Source};

  fn fill(dir: &Path, m: &CatalogModel) {
    std::fs::create_dir_all(dir).unwrap();
    for f in m.files {
      if f.size == 0 && !f.name.contains('.') {
        std::fs::create_dir_all(dir.join(f.name)).unwrap();
      } else {
        let file = std::fs::File::create(dir.join(f.name)).unwrap();
        file.set_len(f.size).unwrap();
      }
    }
  }

  #[test]
  fn resolve_prefers_linked_then_installed_then_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let hub = tmp.path().join("hub");
    let models = ModelsDir::new(tmp.path().join("stt"), Some(hub.clone()));
    let m = get("whisper-tiny").unwrap();
    assert_eq!(models.resolve(m, None), ModelState::NotInstalled);

    let Source::HfFiles { rev, .. } = m.source else {
      panic!("whisper-tiny comes from Hugging Face");
    };
    let cached = hub
      .join("models--csukuangfj--sherpa-onnx-whisper-tiny")
      .join("snapshots")
      .join(rev);
    fill(&cached, m);
    assert_eq!(models.resolve(m, None), ModelState::Cached(cached.clone()));

    fill(&models.install_dir(m.id), m);
    assert_eq!(models.resolve(m, None), ModelState::Installed(models.install_dir(m.id)));

    let linked = tmp.path().join("mine");
    assert_eq!(models.resolve(m, Some(&linked)), ModelState::Missing(linked.clone()));
    fill(&linked, m);
    assert_eq!(models.resolve(m, Some(&linked)), ModelState::Linked(linked));
  }

  #[test]
  fn layout_checks_sizes_and_archive_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let m = get("parakeet-v2").unwrap();
    fill(tmp.path(), m);
    assert!(layout_ok(m, tmp.path()));
    std::fs::File::create(tmp.path().join("tokens.txt"))
      .unwrap()
      .set_len(5)
      .unwrap();
    assert!(!layout_ok(m, tmp.path()));
    let q = get("qwen3-asr").unwrap();
    let qd = tmp.path().join("q");
    fill(&qd, q);
    assert!(layout_ok(q, &qd));
  }

  #[test]
  fn removal_and_cleanup_stay_inside_the_store() {
    let tmp = tempfile::tempdir().unwrap();
    let models = ModelsDir::new(tmp.path().join("stt"), None);
    let m = get("whisper-tiny").unwrap();
    fill(&models.install_dir(m.id), m);
    std::fs::create_dir_all(models.part_dir("parakeet-v3")).unwrap();
    std::fs::create_dir_all(models.part_dir("not-a-model")).unwrap();
    assert_eq!(models.cleanup_parts(), 1);
    assert!(
      models.part_dir("not-a-model").exists(),
      "only catalog downloads are cleaned up"
    );
    models.remove_install(m.id).unwrap();
    assert!(!models.install_dir(m.id).exists());
    assert!(models.remove_install("../escape").is_err());
  }

  #[test]
  fn picked_folder_takes_downloads_and_keeps_default_installs() {
    let tmp = tempfile::tempdir().unwrap();
    let default = ModelsDir::new(tmp.path().join("stt"), None);
    let tiny = get("whisper-tiny").unwrap();
    fill(&default.install_dir(tiny.id), tiny);
    let picked = tmp.path().join("D-models");
    let models = default.select(Some(&picked));
    assert_eq!(models.root(), picked);
    assert_eq!(models.install_dir("parakeet-v3"), picked.join("parakeet-v3"));
    assert_eq!(
      models.resolve(tiny, None),
      ModelState::Installed(default.install_dir(tiny.id))
    );
    let base = get("whisper-base").unwrap();
    fill(&models.install_dir(base.id), base);
    assert_eq!(models.resolve(base, None), ModelState::Installed(picked.join(base.id)));
    models.remove_install(tiny.id).unwrap();
    assert!(!default.install_dir(tiny.id).exists());
    assert_eq!(default.select(Some(default.root())).root(), default.root());
    assert_eq!(default.select(None).root(), default.root());
  }

  #[test]
  fn hf_hub_location_follows_env() {
    let env = |k: &str| match k {
      "HF_HOME" => Some(r"D:\hf".to_string()),
      "USERPROFILE" => Some(r"C:\Users\u".to_string()),
      _ => None,
    };
    assert_eq!(default_hf_hub(env), Some(PathBuf::from(r"D:\hf\hub")));
    let env = |k: &str| (k == "USERPROFILE").then(|| r"C:\Users\u".to_string());
    assert_eq!(
      default_hf_hub(env),
      Some(PathBuf::from(r"C:\Users\u\.cache\huggingface\hub"))
    );
  }
}
