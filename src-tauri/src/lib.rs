pub mod paste;
pub mod config;
pub mod hotkey;
pub mod prompt;
pub mod symbols;

use std::time::{Duration, Instant};
use std::sync::Mutex;
use tauri::{Manager, menu::{Menu, MenuItem}, tray::{TrayIconBuilder, TrayIconEvent}, AppHandle, Emitter};
use tauri_plugin_store::StoreExt;
use tauri_plugin_autostart::ManagerExt as _;
use serde::{Deserialize, Serialize};

// Helper for choosing which monitor the HUD should appear on.
// On Windows, we try to use the monitor of the foreground window (focused app).
// On other platforms, we fall back to Tauri's primary monitor.
#[cfg(all(target_os = "windows", feature = "windows-monitor"))]
mod focused_monitor {
  use windows::Win32::Foundation::{POINT, RECT};
  use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITOR_DEFAULTTONEAREST, MONITORINFO};
  use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

  /// Returns (left, top, width, height) of the work area of the monitor
  /// containing the current mouse cursor, if available.
  pub fn work_area_for_foreground_monitor() -> Option<(i32, i32, u32, u32)> {
    unsafe {
      // Use the current cursor position as a proxy for the "focused" monitor.
      let mut pt = POINT { x: 0, y: 0 };
      if GetCursorPos(&mut pt).is_err() {
        return None;
      }

      let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);

      let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
      };
      if !GetMonitorInfoW(hmon, &mut info).as_bool() {
        return None;
      }

      let RECT { left, top, right, bottom } = info.rcWork;
      let width = (right - left) as u32;
      let height = (bottom - top) as u32;
      Some((left, top, width, height))
    }
  }
}

