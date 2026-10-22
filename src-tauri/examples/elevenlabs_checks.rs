//! Read-only API diagnostics. Arguments: <16k mono wav> <good pool id> <bad pool id>.
//! Secrets and user/account identifiers stay in memory. Output is status and counts only.
use std::time::{Duration, Instant};

use base64::Engine as _;
use dictation_hud::credentials::{names, OsSecretStore, SecretStore, SERVICE};
use dictation_hud::providers::{elevenlabs, pcm_bytes, SttOptions};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

#[tokio::main]
async fn main() {
  let args: Vec<String> = std::env::args().skip(1).collect();
  assert!(args.len() >= 3, "expected wav and two pool ids");
  let pcm = if args.get(3).is_some_and(|s| s == "--silence") {
    vec![0; 16000 * 16]
  } else {
    dictation_hud::audio::capture::load_wav_16k(std::path::Path::new(&args[0])).expect("wav")
  };
  let store = OsSecretStore::new(SERVICE);
  let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(10))
    .build()
    .unwrap();
  let mut users = Vec::new();
  let mut voices = Vec::new();
  let with_audio = !args.iter().any(|a| a == "--metadata-only");
  for (i, id) in args[1..3].iter().enumerate() {
    let secret = store.get(&names::elevenlabs(id)).expect("saved credential");
    for path in [
      "user/subscription",
      "models",
      "voices",
      "user",
      "single-use-token/realtime_scribe",
    ] {
      let start = Instant::now();
      let req = if path.starts_with("single") {
        client.post(format!("https://api.elevenlabs.io/v1/{path}"))
      } else {
        client.get(format!("https://api.elevenlabs.io/v1/{path}"))
      };
      let Ok(resp) = req
        .header("xi-api-key", &secret)
        .header("Content-Length", "0")
        .send()
        .await
      else {
        println!("candidate={} endpoint={path} network_error", i + 1);
        continue;
      };
      let status = resp.status().as_u16();
      let value = resp.json::<serde_json::Value>().await.unwrap_or_default();
      println!(
        "candidate={} endpoint={path} status={status} ms={}",
        i + 1,
        start.elapsed().as_millis()
      );
      match path {
        "user/subscription" => println!(
          "credits_used={} credits_limit={}",
          value["character_count"], value["character_limit"]
        ),
        "models" => println!(
          "models={} eleven_v3={}",
          value.as_array().map_or(0, Vec::len),
          value
            .as_array()
            .is_some_and(|a| a.iter().any(|m| m["model_id"] == "eleven_v3"))
        ),
        "voices" => {
          let ids: std::collections::HashSet<String> = value["voices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v["voice_id"].as_str().map(str::to_string))
            .collect();
          let reference_presets = [
            "SAz9YHcvj6GT2YYXdXww",
            "hpp4J3VqNfWAUOO0d1Us",
            "FGY2WhTYpPnrIDTdsKH5",
            "EXAVITQu4vr4xnSDxMaL",
            "JBFqnCBsd6RMkjVDRZzb",
          ];
          println!(
            "voices={} all_reference_presets={}",
            ids.len(),
            reference_presets.iter().all(|id| ids.contains(*id))
          );
          voices.push(ids);
        }
        "user" => users.push(value["user_id"].as_str().map(str::to_string)),
        _ => {
          if let Some(token) = value["token"].as_str().filter(|_| with_audio) {
            check_stream(i + 1, &secret, Some(token), &pcm, args.iter().any(|a| a == "--burst")).await;
          }
        }
      }
    }
    if with_audio {
      check_stream(i + 1, &secret, None, &pcm, args.iter().any(|a| a == "--burst")).await;
    }
  }
  println!("same_voice_access={}", voices.len() == 2 && voices[0] == voices[1]);
  println!(
    "same_account={}",
    users.len() == 2 && users[0].is_some() && users[0] == users[1]
  );
}

async fn check_stream(candidate: usize, secret: &str, token: Option<&str>, pcm: &[i16], burst: bool) {
  let mode = if token.is_some() { "token" } else { "header" };
  let options = SttOptions {
    model: "scribe_v2_realtime".into(),
    language: "multi".into(),
    keyterms: vec![],
  };
  let mut url = url::Url::parse(&elevenlabs::build_url(
    "wss://api.elevenlabs.io/v1/speech-to-text/realtime",
    &options,
  ))
  .unwrap();
  if let Some(token) = token {
    url.query_pairs_mut().append_pair("token", token);
  }
  let mut req = url.as_str().into_client_request().unwrap();
  if token.is_none() {
    req.headers_mut().insert("xi-api-key", secret.parse().unwrap());
  }
  let start = Instant::now();
  let Ok(Ok((ws, _))) = tokio::time::timeout(Duration::from_secs(10), tokio_tungstenite::connect_async(req)).await
  else {
    println!("candidate={candidate} auth={mode} connect_failed");
    return;
  };
  let (mut sink, mut stream) = ws.split();
  let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
  let mut started = false;
  let mut sent = 0;
  let mut tick = tokio::time::interval(if burst {
    Duration::from_nanos(1)
  } else {
    Duration::from_millis(25)
  });
  let samples = pcm.len();
  loop {
    tokio::select! {
      _ = tokio::time::sleep_until(deadline) => { println!("candidate={candidate} auth={mode} timeout sent={sent}"); break; },
      _ = tick.tick(), if started && sent <= samples => {
        let end = (sent + 1600).min(samples);
        let commit = end == sent;
        let body = serde_json::json!({"message_type":"input_audio_chunk", "audio_base_64":base64::engine::general_purpose::STANDARD.encode(pcm_bytes(&pcm[sent..end])), "sample_rate":16000,"commit":commit});
        if sink.send(Message::Text(body.to_string().into())).await.is_err() { break; }
        sent = if commit {samples + 1} else {end};
      },
      frame = stream.next() => match frame {
        Some(Ok(Message::Text(t))) => {
          let v: serde_json::Value = serde_json::from_str(&t).unwrap_or_default();
          let kind = v["message_type"].as_str().unwrap_or("unknown");
          // Only emit known protocol labels, never a provider body or transcript.
          match kind {
            "session_started" => {started = true; println!("candidate={candidate} auth={mode} started ms={}", start.elapsed().as_millis());},
            "partial_transcript" => {},
            "committed_transcript" | "insufficient_audio_activity" | "quota_exceeded" | "auth_error" | "rate_limited" | "queue_overflow" | "commit_throttled" | "chunk_size_exceeded" | "resource_exhausted" | "transcriber_error" | "invalid_request" | "input_error" | "error" => {
              println!("candidate={candidate} auth={mode} event={kind} ms={} sent_samples={}", start.elapsed().as_millis(), sent.min(samples));
              if kind != "committed_transcript" || sent > samples { break; }
            },
            _ => println!("candidate={candidate} auth={mode} other_event"),
          }
        },
        Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {println!("candidate={candidate} auth={mode} closed"); break;},
        _ => {},
      }
    }
  }
  let _ = sink.send(Message::Close(None)).await;
}
