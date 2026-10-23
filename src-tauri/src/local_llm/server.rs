//! HTTP client for local servers: up check, model list, start, warm-up, thinking lookup, pull.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::detect::{self, EndpointError, Probe};
use super::{is_text_model_name, LocalApp, LocalModel};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerError {
  Unreachable,
  Http(u16),
  Malformed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartError {
  NotInstalled,
  NotLocal,
  NoCli,
  Spawn,
  Timeout,
}

/// How long a start may take before it counts as failed. LM Studio's headless daemon needs
/// about 6 s on an idle machine and much longer while the PC is busy starting other apps.
pub fn start_limit(app: LocalApp) -> Duration {
  match app {
    LocalApp::Ollama => Duration::from_secs(20),
    LocalApp::LmStudio => Duration::from_secs(45),
  }
}

fn client(timeout: Duration) -> reqwest::Client {
  reqwest::Client::builder().timeout(timeout).build().unwrap_or_default()
}

async fn get_text(url: &str, timeout: Duration) -> Result<String, ServerError> {
  let resp = client(timeout)
    .get(url)
    .send()
    .await
    .map_err(|_| ServerError::Unreachable)?;
  let status = resp.status();
  if !status.is_success() {
    return Err(ServerError::Http(status.as_u16()));
  }
  resp.text().await.map_err(|_| ServerError::Unreachable)
}

pub async fn is_up(app: LocalApp, base: &str) -> bool {
  let path = match app {
    LocalApp::Ollama => "/api/version",
    LocalApp::LmStudio => "/api/v1/models",
  };
  get_text(&format!("{base}{path}"), Duration::from_secs(2)).await.is_ok()
}

pub fn parse_ollama_tags(body: &str) -> Result<Vec<(String, u64)>, ServerError> {
  let v: Value = serde_json::from_str(body).map_err(|_| ServerError::Malformed)?;
  let models = v["models"].as_array().ok_or(ServerError::Malformed)?;
  Ok(
    models
      .iter()
      .filter_map(|m| Some((m["name"].as_str()?.to_string(), m["size"].as_u64().unwrap_or(0))))
      .collect(),
  )
}

pub fn parse_ollama_caps(body: &str) -> Vec<String> {
  serde_json::from_str::<Value>(body)
    .ok()
    .and_then(|v| v["capabilities"].as_array().cloned())
    .map(|a| a.iter().filter_map(|c| c.as_str().map(str::to_string)).collect())
    .unwrap_or_default()
}

pub fn parse_lmstudio_models(body: &str) -> Result<Vec<LocalModel>, ServerError> {
  let v: Value = serde_json::from_str(body).map_err(|_| ServerError::Malformed)?;
  let models = v["models"].as_array().ok_or(ServerError::Malformed)?;
  Ok(
    models
      .iter()
      .filter(|m| m["type"].as_str() == Some("llm"))
      .filter_map(|m| {
        let id = m["key"].as_str()?.to_string();
        Some(LocalModel {
          name: m["display_name"].as_str().unwrap_or(&id).to_string(),
          size_bytes: m["size_bytes"].as_u64().unwrap_or(0),
          usable: true,
          thinking: !m["capabilities"]["reasoning"].is_null(),
          id,
        })
      })
      .collect(),
  )
}

/// Whether each model reasons before answering, keyed by `app:model`.
fn thinking_cache() -> &'static Mutex<HashMap<String, bool>> {
  static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
  CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remember_thinking(app: LocalApp, model: &str, thinking: bool) {
  thinking_cache()
    .lock()
    .unwrap()
    .insert(format!("{}:{model}", app.id()), thinking);
}

fn cached_thinking(app: LocalApp, model: &str) -> Option<bool> {
  thinking_cache()
    .lock()
    .unwrap()
    .get(&format!("{}:{model}", app.id()))
    .copied()
}

async fn ollama_caps(base: &str, model: &str) -> Option<Vec<String>> {
  let resp = client(Duration::from_secs(5))
    .post(format!("{base}/api/show"))
    .json(&json!({ "model": model }))
    .send()
    .await
    .ok()?;
  if !resp.status().is_success() {
    return None;
  }
  Some(parse_ollama_caps(&resp.text().await.ok()?))
}

pub async fn list_models(app: LocalApp, base: &str) -> Result<Vec<LocalModel>, ServerError> {
  match app {
    LocalApp::Ollama => {
      let tags = parse_ollama_tags(&get_text(&format!("{base}/api/tags"), Duration::from_secs(5)).await?)?;
      let mut out = Vec::new();
      for (name, size_bytes) in tags {
        let caps = ollama_caps(base, &name).await;
        let text = match &caps {
          Some(c) if !c.is_empty() => c.iter().any(|c| c == "completion"),
          _ => is_text_model_name(&name),
        };
        if !text {
          continue;
        }
        let thinking = caps.is_some_and(|c| c.iter().any(|c| c == "thinking"));
        remember_thinking(app, &name, thinking);
        out.push(LocalModel {
          id: name.clone(),
          name,
          size_bytes,
          usable: true,
          thinking,
        });
      }
      Ok(out)
    }
    LocalApp::LmStudio => {
      let models = parse_lmstudio_models(&get_text(&format!("{base}/api/v1/models"), Duration::from_secs(5)).await?)?;
      for m in &models {
        remember_thinking(app, &m.id, m.thinking);
      }
      Ok(models)
    }
  }
}

/// Whether a model reasons by default (so refinement turns that off); looked up once per model.
pub async fn thinking(app: LocalApp, base: &str, model: &str) -> bool {
  if let Some(t) = cached_thinking(app, model) {
    return t;
  }
  let thinking = match app {
    LocalApp::Ollama => ollama_caps(base, model)
      .await
      .is_some_and(|c| c.iter().any(|c| c == "thinking")),
    LocalApp::LmStudio => list_models(app, base)
      .await
      .ok()
      .and_then(|ms| ms.into_iter().find(|m| m.id == model))
      .is_some_and(|m| m.thinking),
  };
  remember_thinking(app, model, thinking);
  thinking
}

fn start_lock(app: LocalApp) -> &'static tokio::sync::Mutex<()> {
  static LOCKS: OnceLock<[tokio::sync::Mutex<()>; 2]> = OnceLock::new();
  let locks = LOCKS.get_or_init(|| [tokio::sync::Mutex::new(()), tokio::sync::Mutex::new(())]);
  &locks[app as usize]
}