#[cfg(not(all(target_os = "windows", feature = "windows-monitor")))]
mod focused_monitor {
  pub fn work_area_for_foreground_monitor() -> Option<(i32, i32, u32, u32)> {
    None
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BehaviorPrefs {
  auto_paste: bool,
  silence_secs: u32,
  stream_insert: bool,
  autostart: bool,
  ai_refine: bool,
  #[serde(default = "default_ai_provider")]
  ai_provider: String, // "openrouter" | "megallm"
  #[serde(default = "default_stt_provider")]
  stt_provider: String, // "deepgram" | "elevenlabs"
  echo_cancellation: bool,
  noise_suppression: bool,
}

fn default_ai_provider() -> String { "openrouter".into() }
fn default_stt_provider() -> String { "deepgram".into() }

impl Default for BehaviorPrefs {
  fn default() -> Self {
    Self {
      auto_paste: true,
      silence_secs: 2,
      stream_insert: false,
      autostart: false,
      ai_refine: true,
      ai_provider: default_ai_provider(),
      stt_provider: default_stt_provider(),
      echo_cancellation: true,
      noise_suppression: true,
    }
  }
}

// Global state to track recording status
// This prevents race conditions where window is visible but recording hasn't started yet
#[derive(Debug, Clone, Copy, PartialEq)]
enum DictationState {
  Inactive,
  Starting,  // Microphone permission + WebSocket connecting
  Recording, // Actually recording
  Stopping,  // Processing transcript + refinement
}

struct RecordingState {
  state: DictationState,
  start_time: Option<Instant>,
}

impl Default for RecordingState {
  fn default() -> Self {
    Self { state: DictationState::Inactive, start_time: None }
  }
}

static RECORDING_STATE: Mutex<RecordingState> = Mutex::new(RecordingState { state: DictationState::Inactive, start_time: None });

#[tauri::command]
async fn start_dictation(app: AppHandle) -> Result<(), String> {
  eprintln!("🚀🚀🚀 start_dictation COMMAND INVOKED 🚀🚀🚀");

  // CRITICAL: Check if already starting/recording/stopping - prevent duplicates!
  {
    let state = RECORDING_STATE.lock().unwrap();
    match state.state {
      DictationState::Starting => {
        eprintln!("⚠️ Already starting dictation, ignoring duplicate request");
        return Err("already-starting".into());
      }
      DictationState::Recording => {
        eprintln!("⚠️ Already recording, ignoring duplicate request");
        return Err("already-recording".into());
      }
      DictationState::Stopping => {
        eprintln!("⚠️ Currently stopping dictation, ignoring request");
        return Err("currently-stopping".into());
      }
      DictationState::Inactive => {
        eprintln!("✅ State is inactive, proceeding with start");
      }
    }
  }

  // Set state to Starting IMMEDIATELY to prevent race conditions
  {
    let mut state = RECORDING_STATE.lock().unwrap();
    state.state = DictationState::Starting;
    eprintln!("🎯 State set to STARTING");
  }

  // Quick probe: optional. If not acceptable, emit badge and bail.
  eprintln!("🔍 Probing if text field is accepting input...");
  let can_paste = probe_text_accepting_impl(&app).await.unwrap_or(true);
  eprintln!("Probe result: {}", if can_paste { "✅ can paste" } else { "❌ cannot paste" });

  if !can_paste {
    eprintln!("❌ No text field focused, emitting badge and returning error");
    // Reset state back to Inactive
    let mut state = RECORDING_STATE.lock().unwrap();
    state.state = DictationState::Inactive;
    app.emit_to("hud", "hud-badge", "No text field is focused").ok();
    return Err("no-focus".into());
  }

  // Show HUD window
  eprintln!("🪟 Getting HUD window...");
  if let Some(win) = app.get_webview_window("hud") {
    eprintln!("✅ HUD window found, positioning and showing it...");

    // Position HUD at bottom-center of primary monitor
    if let Ok(Some(monitor)) = win.primary_monitor() {
      let monitor_size = monitor.size();
      let hud_width = 600;
      let hud_height = 120;
      let x = (monitor_size.width as i32 - hud_width) / 2;
      let y = monitor_size.height as i32 - hud_height - 60; // 60px from bottom
      eprintln!("📍 Positioning HUD at x:{}, y:{} (monitor: {}x{})", x, y, monitor_size.width, monitor_size.height);
      let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
    } else {
      eprintln!("⚠️ Could not get primary monitor, using default position");
    }

    // Try to reposition HUD based on the foreground (focused) window's monitor when available.
    if let Some((left, top, width, height)) = focused_monitor::work_area_for_foreground_monitor() {
      let hud_width = 600;
      let hud_height = 60;
      let x = left + ((width as i32 - hud_width) / 2);
      let y = top + (height as i32 - hud_height - 60); // 60px from bottom of that monitor
      eprintln!(
        "?? Repositioning HUD to x:{}, y:{} (focused monitor work area: {}x{} at {},{})",
        x, y, width, height, left, top
      );
      let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
    }


    let _ = win.show();
    let _ = win.set_always_on_top(true);
    // CRITICAL: DO NOT steal focus! User needs focus to stay on their text field
    // let _ = win.set_focus();
    eprintln!("✅ HUD window shown, always on top (focus remains on text field)");

    // Emit start event immediately
    eprintln!("🚀 Emitting dictation-start event...");
    app.emit_to("hud", "dictation-start", ()).ok();
    eprintln!("✅✅✅ start_dictation COMPLETED SUCCESSFULLY ✅✅✅");
    Ok(())
  } else {
    eprintln!("❌ HUD window not found!");
    return Err("hud-window-not-found".into());
  }
}

#[tauri::command]
async fn stop_dictation(app: AppHandle) -> Result<(), String> {
  // Hide HUD immediately
  if let Some(win) = app.get_webview_window("hud") {
    let _ = win.hide();
  }
  Ok(())
}

#[tauri::command]
fn is_dictation_active(_app: AppHandle) -> Result<bool, String> {
  eprintln!("🔍 is_dictation_active COMMAND INVOKED");
  let state = RECORDING_STATE.lock().unwrap();
  // CRITICAL: Return true for ANY non-Inactive state to prevent duplicate starts/stops
  // Starting: microphone initializing + WebSocket connecting
  // Recording: actively recording
  // Stopping: processing transcript + refinement
  let is_active = !matches!(state.state, DictationState::Inactive);
  eprintln!("Recording state: {:?} -> {}", state.state, if is_active { "🔴 ACTIVE" } else { "⚪ INACTIVE" });
  Ok(is_active)
}

#[tauri::command]
fn set_recording_active(_app: AppHandle, new_state: String) -> Result<(), String> {
  eprintln!("🎯 set_recording_active COMMAND INVOKED: {}", new_state);
  let mut state = RECORDING_STATE.lock().unwrap();

  match new_state.as_str() {
    "recording" => {
      state.state = DictationState::Recording;
      state.start_time = Some(Instant::now());
      eprintln!("✅ State set to RECORDING");
    }
    "stopping" => {
      state.state = DictationState::Stopping;
      eprintln!("✅ State set to STOPPING");
    }
    "inactive" => {
      state.state = DictationState::Inactive;
      state.start_time = None;
      eprintln!("✅ State set to INACTIVE");
    }
    _ => {
      eprintln!("❌ Invalid state: {}", new_state);
      return Err(format!("Invalid state: {}", new_state));
    }
  }

  Ok(())
}

#[tauri::command]
async fn trigger_stop_dictation(app: AppHandle) -> Result<(), String> {
  eprintln!("🛑 trigger_stop_dictation COMMAND INVOKED");
  // Emit event to HUD to trigger stop
  app.emit_to("hud", "dictation-stop", ()).ok();
  eprintln!("✅ dictation-stop event emitted to HUD");
  Ok(())
}

#[tauri::command]
async fn refine_text(
  raw_text: String,
  app: AppHandle,
  openrouter_key: Option<String>,
  megallm_key: Option<String>,
  provider: Option<String>,
) -> Result<String, String> {
  // Step 1: Symbol replacement layer (STT -> symbols)
  let with_symbols = symbols::replace_symbols(&raw_text);
  eprintln!("📝 After symbol replacement: \"{}\" -> \"{}\"", raw_text, with_symbols);

  // Step 2: Check if AI refinement is enabled
  let behavior = get_behavior(app.clone()).await.unwrap_or_default();

  if !behavior.ai_refine {
    eprintln!("🔕 AI refinement DISABLED, returning symbol-replaced text");
    return Ok(with_symbols);
  }

  let chosen_provider = provider
    .map(|p| p.to_lowercase())
    .unwrap_or_else(|| behavior.ai_provider.clone());
  let provider = if chosen_provider == "megallm" { "megallm" } else { "openrouter" };

  eprintln!("🤖 AI refinement ENABLED using provider={}", provider);

  // Step 3: Send to AI for refinement
  match provider {
    "megallm" => refine_with_megallm(with_symbols, app, megallm_key).await,
    _ => refine_with_openrouter(with_symbols, app, openrouter_key).await,
  }
}

fn refinement_system_prompt() -> &'static str {
  prompt::get_system_prompt()
}

/// Check if AI output looks like a refusal/conversation and should be rejected
/// If rejected, we fall back to the raw STT text
fn validate_ai_output(refined: &str, raw_text: &str) -> String {
  // First sanitize any obvious AI additions
  let sanitized = prompt::sanitize_output(refined);
  
  // Check if it looks like an AI refusal/conversation
  if prompt::is_ai_refusal(&sanitized) {
    eprintln!("⚠️ AI output detected as refusal/conversation, falling back to raw text");
    eprintln!("   Rejected output: \"{}\"", sanitized);
    // Return raw text with basic punctuation cleanup
    return basic_punctuation_cleanup(raw_text);
  }
  
  // Check if the output is suspiciously different from input
  // (e.g., AI completely rewrote it or added lots of content)
  let input_words: Vec<&str> = raw_text.split_whitespace().collect();
  let output_words: Vec<&str> = sanitized.split_whitespace().collect();
  
  // If output is more than 2x the length of input, something is wrong
  if output_words.len() > input_words.len() * 2 && input_words.len() > 3 {
    eprintln!("⚠️ AI output suspiciously longer than input, falling back to raw text");
    eprintln!("   Input words: {}, Output words: {}", input_words.len(), output_words.len());
    return basic_punctuation_cleanup(raw_text);
  }
  
  sanitized
}

/// Basic punctuation cleanup for fallback when AI fails
/// This is a simple rule-based cleanup, not as good as AI but safe
fn basic_punctuation_cleanup(text: &str) -> String {
  let mut result = text.trim().to_string();
  
  // Capitalize first letter
  if let Some(first_char) = result.chars().next() {
    if first_char.is_ascii_lowercase() {
      result = first_char.to_uppercase().to_string() + &result[1..];
    }
  }
  
  // Add period at end if no ending punctuation
  if !result.is_empty() {
    let last_char = result.chars().last().unwrap();
    if !matches!(last_char, '.' | '!' | '?' | ',' | ';' | ':') {
      result.push('.');
    }
  }
  
  result
}

fn strip_think_blocks(mut s: String) -> String {
  while let Some(start) = s.find("<think>") {
    if let Some(end_rel) = s[start..].find("</think>") {
      let end = start + end_rel + "</think>".len();
      s.replace_range(start..end, "");
    } else {
      break;
    }
  }
  s.trim().to_string()
}

async fn refine_with_megallm(raw_text: String, app: AppHandle, megallm_key: Option<String>) -> Result<String, String> {
  eprintln!("?? Refining text with MegaLLM...");

  let key = match megallm_key {
    Some(k) if !k.is_empty() => k,
    _ => config::get_megallm_key(&app).await.ok_or("Missing MegaLLM key")?,
  };
  let model = config::get_megallm_model(&app)
    .await
    .unwrap_or_else(|| "gpt-4".into());

  let body = serde_json::json!({
    "model": model,
    "messages": [
      {"role":"system","content":refinement_system_prompt()},
      {"role":"user","content": raw_text}
    ]
  });

  let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(5))
    .build()
    .map_err(|e| e.to_string())?;

