//! Model downloads: HTTP range resume, SHA-256 checks, `.part` staging, atomic install.
//! Nothing outside the model store is touched, and a model appears only once fully verified.

use std::path::Path;
use std::time::Duration;

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use super::catalog::{CatalogModel, Source};
use super::store::{layout_ok, ModelsDir};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
  NoSpace {
    needed: u64,
  },
  Network,
  Http(u16),
  /// File name whose contents did not match its pinned hash (after one retry).
  Hash(String),
  Cancelled,
  Io(String),
}

impl DownloadError {
  /// One sentence for the UI.
  pub fn message(&self) -> String {
    match self {
      DownloadError::NoSpace { needed } => format!("Needs {:.1} GB free", *needed as f64 / 1e9),
      DownloadError::Network => "Download failed. Check your connection and try again.".into(),
      DownloadError::Http(code) => format!("Download failed (HTTP {code})"),
      DownloadError::Hash(file) => format!("{file} came through damaged. Try again."),
      DownloadError::Cancelled => "Cancelled".into(),
      DownloadError::Io(_) => "Could not save the model files".into(),
    }
  }
}

fn io(e: impl std::fmt::Display) -> DownloadError {
  DownloadError::Io(e.to_string())
}

#[derive(Debug, Clone)]
pub struct Hosts {
  pub hf: String,
  pub github: String,
}

