//! Install detection and endpoint resolution for Ollama and LM Studio.

use std::path::{Path, PathBuf};

use super::LocalApp;

/// Environment and file-system lookups, injectable for tests. `Sync` because a probe is held
/// across awaits in Send futures (commands, spawned warm-ups).
pub trait Probe: Send + Sync {
  fn env(&self, key: &str) -> Option<String>;
  fn exists(&self, path: &Path) -> bool;
  fn read(&self, path: &Path) -> Option<String>;
}

/// The real environment. Falls back to the user's saved environment (HKCU\Environment) so a
/// variable set after the app started, like a changed `OLLAMA_MODELS`, is still seen.
pub struct SystemProbe;

impl Probe for SystemProbe {
  fn env(&self, key: &str) -> Option<String> {
    std::env::var(key)
      .ok()
      .filter(|v| !v.trim().is_empty())
      .or_else(|| user_env(key))
  }
  fn exists(&self, path: &Path) -> bool {
    path.exists()
  }
  fn read(&self, path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
  }
}

#[cfg(windows)]
fn user_env(key: &str) -> Option<String> {
  use windows::core::{w, HSTRING};
  use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ};
  let mut buf = vec![0u16; 2048];
  let mut size = (buf.len() * 2) as u32;
  let r = unsafe {
    RegGetValueW(
      HKEY_CURRENT_USER,
      w!("Environment"),
      &HSTRING::from(key),
      RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
      None,
      Some(buf.as_mut_ptr().cast()),
      Some(&mut size),
    )
  };
  if r.is_err() {
    return None;
  }
  let len = (size as usize / 2).saturating_sub(1);
  let v = String::from_utf16_lossy(&buf[..len]);
  (!v.trim().is_empty()).then_some(v)
}

#[cfg(not(windows))]
fn user_env(_key: &str) -> Option<String> {
  None
}

fn path_dirs(p: &dyn Probe) -> Vec<PathBuf> {
  p.env("PATH")
    .map(|v| {
      v.split(';')
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .collect()
    })
    .unwrap_or_default()
}

fn home(p: &dyn Probe) -> Option<PathBuf> {
  p.env("USERPROFILE").map(PathBuf::from)
}

/// The command-line tool that can start the server: `ollama.exe` or `lms.exe`.
pub fn find_cli(app: LocalApp, p: &dyn Probe) -> Option<PathBuf> {
  let mut candidates = Vec::new();
  match app {
    LocalApp::Ollama => {
      if let Some(local) = p.env("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join(r"Programs\Ollama\ollama.exe"));
      }
      candidates.extend(path_dirs(p).into_iter().map(|d| d.join("ollama.exe")));
    }
    LocalApp::LmStudio => {
      if let Some(h) = home(p) {
        candidates.push(h.join(r".lmstudio\bin\lms.exe"));
      }
      candidates.extend(path_dirs(p).into_iter().map(|d| d.join("lms.exe")));
    }
  }
  candidates.into_iter().find(|c| p.exists(c))
}

/// LM Studio records where its app lives; used when the CLI has not been set up yet.
fn lmstudio_gui(p: &dyn Probe) -> Option<PathBuf> {
  let h = home(p)?;
  let recorded = p
    .read(&h.join(r".lmstudio\.internal\app-install-location.json"))
    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
    .and_then(|v| v["path"].as_str().map(PathBuf::from));
  let default = p
    .env("LOCALAPPDATA")
    .map(|l| PathBuf::from(l).join(r"Programs\LM Studio\LM Studio.exe"));
  [recorded, default].into_iter().flatten().find(|c| p.exists(c))
}