  let resp = client
    .post("https://ai.megallm.io/v1/chat/completions")
    .header("content-type", "application/json")
    .header("authorization", format!("Bearer {}", key))
    .json(&body)
    .send()
    .await
    .map_err(|e| e.to_string())?;

  let status = resp.status();
  let text_body = resp.text().await.map_err(|e| e.to_string())?;
  if !status.is_success() {
    return Err(format!("MegaLLM HTTP {} - {}", status, text_body));
  }

  let v: serde_json::Value = serde_json::from_str(&text_body).map_err(|e| e.to_string())?;
  let refined = v["choices"][0]["message"]["content"]
    .as_str()
    .unwrap_or("{}")
    .to_string();
  let cleaned = strip_think_blocks(refined);
  
  // Validate AI output - if it looks like a refusal/conversation, fall back to raw text
  let validated = validate_ai_output(&cleaned, &raw_text);
  eprintln!("✅ MegaLLM refined: \"{}\" -> \"{}\"", raw_text, validated);
  Ok(validated)
}

async fn refine_with_openrouter(raw_text: String, app: AppHandle, openrouter_key: Option<String>) -> Result<String, String> {
  eprintln!("?? Refining text with OpenRouter...");

  let key = match openrouter_key {
    Some(k) if !k.is_empty() => k,
    _ => config::get_openrouter_key(&app).await.ok_or("Missing OpenRouter key")?,
  };
  let model = config::get_model(&app).await.unwrap_or_else(|| "openai/gpt-oss-20b:free".into());

  let body = serde_json::json!({
    "model": model,
    "messages": [
      {"role":"system","content":refinement_system_prompt()},
      {"role":"user","content": raw_text}
    ]
  });
  let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(5)).build().map_err(|e| e.to_string())?;
  let resp = client
    .post("https://openrouter.ai/api/v1/chat/completions")
    .header("content-type","application/json")
    .header("authorization", format!("Bearer {}", key))
    .json(&body)
    .send().await.map_err(|e| e.to_string())?;
  if !resp.status().is_success() { return Err(format!("OpenRouter HTTP {}", resp.status())); }
  let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
  let refined = v["choices"][0]["message"]["content"].as_str().unwrap_or("{}").to_string();
  let cleaned = strip_think_blocks(refined);
  
  // Validate AI output - if it looks like a refusal/conversation, fall back to raw text
  let validated = validate_ai_output(&cleaned, &raw_text);
  eprintln!("✅ OpenRouter refined: \"{}\" -> \"{}\"", raw_text, validated);
  Ok(validated)
}

