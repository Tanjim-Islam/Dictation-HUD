//! Flow Bar window visibility and placement. Rebuilt in Phase 7.

use tauri::{AppHandle, Emitter, Manager};

pub const NOTICE_EVENT: &str = "session://notice";
const NOTICE_MS: u64 = 3_200;

/// While a notice is on screen the window must not be hidden by the Idle transition.
static NOTICE_UNTIL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn now_ms() -> u64 {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_millis() as u64)
    .unwrap_or(0)
}

#[cfg(target_os = "windows")]
fn cursor_work_area() -> Option<(i32, i32, i32, i32)> {
  use windows::Win32::Foundation::POINT;
  use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
  use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
  unsafe {
    let mut pt = POINT::default();
    GetCursorPos(&mut pt).ok()?;
    let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
    let mut info = MONITORINFO {
      cbSize: std::mem::size_of::<MONITORINFO>() as u32,
      ..Default::default()
    };
    if !GetMonitorInfoW(mon, &mut info).as_bool() {
      return None;
    }
    let r = info.rcWork;
    Some((r.left, r.top, r.right - r.left, r.bottom - r.top))
  }
}

#[cfg(not(target_os = "windows"))]
fn cursor_work_area() -> Option<(i32, i32, i32, i32)> {
  None
}

pub fn show(app: &AppHandle) {
  let Some(win) = app.get_webview_window("hud") else {
    return;
  };
  if win.is_visible().unwrap_or(false) {
    return;
  }
  if let Some((left, top, width, height)) = cursor_work_area() {
    let size = win
      .outer_size()
      .map(|s| (s.width as i32, s.height as i32))
      .unwrap_or((600, 120));
    let x = left + (width - size.0) / 2;
    let y = top + height - size.1 - 24;
    let _ = win.set_position(tauri::PhysicalPosition { x, y });
  }
  let _ = win.show();
  let _ = win.set_always_on_top(true);
}

pub fn hide(app: &AppHandle) {
  if now_ms() < NOTICE_UNTIL.load(std::sync::atomic::Ordering::SeqCst) {
    return;
  }
  if let Some(win) = app.get_webview_window("hud") {
    let _ = win.hide();
  }
}

/// Shows a short message in the Flow Bar window (for example why text was only copied).
/// The window hides again after the message unless a new session is running.
pub fn notice(app: &AppHandle, message: &str) {
  NOTICE_UNTIL.store(now_ms() + NOTICE_MS, std::sync::atomic::Ordering::SeqCst);
  show(app);
  let _ = app.emit_to("hud", NOTICE_EVENT, message);
  let app = app.clone();
  tauri::async_runtime::spawn(async move {
    tokio::time::sleep(std::time::Duration::from_millis(NOTICE_MS + 50)).await;
    let idle = match app.try_state::<crate::session::SessionHandle>() {
      Some(s) => s
        .snapshot()
        .await
        .map(|s| s.phase == crate::session::Phase::Idle)
        .unwrap_or(true),
      None => true,
    };
    if idle {
      hide(&app);
    }
  });
}
