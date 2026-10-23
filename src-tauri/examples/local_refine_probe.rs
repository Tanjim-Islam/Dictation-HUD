//! Dev check: `cargo run --example local_refine_probe -- <ollama|lmstudio> [model] [timeout-secs]`.
//! Starts the local server if needed, lists text models, and refines one fixed test sentence
//! (not user data), printing timings and the result.

use std::time::{Duration, Instant};

use dictation_hud::local_llm::{detect, scan, server, LocalApp};
use dictation_hud::refine;

#[tokio::main]
async fn main() {
  let mut args = std::env::args().skip(1);
  let app = args
    .next()
    .and_then(|a| LocalApp::from_id(&a))
    .expect("ollama or lmstudio");
  let p = detect::SystemProbe;
  println!("installed: {}", detect::installed(app, &p));
  println!("cli: {:?}", detect::find_cli(app, &p));
  println!("endpoint: {:?}", detect::endpoint(app, &p));
  if let Some(dir) = scan::models_dir(app, &p) {
    println!("models dir: {}", dir.display());
    for m in scan::scan(app, &dir) {
      println!("  on disk: {}  {} bytes", m.id, m.size_bytes);
    }
  }
  let t = Instant::now();
  let base = server::ensure_running(app, &p).await.expect("server start");
  println!("server up in {} ms at {base}", t.elapsed().as_millis());
  let models = server::list_models(app, &base).await.expect("model list");
  for m in &models {
    println!("  served: {}  {} bytes  thinking={}", m.id, m.size_bytes, m.thinking);
  }
  let model = args
    .next()
    .or_else(|| models.first().map(|m| m.id.clone()))
    .expect("no text models");
  println!("model: {model}");
  let req = refine::Request {
    text: "um so i think we should uh meet on tuesday at 3 pm to go over the the budget",
    vocabulary: &[],
    context: None,
    code: false,
    timeout: args
      .next()
      .and_then(|t| t.parse().ok())
      .map(Duration::from_secs)
      .unwrap_or(refine::LOCAL_LIVE_TIMEOUT),
  };
  for round in ["cold", "warm"] {
    let t = Instant::now();
    let out = refine::refine(refine::provider(app.id()), None, &model, &req).await;
    println!("{round}: {} ms -> {out:?}", t.elapsed().as_millis());
    tokio::time::sleep(Duration::from_millis(200)).await;
  }
}