#[tauri::command]
async fn save_keys_secure(app: AppHandle, openrouter: String, deepgram: String, megallm: String, elevenlabs: String) -> Result<(), String> {
  if !openrouter.is_empty() { config::set_openrouter_key(&app, &openrouter).await.map_err(|e| e.to_string())?; }
  if !deepgram.is_empty() { config::set_deepgram_key(&app, &deepgram).await.map_err(|e| e.to_string())?; }
  if !megallm.is_empty() { config::set_megallm_key(&app, &megallm).await.map_err(|e| e.to_string())?; }
  if !elevenlabs.is_empty() { config::set_elevenlabs_key(&app, &elevenlabs).await.map_err(|e| e.to_string())?; }
  Ok(())
}

#[tauri::command]
async fn get_keys_secure(app: AppHandle) -> Result<(bool, bool, bool, bool), String> {
  Ok((
    config::get_openrouter_key(&app).await.is_some(),
    config::get_deepgram_key(&app).await.is_some(),
    config::get_megallm_key(&app).await.is_some(),
    config::get_elevenlabs_key(&app).await.is_some(),
  ))
}

#[tauri::command]
async fn set_hotkey(app: AppHandle, combo: String) -> Result<(), String> { hotkey::set_hotkey(&app, &combo) }

#[tauri::command]
async fn get_hotkey(app: AppHandle) -> Result<String, String> { Ok(hotkey::get_hotkey(&app)) }

