//! Flow Bar window visibility and placement. Rebuilt in Phase 7.

use tauri::{AppHandle, Manager};

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
  if let Some(win) = app.get_webview_window("hud") {
    let _ = win.hide();
  }
}
