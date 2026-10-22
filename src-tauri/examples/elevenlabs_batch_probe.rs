//! Live diagnostic: `<16k mono wav> <pool ids...>`. Sends the file to the regular (non-realtime)
//! ElevenLabs speech-to-text endpoint with each saved key and prints status, timing, word count
//! and the provider's error code. Credentials and transcript text are never printed.
use std::time::{Duration, Instant};

use dictation_hud::credentials::{names, OsSecretStore, SecretStore, SERVICE};

#[tokio::main]
async fn main() {
  let args: Vec<String> = std::env::args().skip(1).collect();
  assert!(args.len() >= 2, "expected wav and pool ids");
  let wav = std::fs::read(&args[0]).expect("wav");
  let store = OsSecretStore::new(SERVICE);
  let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(120))
    .build()
    .unwrap();
  let boundary = "dictationhudprobeboundary";
  let mut body = Vec::new();
  body.extend_from_slice(
    format!("--{boundary}\r\nContent-Disposition: form-data; name=\"model_id\"\r\n\r\nscribe_v1\r\n").as_bytes(),
  );
  body.extend_from_slice(
    format!(
      "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
    )
    .as_bytes(),
  );
  body.extend_from_slice(&wav);
  body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
  for id in &args[1..] {
    let secret = store.get(&names::elevenlabs(id)).expect("saved credential");
    let start = Instant::now();
    let resp = client
      .post("https://api.elevenlabs.io/v1/speech-to-text")
      .header("xi-api-key", secret)
      .header("content-type", format!("multipart/form-data; boundary={boundary}"))
      .body(body.clone())
      .send()
      .await;
    let ms = start.elapsed().as_millis();
    match resp {
      Ok(r) => {
        let status = r.status().as_u16();
        let v: serde_json::Value = r.json().await.unwrap_or_default();
        let words = v["text"].as_str().map(|t| t.split_whitespace().count());
        let code = v["detail"]["status"].as_str().unwrap_or("");
        let message: String = v["detail"]["message"]
          .as_str()
          .unwrap_or("")
          .chars()
          .take(400)
          .collect();
        println!("key={id} http={status} ms={ms} words={words:?} error={code} {message}");
      }
      Err(e) => println!("key={id} request_failed ms={ms} timeout={}", e.is_timeout()),
    }
  }
}