#[tauri::command]
async fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
  eprintln!("⚙️ set_autostart called: enabled={}", enabled);
  let autolaunch = app.autolaunch();
  if enabled { autolaunch.enable().map_err(|e| e.to_string())?; } else { autolaunch.disable().map_err(|e| e.to_string())?; }
  // Persist autostart flag in the Store directly (do not route through set_behavior so we don't drop the field)
  let store = app.store("prefs.json").map_err(|e| e.to_string())?;
  let mut prefs = if let Some(v) = store.get("behavior") {
    eprintln!("set_autostart: existing behavior raw: {}", v);
    serde_json::from_value::<BehaviorPrefs>(v).unwrap_or_default()
  } else {
    eprintln!("set_autostart: no existing behavior in store");
    BehaviorPrefs::default()
  };
  prefs.autostart = enabled;
  let val = serde_json::to_value(&prefs).map_err(|e| e.to_string())?;
  store.set("behavior", val);
  store.save().map_err(|e| e.to_string())?;
  if let Some(v) = store.get("behavior") { eprintln!("set_autostart: after write behavior raw: {}", v); }
  eprintln!("✅ set_autostart persisted: autostart={} (OS updated)", enabled);
  Ok(())
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> Result<bool, String> {
  app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_behavior(app: AppHandle, args: serde_json::Value) -> Result<BehaviorPrefs, String> {
  eprintln!("📝 set_behavior called with args: {}", args);
  let store = app.store("prefs.json").map_err(|e| e.to_string())?;

  // Start from existing prefs or defaults
  let mut prefs = if let Some(existing) = store.get("behavior") {
    eprintln!("set_behavior: existing behavior raw: {}", existing);
    serde_json::from_value::<BehaviorPrefs>(existing).unwrap_or_default()
  } else {
    eprintln!("set_behavior: no existing behavior in store");
    BehaviorPrefs::default()
  };

  // Accept both snake_case and camelCase keys from the frontend
  let get_bool = |k1: &str, k2: &str| -> Option<bool> {
    args.get(k1).and_then(|v| v.as_bool()).or_else(|| args.get(k2).and_then(|v| v.as_bool()))
  };
  let get_u32 = |k1: &str, k2: &str| -> Option<u32> {
    args.get(k1).and_then(|v| v.as_u64()).or_else(|| args.get(k2).and_then(|v| v.as_u64())).map(|x| x as u32)
  };
  let get_str = |k1: &str, k2: &str| -> Option<String> {
    args.get(k1).and_then(|v| v.as_str()).or_else(|| args.get(k2).and_then(|v| v.as_str())).map(|s| s.to_string())
  };

  if let Some(v) = get_bool("auto_paste", "autoPaste") { prefs.auto_paste = v; }
  if let Some(v) = get_bool("stream_insert", "streamInsert") { prefs.stream_insert = v; }
  if let Some(v) = get_bool("ai_refine", "aiRefine") { prefs.ai_refine = v; }
  if let Some(v) = get_str("ai_provider", "aiProvider") {
    let normalized = v.to_lowercase();
    if normalized == "openrouter" || normalized == "megallm" {
      prefs.ai_provider = normalized;
    }
  }
  if let Some(v) = get_str("stt_provider", "sttProvider") {
    let normalized = v.to_lowercase();
    if normalized == "deepgram" || normalized == "elevenlabs" {
      prefs.stt_provider = normalized;
    }
  }
  if let Some(v) = get_bool("echo_cancellation", "echoCancellation") { prefs.echo_cancellation = v; }
  if let Some(v) = get_bool("noise_suppression", "noiseSuppression") { prefs.noise_suppression = v; }
  if let Some(v) = get_u32("silence_secs", "silenceSecs") { prefs.silence_secs = v; }

  let val = serde_json::to_value(&prefs).map_err(|e| e.to_string())?;
  store.set("behavior", val);
  store.save().map_err(|e| e.to_string())?;
  eprintln!("set_behavior: saved prefs -> {:?}", prefs);
  Ok(prefs)
}

#[tauri::command]
async fn get_behavior(app: AppHandle) -> Result<BehaviorPrefs, String> {
  let store = app.store("prefs.json").map_err(|e| e.to_string())?;
  let mut prefs = if let Some(v) = store.get("behavior") {
    eprintln!("get_behavior: behavior raw: {}", v);
    serde_json::from_value(v).unwrap_or_default()
  } else {
    eprintln!("get_behavior: no behavior found, using defaults");
    BehaviorPrefs::default()
  };
  // Authoritative autostart value comes from the OS/plugin
  if let Ok(os_enabled) = app.autolaunch().is_enabled() { prefs.autostart = os_enabled; }
  eprintln!("📦 get_behavior -> {:?}", prefs);
  Ok(prefs)
}

#[tauri::command]
async fn probe_text_accepting(app: AppHandle) -> Result<bool, String> { probe_text_accepting_impl(&app).await }

async fn probe_text_accepting_impl(app: &AppHandle) -> Result<bool, String> {
  paste::quick_probe_can_paste(app).await
}

#[tauri::command]
async fn set_model(app: AppHandle, name: String) -> Result<(), String> { config::set_model(&app, &name).await.map_err(|e| e.to_string()) }
#[tauri::command]
async fn get_model(app: AppHandle) -> Result<String, String> { Ok(config::get_model(&app).await.unwrap_or_else(|| "openai/gpt-oss-20b:free".into())) }
#[tauri::command]
async fn set_megallm_model(app: AppHandle, name: String) -> Result<(), String> { config::set_megallm_model(&app, &name).await.map_err(|e| e.to_string()) }
#[tauri::command]
async fn get_megallm_model(app: AppHandle) -> Result<String, String> { Ok(config::get_megallm_model(&app).await.unwrap_or_else(|| "gpt-4".into())) }
#[tauri::command]
async fn set_language(app: AppHandle, code: String) -> Result<(), String> { config::set_language(&app, &code).await.map_err(|e| e.to_string()) }
#[tauri::command]
async fn get_language(app: AppHandle) -> Result<String, String> { Ok(config::get_language(&app).await.unwrap_or_else(|| "en-US".into())) }

#[tauri::command]
async fn test_openrouter(app: AppHandle) -> Result<(), String> {
  let _ = refine_text("ping".into(), app, None, None, Some("openrouter".into())).await?; Ok(())
}

#[tauri::command]
async fn test_deepgram(app: AppHandle) -> Result<(), String> {
  // Browser-based test is better; here we just check presence of key.
  if config::get_deepgram_key(&app).await.is_some() { Ok(()) } else { Err("Missing Deepgram key".into()) }
}

#[tauri::command]
async fn test_megallm(app: AppHandle, api_key: Option<String>) -> Result<(), String> {
  let _ = list_megallm_models(app, api_key).await?; Ok(())
}

#[tauri::command]
async fn create_elevenlabs_token(app: AppHandle, api_key: Option<String>) -> Result<String, String> {
  let key = match api_key {
    Some(k) if !k.is_empty() => k,
    _ => config::get_elevenlabs_key(&app).await.ok_or("Missing ElevenLabs key")?,
  };
  let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(5))
    .build()
    .map_err(|e| e.to_string())?;
  let resp = client
    .post("https://api.elevenlabs.io/v1/single-use-token/realtime_scribe")
    .header("xi-api-key", key)
    .header("content-length", "0")
    .body("")
    .send()
    .await
    .map_err(|e| e.to_string())?;
  let status = resp.status();
  let body = resp.text().await.map_err(|e| e.to_string())?;
  if !status.is_success() {
    return Err(format!("ElevenLabs HTTP {} - {}", status, body));
  }
  let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
  let token = v.get("token").and_then(|t| t.as_str()).ok_or("Missing token in ElevenLabs response")?;
  Ok(token.to_string())
}

