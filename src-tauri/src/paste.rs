use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

#[cfg(feature = "native-input")]
fn send_paste() -> anyhow::Result<()> {
  use enigo::*;
  let modifier = if cfg!(target_os = "macos") {
    Key::Meta
  } else {
    Key::Control
  };
  let err = |e: enigo::InputError| anyhow::anyhow!("{e:?}");
  let mut e = Enigo::new(&Settings::default()).map_err(|e| anyhow::anyhow!("{e:?}"))?;
  e.key(modifier, Direction::Press).map_err(err)?;
  std::thread::sleep(std::time::Duration::from_millis(20));
  let click = e.key(Key::Unicode('v'), Direction::Click).map_err(err);
  std::thread::sleep(std::time::Duration::from_millis(20));
  // Always release the modifier, even if the V click failed.
  e.key(modifier, Direction::Release).map_err(err)?;
  click
}

#[cfg(not(feature = "native-input"))]
fn send_paste() -> anyhow::Result<()> {
  Err(anyhow::anyhow!("native input not enabled"))
}

/// Copies `text` to the clipboard and, when `auto_paste` is on, sends one paste keystroke.
/// Returns true only when a paste keystroke was sent.
pub async fn copy_and_paste(app: &AppHandle, text: &str, auto_paste: bool) -> Result<bool, String> {
  app
    .clipboard()
    .write_text(text.to_string())
    .map_err(|e| e.to_string())?;
  if !auto_paste {
    return Ok(false);
  }
  tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
  let pasted = match send_paste() {
    Ok(()) => true,
    Err(e) => {
      tracing::warn!(error = %e, "paste keystroke failed; text left on clipboard");
      false
    }
  };
  tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
  Ok(pasted)
}