fn hidden(cli: &std::path::Path, args: &[&str]) -> std::process::Command {
  let mut cmd = std::process::Command::new(cli);
  cmd
    .args(args)
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null());
  #[cfg(windows)]
  {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
  }
  cmd
}

/// Not tied to this app: the server keeps running for the user's other tools.
fn spawn_server(app: LocalApp, cli: &std::path::Path) -> std::io::Result<()> {
  match app {
    LocalApp::Ollama => hidden(cli, &["serve"]).spawn().map(|_| ()),
    // `lms server start` alone opens the LM Studio window when nothing is running. The headless
    // daemon first, then the server inside it, keeps everything in the background.
    LocalApp::LmStudio => {
      let cli = cli.to_path_buf();
      std::thread::spawn(move || {
        let _ = hidden(&cli, &["daemon", "up"]).status();
        let _ = hidden(&cli, &["server", "start"]).status();
      });
      Ok(())
    }
  }
}

/// Returns the base URL of a running server, starting it hidden if needed. One start at a time.
pub async fn ensure_running(app: LocalApp, p: &dyn Probe) -> Result<String, StartError> {
  let base = detect::endpoint(app, p)
    .map_err(|EndpointError::NotLocal| StartError::NotLocal)?
    .base;
  if is_up(app, &base).await {
    return Ok(base);
  }
  if !detect::installed(app, p) {
    return Err(StartError::NotInstalled);
  }
  let _guard = start_lock(app).lock().await;
  if is_up(app, &base).await {
    return Ok(base);
  }
  let cli = detect::find_cli(app, p).ok_or(StartError::NoCli)?;
  spawn_server(app, &cli).map_err(|_| StartError::Spawn)?;
  tracing::info!(app = app.id(), "starting local server");
  let deadline = tokio::time::Instant::now() + start_limit(app);
  while tokio::time::Instant::now() < deadline {
    tokio::time::sleep(Duration::from_millis(300)).await;
    if is_up(app, &base).await {
      return Ok(base);
    }
  }
  tracing::warn!(app = app.id(), "local server did not answer in time");
  Err(StartError::Timeout)
}