#[tauri::command]
async fn test_elevenlabs(app: AppHandle, api_key: Option<String>) -> Result<(), String> {
  // Generating a single-use token is a lightweight validity check.
  let _ = create_elevenlabs_token(app, api_key).await?;
  Ok(())
}

#[tauri::command]
async fn list_megallm_models(app: AppHandle, api_key: Option<String>) -> Result<Vec<String>, String> {
  let key = match api_key {
    Some(k) if !k.is_empty() => k,
    _ => config::get_megallm_key(&app).await.ok_or("Missing MegaLLM key")?,
  };
  let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(5))
    .build()
    .map_err(|e| e.to_string())?;
  let resp = client
    .get("https://ai.megallm.io/v1/models")
    .header("authorization", format!("Bearer {}", key))
    .send()
    .await
    .map_err(|e| e.to_string())?;
  let status = resp.status();
  let text = resp.text().await.map_err(|e| e.to_string())?;
  if !status.is_success() { return Err(format!("MegaLLM HTTP {} - {}", status, text)); }
  let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
  let models: Vec<String> = v["data"].as_array()
    .map(|arr| arr.iter().filter_map(|m| m["id"].as_str().map(|s| s.to_string())).collect())
    .unwrap_or_default();
  Ok(models)
}

#[tauri::command]
async fn insert_text(app: AppHandle, text: String) -> Result<bool, String> { paste::copy_and_paste(&app, &text).await }