impl Default for Hosts {
  fn default() -> Self {
    Self {
      hf: "https://huggingface.co".into(),
      github: "https://github.com".into(),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
  pub done: u64,
  pub total: u64,
}

/// Needs the download plus 10% headroom. Unknown free space does not block.
pub fn check_space(needed: u64, available: Option<u64>) -> Result<(), DownloadError> {
  match available {
    Some(a) if a < needed + needed / 10 => Err(DownloadError::NoSpace { needed }),
    _ => Ok(()),
  }
}

#[cfg(windows)]
fn free_bytes(path: &Path) -> Option<u64> {
  use windows::core::HSTRING;
  use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
  let mut avail = 0u64;
  unsafe { GetDiskFreeSpaceExW(&HSTRING::from(path.as_os_str()), Some(&mut avail), None, None) }.ok()?;
  Some(avail)
}

#[cfg(not(windows))]
fn free_bytes(_path: &Path) -> Option<u64> {
  None
}

async fn sha256_file(path: &Path) -> Result<String, DownloadError> {
  let path = path.to_path_buf();
  tokio::task::spawn_blocking(move || {
    use std::io::Read;
    let mut f = std::fs::File::open(&path).map_err(io)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
      let n = f.read(&mut buf).map_err(io)?;
      if n == 0 {
        break;
      }
      h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
  })
  .await
  .map_err(io)?
}

/// One attempt: resumes from the bytes already on disk when the server honors Range.
async fn fetch_once(
  client: &reqwest::Client,
  url: &str,
  dest: &Path,
  size: u64,
  cancel: &CancellationToken,
  on: &mut (dyn FnMut(u64) + Send),
) -> Result<(), DownloadError> {
  let mut have = tokio::fs::metadata(dest).await.map(|m| m.len()).unwrap_or(0);
  if size > 0 && have == size {
    on(have);
    return Ok(());
  }
  if size > 0 && have > size {
    let _ = tokio::fs::remove_file(dest).await;
    have = 0;
  }
  let mut req = client.get(url);
  if have > 0 {
    req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
  }
  let resp = tokio::select! {
    _ = cancel.cancelled() => return Err(DownloadError::Cancelled),
    r = req.send() => r.map_err(|_| DownloadError::Network)?,
  };
  let status = resp.status();
  if !status.is_success() {
    return Err(if status.is_server_error() {
      DownloadError::Network
    } else {
      DownloadError::Http(status.as_u16())
    });
  }
  let append = status == reqwest::StatusCode::PARTIAL_CONTENT;
  let mut opts = tokio::fs::OpenOptions::new();
  if append {
    opts.append(true);
  } else {
    opts.create(true).write(true).truncate(true);
  }
  let mut file = opts.open(dest).await.map_err(io)?;
  let mut written = if append { have } else { 0 };
  on(written);
  let mut stream = resp.bytes_stream();
  loop {
    let chunk = tokio::select! {
      _ = cancel.cancelled() => return Err(DownloadError::Cancelled),
      c = stream.next() => c,
    };
    let Some(chunk) = chunk else { break };
    let chunk = chunk.map_err(|_| DownloadError::Network)?;
    file.write_all(&chunk).await.map_err(io)?;
    written += chunk.len() as u64;
    on(written);
  }
  file.flush().await.map_err(io)?;
  if size > 0 && written != size {
    // Short body: the next attempt resumes from here.
    return Err(DownloadError::Network);
  }
  Ok(())
}

/// Up to 3 network retries with backoff, then the hash check; a bad hash re-downloads once.
async fn fetch(
  client: &reqwest::Client,
  url: &str,
  dest: &Path,
  size: u64,
  sha256: &str,
  cancel: &CancellationToken,
  on: &mut (dyn FnMut(u64) + Send),
) -> Result<(), DownloadError> {
  for hash_attempt in 0..2 {
    let mut attempt = 0u32;
    loop {
      match fetch_once(client, url, dest, size, cancel, on).await {
        Ok(()) => break,
        Err(DownloadError::Network) if attempt < 3 => {
          attempt += 1;
          tokio::select! {
            _ = cancel.cancelled() => return Err(DownloadError::Cancelled),
            _ = tokio::time::sleep(Duration::from_millis(500 * (1 << attempt))) => {}
          }
        }
        Err(e) => return Err(e),
      }
    }
    if sha256.is_empty() || sha256_file(dest).await? == sha256 {
      return Ok(());
    }
    let _ = tokio::fs::remove_file(dest).await;
    tracing::warn!(attempt = hash_attempt, "model file hash mismatch");
  }
  let name = dest
    .file_name()
    .map(|n| n.to_string_lossy().into_owned())
    .unwrap_or_default();
  Err(DownloadError::Hash(name))
}

/// Renames with retries: antivirus scanners briefly lock freshly written files.
async fn rename_retry(from: &Path, to: &Path) -> Result<(), DownloadError> {
  let mut last = None;
  for i in 0..6u32 {
    match tokio::fs::rename(from, to).await {
      Ok(()) => return Ok(()),
      Err(e) => last = Some(e),
    }
    tokio::time::sleep(Duration::from_millis(200 * (1 << i))).await;
  }
  Err(io(last.map(|e| e.to_string()).unwrap_or_default()))
}

fn extract_tar_bz2(archive: &Path, dest: &Path) -> Result<(), DownloadError> {
  let f = std::fs::File::open(archive).map_err(io)?;
  let mut ar = tar::Archive::new(bzip2::read::BzDecoder::new(std::io::BufReader::new(f)));
  std::fs::create_dir_all(dest).map_err(io)?;
  // `unpack` skips entries that would land outside `dest` (absolute paths, `..`).
  ar.unpack(dest).map_err(io)
}

pub async fn download_model(
  m: &CatalogModel,
  dir: &ModelsDir,
  hosts: &Hosts,
  cancel: CancellationToken,
  mut on: impl FnMut(Progress) + Send,
) -> Result<(), DownloadError> {
  if cancel.is_cancelled() {
    return Err(DownloadError::Cancelled);
  }
  tokio::fs::create_dir_all(dir.root()).await.map_err(io)?;
  let archive = matches!(m.source, Source::Archive { .. });
  let total = m.download_bytes();
  // An archive needs room for itself and its unpacked copy.
  check_space(if archive { total * 2 } else { total }, free_bytes(dir.root()))?;
  let client = reqwest::Client::builder()
    .connect_timeout(Duration::from_secs(15))
    .read_timeout(Duration::from_secs(60))
    .build()
    .map_err(|_| DownloadError::Network)?;
  let part = dir.part_dir(m.id);
  tokio::fs::create_dir_all(&part).await.map_err(io)?;
  let install = dir.install_dir(m.id);
  match m.source {
    Source::HfFiles { repo, rev } => {
      let mut base = 0u64;
      for f in m.files {
        let url = format!("{}/{repo}/resolve/{rev}/{}", hosts.hf, f.name);
        let mut report = |n: u64| on(Progress { done: base + n, total });
        fetch(
          &client,
          &url,
          &part.join(f.name),
          f.size,
          f.sha256,
          &cancel,
          &mut report,
        )
        .await?;
        base += f.size;
      }
      if !layout_ok(m, &part) {
        return Err(io("downloaded files are incomplete"));
      }
      if install.exists() && dir.contains(&install) {
        tokio::fs::remove_dir_all(&install).await.map_err(io)?;
      }
      rename_retry(&part, &install).await
    }
    Source::Archive {
      path,
      size,
      sha256,
      root,
    } => {
      let file = part.join("model.tar.bz2");
      let url = format!("{}/{path}", hosts.github);
      let mut report = |n: u64| on(Progress { done: n, total });
      fetch(&client, &url, &file, size, sha256, &cancel, &mut report).await?;
      let extract = part.join("x");
      let (a, e) = (file.clone(), extract.clone());
      tokio::task::spawn_blocking(move || extract_tar_bz2(&a, &e))
        .await
        .map_err(io)??;
      let unpacked = extract.join(root);
      if !layout_ok(m, &unpacked) {
        return Err(io("archive does not contain the expected files"));
      }
      if install.exists() && dir.contains(&install) {
        tokio::fs::remove_dir_all(&install).await.map_err(io)?;
      }
      rename_retry(&unpacked, &install).await?;
      let _ = tokio::fs::remove_dir_all(&part).await;
      Ok(())
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::local_asr::catalog::{CatalogModel, ModelFile, ModelKind, Source};
  use std::sync::{Arc, Mutex};
  use tokio::io::{AsyncReadExt, AsyncWriteExt};

  type Seen = Arc<Mutex<Vec<String>>>;

  /// Minimal HTTP file server on loopback with Range support. With `cut`, the first response for
  /// each path stops after that many body bytes (a dropped connection).
  async fn serve(files: Vec<(String, Vec<u8>)>, cut: Option<usize>) -> (String, Seen) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen: Seen = Arc::default();
    let log = seen.clone();
    let cut_done: Arc<Mutex<std::collections::HashSet<String>>> = Arc::default();
    tokio::spawn(async move {
      while let Ok((mut sock, _)) = listener.accept().await {
        let files = files.clone();
        let log = log.clone();
        let cut_done = cut_done.clone();
        tokio::spawn(async move {
          let mut buf = vec![0u8; 8192];
          let n = sock.read(&mut buf).await.unwrap_or(0);
          let req = String::from_utf8_lossy(&buf[..n]).to_string();
          let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
          let start: usize = req
            .lines()
            .find_map(|l| {
              l.to_ascii_lowercase()
                .strip_prefix("range: bytes=")
                .map(|r| r.trim_end_matches('-').parse().unwrap_or(0))
            })
            .unwrap_or(0);
          log.lock().unwrap().push(format!("{path} @{start}"));
          let Some((_, body)) = files.iter().find(|(p, _)| *p == path) else {
            let _ = sock
              .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
              .await;
            return;
          };
          let part = &body[start..];
          let status = if start > 0 { "206 Partial Content" } else { "200 OK" };
          let head = format!(
            "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            part.len()
          );
          let _ = sock.write_all(head.as_bytes()).await;
          let first = cut_done.lock().unwrap().insert(path.clone());
          let send = match cut {
            Some(c) if first && start == 0 => &part[..c.min(part.len())],
            _ => part,
          };
          let _ = sock.write_all(send).await;
        });
      }
    });
    (format!("http://{addr}"), seen)
  }

  fn sha(bytes: &[u8]) -> &'static str {
    Box::leak(format!("{:x}", Sha256::digest(bytes)).into_boxed_str())
  }

  fn hf_model(files: Vec<(&'static str, &[u8])>, bad_hash: bool) -> CatalogModel {
    let files: Vec<ModelFile> = files
      .into_iter()
      .map(|(name, b)| ModelFile {
        role: name,
        name,
        size: b.len() as u64,
        sha256: if bad_hash { sha(b"nope") } else { sha(b) },
      })
      .collect();
    CatalogModel {
      id: "fake",
      name: "Fake",
      kind: ModelKind::Dolphin,
      languages: &["en"],
      source: Source::HfFiles {
        repo: "org/fake",
        rev: "0000000000000000000000000000000000000000",
      },
      files: Box::leak(files.into_boxed_slice()),
      license: "MIT",
    }
  }

  fn hosts(base: &str) -> Hosts {
    Hosts {
      hf: base.into(),
      github: base.into(),
    }
  }

  const MODEL: &[u8] = b"model-bytes-0123456789";
  const TOKENS: &[u8] = b"a 0\nb 1\n";

  fn files() -> Vec<(String, Vec<u8>)> {
    vec![
      (
        "/org/fake/resolve/0000000000000000000000000000000000000000/model.int8.onnx".into(),
        MODEL.to_vec(),
      ),
      (
        "/org/fake/resolve/0000000000000000000000000000000000000000/tokens.txt".into(),
        TOKENS.to_vec(),
      ),
    ]
  }

  #[tokio::test]
  async fn downloads_verifies_and_installs() {
    let (base, _) = serve(files(), None).await;
    let tmp = tempfile::tempdir().unwrap();
    let dir = ModelsDir::new(tmp.path().to_path_buf(), None);
    let m = hf_model(vec![("model.int8.onnx", MODEL), ("tokens.txt", TOKENS)], false);
    let mut last = Progress { done: 0, total: 0 };
    download_model(&m, &dir, &hosts(&base), CancellationToken::new(), |p| last = p)
      .await
      .unwrap();
    assert_eq!(
      std::fs::read(dir.install_dir("fake").join("tokens.txt")).unwrap(),
      TOKENS
    );
    assert!(!dir.part_dir("fake").exists());
    assert_eq!(last.done, last.total);
  }

  #[tokio::test]
  async fn resumes_after_a_dropped_connection() {
    let (base, seen) = serve(files(), Some(5)).await;
    let tmp = tempfile::tempdir().unwrap();
    let dir = ModelsDir::new(tmp.path().to_path_buf(), None);
    let m = hf_model(vec![("model.int8.onnx", MODEL), ("tokens.txt", TOKENS)], false);
    download_model(&m, &dir, &hosts(&base), CancellationToken::new(), |_| {})
      .await
      .unwrap();
    assert_eq!(
      std::fs::read(dir.install_dir("fake").join("model.int8.onnx")).unwrap(),
      MODEL
    );
    let log = seen.lock().unwrap().clone();
    assert!(log.iter().any(|s| s.ends_with("model.int8.onnx @5")), "{log:?}");
  }

  #[tokio::test]
  async fn hash_mismatch_fails_without_installing() {
    let (base, _) = serve(files(), None).await;
    let tmp = tempfile::tempdir().unwrap();
    let dir = ModelsDir::new(tmp.path().to_path_buf(), None);
    let m = hf_model(vec![("model.int8.onnx", MODEL)], true);
    let r = download_model(&m, &dir, &hosts(&base), CancellationToken::new(), |_| {}).await;
    assert_eq!(r, Err(DownloadError::Hash("model.int8.onnx".into())));
    assert!(!dir.install_dir("fake").exists());
    assert!(!dir.part_dir("fake").join("model.int8.onnx").exists());
  }

  #[tokio::test]
  async fn cancel_stops_and_installs_nothing() {
    let (base, _) = serve(files(), None).await;
    let tmp = tempfile::tempdir().unwrap();
    let dir = ModelsDir::new(tmp.path().to_path_buf(), None);
    let m = hf_model(vec![("model.int8.onnx", MODEL)], false);
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
      download_model(&m, &dir, &hosts(&base), cancel, |_| {}).await,
      Err(DownloadError::Cancelled)
    );
    assert!(!dir.install_dir("fake").exists());
  }

  #[tokio::test]
  async fn archive_is_verified_unpacked_and_installed() {
    let mut tar_bytes = Vec::new();
    {
      let enc = bzip2::write::BzEncoder::new(&mut tar_bytes, bzip2::Compression::fast());
      let mut b = tar::Builder::new(enc);
      for (name, data) in [("root/model.int8.onnx", MODEL), ("root/tokens.txt", TOKENS)] {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        b.append_data(&mut h, name, data).unwrap();
      }
      b.into_inner().unwrap().finish().unwrap();
    }
    let (base, _) = serve(vec![("/a/b.tar.bz2".into(), tar_bytes.clone())], None).await;
    let tmp = tempfile::tempdir().unwrap();
    let dir = ModelsDir::new(tmp.path().to_path_buf(), None);
    let m = CatalogModel {
      id: "arch",
      name: "Arch",
      kind: ModelKind::Dolphin,
      languages: &["en"],
      source: Source::Archive {
        path: "a/b.tar.bz2",
        size: tar_bytes.len() as u64,
        sha256: sha(&tar_bytes),
        root: "root",
      },
      files: &[
        ModelFile {
          role: "model",
          name: "model.int8.onnx",
          size: 0,
          sha256: "",
        },
        ModelFile {
          role: "tokens",
          name: "tokens.txt",
          size: 0,
          sha256: "",
        },
      ],
      license: "MIT",
    };
    download_model(&m, &dir, &hosts(&base), CancellationToken::new(), |_| {})
      .await
      .unwrap();
    assert_eq!(
      std::fs::read(dir.install_dir("arch").join("model.int8.onnx")).unwrap(),
      MODEL
    );
    assert!(!dir.part_dir("arch").exists());
  }

  #[test]
  fn space_check_keeps_headroom() {
    assert!(check_space(1_000, None).is_ok());
    assert!(check_space(1_000, Some(1_200)).is_ok());
    assert_eq!(
      check_space(1_000, Some(1_050)),
      Err(DownloadError::NoSpace { needed: 1_000 })
    );
  }
}
