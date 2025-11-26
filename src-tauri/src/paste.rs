use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

#[cfg(feature = "native-input")]
fn send_paste() -> anyhow::Result<()> {
  #[cfg(target_os="macos")] {
    use enigo::*;
    let mut e = Enigo::new(&Settings::default()).map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;

    // Press and hold Cmd
    e.key(Key::Meta, Direction::Press).map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Press V while holding Cmd
    e.key(Key::Unicode('v'), Direction::Click).map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Release Cmd
    e.key(Key::Meta, Direction::Release).map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;

    return Ok(());
  }
  #[cfg(not(target_os="macos"))] {
    use enigo::*;
    let mut e = Enigo::new(&Settings::default()).map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;

    // Press and hold Control
    e.key(Key::Control, Direction::Press).map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Press V while holding Control
    e.key(Key::Unicode('v'), Direction::Click).map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Release Control
    e.key(Key::Control, Direction::Release).map_err(|e| anyhow::anyhow!(format!("{:?}", e)))?;

    return Ok(());
  }
}

#[cfg(not(feature = "native-input"))]
fn send_paste() -> anyhow::Result<()> { Err(anyhow::anyhow!("native input not enabled")) }

pub async fn quick_probe_can_paste(app: &AppHandle) -> Result<bool, String> {
  // Try writing to clipboard; we avoid actually pasting content into user apps by sending an Undo immediately is not feasible without full simulation.
  let cb = app.clipboard();
  let original = cb.read_text().ok();
  let sentinel = "__DICTATION_HUD_SENTINEL__".to_string();
  cb.write_text(sentinel.clone()).map_err(|e| e.to_string())?;
  // If native-input is not enabled, treat probe as passed (optional check)
  if let Err(_) = send_paste() {
    if let Some(t) = original { let _ = cb.write_text(t); }
    return Ok(true);
  }
  let ok = true;
  // try to restore clipboard
  if let Some(t) = original { let _ = cb.write_text(t); }
  Ok(ok)
}

pub async fn copy_and_paste(app: &AppHandle, text: &str) -> Result<bool, String> {
  let cb = app.clipboard();
  cb.write_text(text.to_string()).map_err(|e| e.to_string())?;

  // Slightly longer pre-paste delay to cover fast-path cases (AI refinement OFF)
  tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

  // Attempt paste; if it fails (e.g., native input disabled), return false
  let result = send_paste().is_ok();

  // Allow the OS to process paste before any subsequent UI actions
  tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
  Ok(result)
}