#[tauri::command]
async fn runtime_keys(app: AppHandle) -> Result<(Option<String>, Option<String>, Option<String>, Option<String>), String> {
  eprintln!("dY\"`dY\"` runtime_keys COMMAND INVOKED dY\"`dY\"`");
  let or = config::get_openrouter_key(&app).await;
  let dg = config::get_deepgram_key(&app).await;
  let mg = config::get_megallm_key(&app).await;
  let el = config::get_elevenlabs_key(&app).await;
  eprintln!("Returning keys - OpenRouter: {}, Deepgram: {}, MegaLLM: {}, ElevenLabs: {}",
    if or.is_some() { "? present" } else { "? missing" },
    if dg.is_some() { "? present" } else { "? missing" },
    if mg.is_some() { "? present" } else { "? missing" },
    if el.is_some() { "? present" } else { "? missing" }
  );
  Ok((or, dg, mg, el))
}

#[tauri::command]
fn log_to_terminal(message: String) {
  eprintln!("[FRONTEND] {}", message);
}

#[tauri::command]
async fn export_test_keys(app: AppHandle) -> Result<(), String> {
  let or_key = config::get_openrouter_key(&app).await.unwrap_or_else(|| "NOT_FOUND".into());
  let dg_key = config::get_deepgram_key(&app).await.unwrap_or_else(|| "NOT_FOUND".into());
  let mg_key = config::get_megallm_key(&app).await.unwrap_or_else(|| "NOT_FOUND".into());
  let el_key = config::get_elevenlabs_key(&app).await.unwrap_or_else(|| "NOT_FOUND".into());

  let sep = "=".repeat(60);
  eprintln!("
{}", sep);
  eprintln!("?? API KEYS FOR TESTING:");
  eprintln!("{}", sep);
  eprintln!("DEEPGRAM_KEY={}", dg_key);
  eprintln!("OPENROUTER_KEY={}", or_key);
  eprintln!("MEGALLM_API_KEY={}", mg_key);
  eprintln!("ELEVENLABS_API_KEY={}", el_key);
  eprintln!("{}", sep);
  eprintln!("
Run the test with:");
  eprintln!("node test-apis.mjs {} {}", dg_key, or_key);
  eprintln!("");

  Ok(())
}


fn build_tray(app: &tauri::App) -> tauri::Result<()> {
  let menu = Menu::new(app)?;
  let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
  let start = MenuItem::with_id(app, "start", "Start Dictation", true, None::<&str>)?;
  let stop = MenuItem::with_id(app, "stop", "Stop Dictation", true, None::<&str>)?;
  let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
  let _ = menu.append(&settings)?;
  let _ = menu.append(&start)?;
  let _ = menu.append(&stop)?;
  let _ = menu.append(&quit)?;
    let _tray = TrayIconBuilder::with_id("main")
      .tooltip("Dictation HUD")
      .icon(app.default_window_icon().unwrap().clone())
      .menu(&menu)
    .on_menu_event(|app, event| {
      eprintln!("🎯🎯🎯 TRAY MENU EVENT: {}", event.id.as_ref());
      match event.id.as_ref() {
        "settings" => {
          eprintln!("📝 Tray: Opening settings window...");
          if let Some(w) = app.get_webview_window("settings") { let _ = w.show(); let _ = w.set_focus(); }
        },
        "start" => {
          eprintln!("🚀🚀🚀 Tray: Start Dictation clicked! 🚀🚀🚀");
          let app_clone = app.clone();
          tauri::async_runtime::spawn(async move {
            eprintln!("⚡ Spawning async task for start_dictation...");
            match start_dictation(app_clone).await {
              Ok(_) => eprintln!("✅ Tray start_dictation completed successfully"),
              Err(e) => eprintln!("❌ Tray start_dictation FAILED: {}", e),
            }
          });
        },
        "stop" => {
          eprintln!("⏹️ Tray: Stop Dictation clicked!");
          let app_clone = app.clone();
          tauri::async_runtime::spawn(async move {
            eprintln!("⚡ Spawning async task for stop_dictation...");
            match stop_dictation(app_clone).await {
              Ok(_) => eprintln!("✅ Tray stop_dictation completed successfully"),
              Err(e) => eprintln!("❌ Tray stop_dictation FAILED: {}", e),
            }
          });
        },
        "quit" => {
          eprintln!("👋 Tray: Quit clicked, exiting app...");
          app.exit(0);
        },
        _ => {
          eprintln!("⚠️ Unknown tray menu event: {}", event.id.as_ref());
        }
      }
    })
    .on_tray_icon_event(|_app, _ev: TrayIconEvent| {})
    .build(app)?;
  Ok(())
}

pub fn run(context: tauri::Context<tauri::Wry>) -> tauri::Result<()> {
  tauri::Builder::default()
    .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
      if let Some(w) = app.get_webview_window("settings") { let _ = w.show(); let _ = w.set_focus(); }
    }))
    .plugin(tauri_plugin_store::Builder::default().build())
    .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
    .plugin(tauri_plugin_global_shortcut::Builder::new().build())
    .plugin(tauri_plugin_clipboard_manager::init())
    .plugin(tauri_plugin_updater::Builder::new().build())
    .plugin(tauri_plugin_process::init())
    .setup(|app| {
      // ensure windows exist & hidden by default
      if let Some(s) = app.get_webview_window("settings") { let _ = s.hide(); }
      if let Some(h) = app.get_webview_window("hud") { let _ = h.hide(); let _ = h.set_decorations(false); let _ = h.set_always_on_top(true); }
      build_tray(app)?;
      let _ = hotkey::ensure_default_hotkey(app.handle().clone());
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      start_dictation, stop_dictation, is_dictation_active, set_recording_active, trigger_stop_dictation,
      refine_text,
      save_keys_secure, get_keys_secure,
      set_hotkey, get_hotkey,
      set_autostart, set_behavior, get_behavior,
      probe_text_accepting,
      set_model, get_model, set_megallm_model, get_megallm_model, set_language, get_language,
      test_openrouter, test_deepgram, test_megallm, test_elevenlabs, list_megallm_models, create_elevenlabs_token,
      insert_text, runtime_keys, log_to_terminal, export_test_keys, get_autostart
    ])
    .run(context)
}