/// Loads the model while the user is still speaking. Errors are logged by kind only.
pub async fn warm_up(app: LocalApp, model: String) {
  let Ok(base) = ensure_running(app, &detect::SystemProbe).await else {
    return;
  };
  let c = client(Duration::from_secs(60));
  // A server that has just started can drop the first request while it finishes booting, so
  // a failed send is retried once after a short pause.
  for attempt in 1..=2 {
    let req = match app {
      LocalApp::Ollama => c
        .post(format!("{base}/api/generate"))
        .json(&json!({ "model": model, "keep_alive": "10m" })),
      // Just-in-time loading: a 1-token request loads the model without a second instance.
      LocalApp::LmStudio => c.post(format!("{base}/v1/chat/completions")).json(&json!({
        "model": model, "max_tokens": 1, "stream": false,
        "messages": [{ "role": "user", "content": "ok" }]
      })),
    };
    match req.send().await {
      Ok(r) if r.status().is_success() => {
        tracing::debug!(app = app.id(), attempt, "local model warm");
        return;
      }
      Ok(r) => tracing::warn!(
        app = app.id(),
        attempt,
        status = r.status().as_u16(),
        "warm-up rejected"
      ),
      Err(e) => tracing::warn!(
        app = app.id(),
        attempt,
        timeout = e.is_timeout(),
        connect = e.is_connect(),
        "warm-up failed"
      ),
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
  }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PullUpdate {
  pub pct: f32,
}

/// One NDJSON line from Ollama `/api/pull`. Progress is summed across layers.
pub fn ollama_pull_pct(line: &str, layers: &mut HashMap<String, (u64, u64)>) -> Result<Option<f32>, String> {
  let v: Value = serde_json::from_str(line).map_err(|_| "unexpected reply from Ollama".to_string())?;
  if let Some(e) = v["error"].as_str() {
    return Err(e.to_string());
  }
  let (Some(digest), Some(total)) = (v["digest"].as_str(), v["total"].as_u64()) else {
    return Ok(None);
  };
  layers.insert(digest.to_string(), (v["completed"].as_u64().unwrap_or(0), total));
  let (done, all) = layers.values().fold((0u64, 0u64), |(d, a), (c, t)| (d + c, a + t));
  Ok((all > 0).then(|| done as f32 / all as f32))
}

/// Downloads a model through the server itself, so it lands in the user's chosen folder.
pub async fn pull(
  app: LocalApp,
  base: &str,
  model: &str,
  cancel: CancellationToken,
  on: impl Fn(PullUpdate),
) -> Result<(), String> {
  use futures_util::StreamExt;
  let c = reqwest::Client::new();
  match app {
    LocalApp::Ollama => {
      let resp = c
        .post(format!("{base}/api/pull"))
        .json(&json!({ "model": model, "stream": true }))
        .send()
        .await
        .map_err(|_| "Could not reach Ollama".to_string())?;
      if !resp.status().is_success() {
        return Err(format!("Ollama returned HTTP {}", resp.status().as_u16()));
      }
      let mut stream = resp.bytes_stream();
      let mut buf = Vec::new();
      let mut layers = HashMap::new();
      loop {
        let chunk = tokio::select! {
          _ = cancel.cancelled() => return Err("Cancelled".into()),
          c = stream.next() => c,
        };
        let Some(chunk) = chunk else { break };
        buf.extend_from_slice(&chunk.map_err(|_| "Download interrupted".to_string())?);
        while let Some(i) = buf.iter().position(|b| *b == b'\n') {
          let line: Vec<u8> = buf.drain(..=i).collect();
          let line = String::from_utf8_lossy(&line);
          if line.trim().is_empty() {
            continue;
          }
          if let Some(pct) = ollama_pull_pct(line.trim(), &mut layers)? {
            on(PullUpdate { pct });
          }
        }
      }
      Ok(())
    }
    LocalApp::LmStudio => {
      let v: Value = c
        .post(format!("{base}/api/v1/models/download"))
        .json(&json!({ "model": model }))
        .send()
        .await
        .map_err(|_| "Could not reach LM Studio".to_string())?
        .json()
        .await
        .map_err(|_| "Unexpected reply from LM Studio".to_string())?;
      if v["status"].as_str() == Some("already_downloaded") {
        return Ok(());
      }
      let job = v["job_id"]
        .as_str()
        .ok_or("LM Studio did not start the download")?
        .to_string();
      loop {
        tokio::select! {
          // LM Studio has no cancel endpoint; stop watching and let LM Studio finish or the user cancel there.
          _ = cancel.cancelled() => return Err("Cancelled".into()),
          _ = tokio::time::sleep(Duration::from_millis(500)) => {}
        }
        let s: Value = c
          .get(format!("{base}/api/v1/models/download/status/{job}"))
          .send()
          .await
          .map_err(|_| "Lost connection to LM Studio".to_string())?
          .json()
          .await
          .map_err(|_| "Unexpected reply from LM Studio".to_string())?;
        match s["status"].as_str() {
          Some("completed") => return Ok(()),
          Some("failed") => return Err("LM Studio could not download the model".into()),
          _ => {
            if let (Some(d), Some(t)) = (s["downloaded_bytes"].as_u64(), s["total_size_bytes"].as_u64()) {
              if t > 0 {
                on(PullUpdate {
                  pct: d as f32 / t as f32,
                });
              }
            }
          }
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tokio::io::{AsyncReadExt, AsyncWriteExt};

  #[test]
  fn parses_ollama_tags() {
    let body = r#"{"models":[{"name":"gemma3:4b","model":"gemma3:4b","size":3338801804},{"name":"nomic-embed-text:latest","size":274302450}]}"#;
    let tags = parse_ollama_tags(body).unwrap();
    assert_eq!(
      tags,
      vec![
        ("gemma3:4b".to_string(), 3_338_801_804),
        ("nomic-embed-text:latest".to_string(), 274_302_450)
      ]
    );
    assert_eq!(parse_ollama_tags("<html>"), Err(ServerError::Malformed));
  }

  #[test]
  fn parses_ollama_capabilities() {
    assert_eq!(
      parse_ollama_caps(r#"{"capabilities":["completion","thinking"]}"#),
      vec!["completion", "thinking"]
    );
    assert!(parse_ollama_caps(r#"{"license":"x"}"#).is_empty());
  }

  #[test]
  fn lmstudio_keeps_only_llms() {
    let body = r#"{"models":[
      {"type":"llm","key":"google/gemma-3-12b","display_name":"Gemma 3 12B","size_bytes":8000,"capabilities":{"vision":true}},
      {"type":"llm","key":"qwen/qwen3.5-9b","display_name":"Qwen3.5 9B","size_bytes":6000,"capabilities":{"reasoning":{"allowed_options":["off","on"],"default":"on"}}},
      {"type":"embedding","key":"text-embedding-nomic","display_name":"Nomic","size_bytes":80}
    ]}"#;
    let models = parse_lmstudio_models(body).unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "google/gemma-3-12b");
    assert_eq!(models[0].name, "Gemma 3 12B");
    assert!(models[0].usable && !models[0].thinking);
    assert!(models[1].thinking);
  }

  #[test]
  fn ollama_pull_progress_sums_layers() {
    let mut layers = HashMap::new();
    assert_eq!(
      ollama_pull_pct(r#"{"status":"pulling manifest"}"#, &mut layers),
      Ok(None)
    );
    let p = ollama_pull_pct(
      r#"{"status":"pulling a","digest":"a","total":100,"completed":50}"#,
      &mut layers,
    );
    assert_eq!(p, Ok(Some(0.5)));
    let p = ollama_pull_pct(
      r#"{"status":"pulling b","digest":"b","total":100,"completed":0}"#,
      &mut layers,
    );
    assert_eq!(p, Ok(Some(0.25)));
    assert_eq!(
      ollama_pull_pct(r#"{"error":"pull model manifest: file does not exist"}"#, &mut layers),
      Err("pull model manifest: file does not exist".to_string())
    );
  }

  /// HTTP server on loopback that answers every request with `body`.
  async fn serve(body: &'static str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
      while let Ok((mut sock, _)) = listener.accept().await {
        let mut buf = [0u8; 4096];
        let _ = sock.read(&mut buf).await;
        let resp = format!(
          "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
          body.len()
        );
        let _ = sock.write_all(resp.as_bytes()).await;
      }
    });
    format!("http://{addr}")
  }

  #[tokio::test]
  async fn up_check_and_listing_against_a_local_server() {
    let base = serve(r#"{"models":[{"type":"llm","key":"m","display_name":"M","size_bytes":1}]}"#).await;
    assert!(is_up(LocalApp::LmStudio, &base).await);
    let models = list_models(LocalApp::LmStudio, &base).await.unwrap();
    assert_eq!(models[0].id, "m");
    assert!(!is_up(LocalApp::LmStudio, "http://127.0.0.1:9").await);
  }
}