pub fn installed(app: LocalApp, p: &dyn Probe) -> bool {
  find_cli(app, p).is_some() || (app == LocalApp::LmStudio && lmstudio_gui(p).is_some())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
  /// Base URL without a trailing slash, for example `http://127.0.0.1:11434`.
  pub base: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointError {
  /// Configured to a host other than this computer; never called.
  NotLocal,
}

/// Parses an `OLLAMA_HOST`-style value. Bind-all addresses map to loopback; anything that is not
/// this computer is refused.
pub fn parse_host(raw: &str, default_port: u16) -> Result<Endpoint, EndpointError> {
  let raw = raw.trim();
  let raw = if raw.starts_with(':') {
    format!("127.0.0.1{raw}")
  } else {
    raw.to_string()
  };
  let with_scheme = if raw.contains("://") {
    raw
  } else {
    format!("http://{raw}")
  };
  let url = url::Url::parse(&with_scheme).map_err(|_| EndpointError::NotLocal)?;
  if url.scheme() != "http" && url.scheme() != "https" {
    return Err(EndpointError::NotLocal);
  }
  let host = match url.host() {
    None => "127.0.0.1".to_string(),
    Some(url::Host::Domain(d)) if d.eq_ignore_ascii_case("localhost") => "127.0.0.1".into(),
    Some(url::Host::Ipv4(ip)) if ip.is_loopback() || ip.is_unspecified() => "127.0.0.1".into(),
    Some(url::Host::Ipv6(ip)) if ip.is_loopback() || ip.is_unspecified() => "[::1]".into(),
    _ => return Err(EndpointError::NotLocal),
  };
  let port = url.port().unwrap_or(default_port);
  Ok(Endpoint {
    base: format!("http://{host}:{port}"),
  })
}

pub fn endpoint(app: LocalApp, p: &dyn Probe) -> Result<Endpoint, EndpointError> {
  match app {
    LocalApp::Ollama => match p.env("OLLAMA_HOST") {
      Some(h) => parse_host(&h, app.default_port()),
      None => parse_host("127.0.0.1", app.default_port()),
    },
    LocalApp::LmStudio => {
      let port = home(p)
        .and_then(|h| p.read(&h.join(r".lmstudio\.internal\http-server-config.json")))
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["port"].as_u64())
        .and_then(|n| u16::try_from(n).ok())
        .unwrap_or(app.default_port());
      parse_host(&format!("127.0.0.1:{port}"), port)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::collections::{HashMap, HashSet};

  #[derive(Default)]
  struct FakeProbe {
    env: HashMap<String, String>,
    files: HashSet<PathBuf>,
    contents: HashMap<PathBuf, String>,
  }

  impl Probe for FakeProbe {
    fn env(&self, key: &str) -> Option<String> {
      self.env.get(key).cloned()
    }
    fn exists(&self, path: &Path) -> bool {
      self.files.contains(path) || self.contents.contains_key(path)
    }
    fn read(&self, path: &Path) -> Option<String> {
      self.contents.get(path).cloned()
    }
  }

  fn probe() -> FakeProbe {
    let mut p = FakeProbe::default();
    p.env.insert("LOCALAPPDATA".into(), r"C:\Users\u\AppData\Local".into());
    p.env.insert("USERPROFILE".into(), r"C:\Users\u".into());
    p
  }

  #[test]
  fn loopback_hosts_are_accepted_and_normalized() {
    let ok = |raw: &str| parse_host(raw, 11434).unwrap().base;
    assert_eq!(ok("127.0.0.1:11434"), "http://127.0.0.1:11434");
    assert_eq!(ok("0.0.0.0"), "http://127.0.0.1:11434");
    assert_eq!(ok("http://localhost:9999"), "http://127.0.0.1:9999");
    assert_eq!(ok(":5000"), "http://127.0.0.1:5000");
    assert_eq!(ok("[::1]:8080"), "http://[::1]:8080");
  }

  #[test]
  fn remote_hosts_are_refused() {
    assert_eq!(parse_host("192.168.1.20:11434", 11434), Err(EndpointError::NotLocal));
    assert_eq!(
      parse_host("http://my-server:11434", 11434),
      Err(EndpointError::NotLocal)
    );
    assert_eq!(parse_host("ftp://127.0.0.1", 11434), Err(EndpointError::NotLocal));
  }

  #[test]
  fn ollama_endpoint_uses_env_or_default() {
    let mut p = probe();
    assert_eq!(endpoint(LocalApp::Ollama, &p).unwrap().base, "http://127.0.0.1:11434");
    p.env.insert("OLLAMA_HOST".into(), "127.0.0.1:12000".into());
    assert_eq!(endpoint(LocalApp::Ollama, &p).unwrap().base, "http://127.0.0.1:12000");
    p.env.insert("OLLAMA_HOST".into(), "10.0.0.5".into());
    assert_eq!(endpoint(LocalApp::Ollama, &p), Err(EndpointError::NotLocal));
  }

  #[test]
  fn lmstudio_endpoint_reads_server_config_port() {
    let mut p = probe();
    assert_eq!(endpoint(LocalApp::LmStudio, &p).unwrap().base, "http://127.0.0.1:1234");
    p.contents.insert(
      PathBuf::from(r"C:\Users\u\.lmstudio\.internal\http-server-config.json"),
      r#"{"port": 4321, "networkInterface": "127.0.0.1"}"#.into(),
    );
    assert_eq!(endpoint(LocalApp::LmStudio, &p).unwrap().base, "http://127.0.0.1:4321");
  }

  #[test]
  fn finds_cli_in_known_folders_then_path() {
    let mut p = probe();
    assert_eq!(find_cli(LocalApp::Ollama, &p), None);
    assert!(!installed(LocalApp::Ollama, &p));
    p.env.insert("PATH".into(), r"C:\tools;C:\bin".into());
    p.files.insert(PathBuf::from(r"C:\bin\ollama.exe"));
    assert_eq!(
      find_cli(LocalApp::Ollama, &p),
      Some(PathBuf::from(r"C:\bin\ollama.exe"))
    );
    p.files
      .insert(PathBuf::from(r"C:\Users\u\AppData\Local\Programs\Ollama\ollama.exe"));
    assert_eq!(
      find_cli(LocalApp::Ollama, &p),
      Some(PathBuf::from(r"C:\Users\u\AppData\Local\Programs\Ollama\ollama.exe"))
    );
    p.files.insert(PathBuf::from(r"C:\Users\u\.lmstudio\bin\lms.exe"));
    assert!(installed(LocalApp::LmStudio, &p));
  }

  #[test]
  fn lmstudio_gui_counts_as_installed_without_cli() {
    let mut p = probe();
    let gui = r"D:\Apps\LM Studio\LM Studio.exe";
    p.contents.insert(
      PathBuf::from(r"C:\Users\u\.lmstudio\.internal\app-install-location.json"),
      format!(r#"{{"path":{}}}"#, serde_json::to_string(gui).unwrap()),
    );
    p.files.insert(PathBuf::from(gui));
    assert!(installed(LocalApp::LmStudio, &p));
    assert_eq!(find_cli(LocalApp::LmStudio, &p), None);
  }
}
