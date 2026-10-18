//! Win32 implementation of target capture, validation, and focus restore.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, BOOL, HANDLE, HWND, RECT};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{
  AttachThreadInput, GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, OpenProcess, OpenProcessToken,
  QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
  BringWindowToTop, DispatchMessageW, GetAncestor, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetMessageW,
  GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow, ShowWindow,
  TranslateMessage, EVENT_SYSTEM_FOREGROUND, GA_ROOT, GUITHREADINFO, MSG, SW_RESTORE, WINEVENT_OUTOFCONTEXT,
};

use super::{Foreground, Target, TargetKind, Validation};

fn hwnd(v: isize) -> HWND {
  HWND(v as *mut core::ffi::c_void)
}

fn wide_to_string(buf: &[u16], len: i32) -> String {
  String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

fn window_text(h: HWND) -> String {
  let mut buf = [0u16; 512];
  let n = unsafe { GetWindowTextW(h, &mut buf) };
  wide_to_string(&buf, n)
}

fn class_name(h: HWND) -> String {
  let mut buf = [0u16; 256];
  let n = unsafe { GetClassNameW(h, &mut buf) };
  wide_to_string(&buf, n)
}

fn process_info(pid: u32) -> (String, Option<bool>) {
  unsafe {
    let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
      // Access denied to even limited info usually means a higher-integrity process.
      return (String::new(), Some(true));
    };
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let name = if QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
      let path = wide_to_string(&buf, len as i32);
      path.rsplit(['\\', '/']).next().unwrap_or_default().to_string()
    } else {
      String::new()
    };
    let elevated = token_elevated(handle);
    let _ = CloseHandle(handle);
    (name, elevated)
  }
}

unsafe fn token_elevated(process: HANDLE) -> Option<bool> {
  let mut token = HANDLE::default();
  if OpenProcessToken(process, TOKEN_QUERY, &mut token).is_err() {
    return None;
  }
  let mut elevation = TOKEN_ELEVATION::default();
  let mut ret = 0u32;
  let ok = GetTokenInformation(
    token,
    TokenElevation,
    Some(&mut elevation as *mut _ as *mut core::ffi::c_void),
    std::mem::size_of::<TOKEN_ELEVATION>() as u32,
    &mut ret,
  )
  .is_ok();
  let _ = CloseHandle(token);
  ok.then_some(elevation.TokenIsElevated != 0)
}

fn self_elevated() -> bool {
  static SELF: OnceLock<bool> = OnceLock::new();
  *SELF.get_or_init(|| unsafe { token_elevated(GetCurrentProcess()).unwrap_or(false) })
}

const SHELL_CLASSES: &[&str] = &[
  "Shell_TrayWnd",
  "Shell_SecondaryTrayWnd",
  "Progman",
  "WorkerW",
  "NotifyIconOverflowWindow",
  "TopLevelWindowForOverflowXamlIsland",
  "Xaml_WindowedPopupClass",
];
const SHELL_PROCESSES: &[&str] = &[
  "searchhost.exe",
  "startmenuexperiencehost.exe",
  "shellexperiencehost.exe",
  "lockapp.exe",
];

fn describe(top: HWND) -> Option<Target> {
  if top.0.is_null() {
    return None;
  }
  let mut pid = 0u32;
  let thread_id = unsafe { GetWindowThreadProcessId(top, Some(&mut pid)) };
  if thread_id == 0 {
    return None;
  }
  let class = class_name(top);
  let (process, elevated) = process_info(pid);
  let kind = if pid == unsafe { GetCurrentProcessId() } {
    TargetKind::OwnApp
  } else if SHELL_CLASSES.contains(&class.as_str()) || SHELL_PROCESSES.contains(&process.to_ascii_lowercase().as_str())
  {
    TargetKind::Shell
  } else {
    TargetKind::App
  };
  let mut info = GUITHREADINFO {
    cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
    ..Default::default()
  };
  let focus = if unsafe { GetGUIThreadInfo(thread_id, &mut info) }.is_ok() {
    info.hwndFocus.0 as isize
  } else {
    0
  };
  Some(Target {
    hwnd: top.0 as isize,
    focus_hwnd: focus,
    pid,
    thread_id,
    process,
    title: window_text(top),
    class_name: class,
    kind,
    elevated_mismatch: elevated.unwrap_or(false) && !self_elevated(),
  })
}

fn foreground_top() -> HWND {
  unsafe {
    let fg = GetForegroundWindow();
    if fg.0.is_null() {
      return fg;
    }
    let root = GetAncestor(fg, GA_ROOT);
    if root.0.is_null() {
      fg
    } else {
      root
    }
  }
}

static LAST_APP: Mutex<Option<Target>> = Mutex::new(None);

/// The window in front now. If that is the shell or one of our own windows (for example the
/// user clicked the tray), fall back to the last real application window.
pub fn capture() -> Option<Target> {
  let now = describe(foreground_top());
  match now {
    Some(t) if t.kind == TargetKind::App => Some(t),
    _ => last_app_target().or(now),
  }
}

/// Last application window that was in front (tracked by a WinEvent hook).
pub fn last_app_target() -> Option<Target> {
  let t = LAST_APP.lock().unwrap().clone()?;
  (validate(&t) == Validation::Valid).then_some(t)
}

/// Center of a window in physical screen coordinates (for picking its monitor).
pub fn window_center(hwnd_value: isize) -> Option<(i32, i32)> {
  let h = hwnd(hwnd_value);
  let mut r = RECT::default();
  unsafe {
    if !IsWindow(h).as_bool() || GetWindowRect(h, &mut r).is_err() {
      return None;
    }
  }
  Some(((r.left + r.right) / 2, (r.top + r.bottom) / 2))
}

pub fn validate(t: &Target) -> Validation {
  let h = hwnd(t.hwnd);
  if !unsafe { IsWindow(h) }.as_bool() {
    return Validation::Closed;
  }
  let mut pid = 0u32;
  unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
  if pid != t.pid {
    return Validation::Replaced;
  }
  Validation::Valid
}

pub fn foreground_relative_to(t: &Target) -> Foreground {
  let top = foreground_top();
  if top.0 as isize == t.hwnd {
    return Foreground::Target;
  }
  match describe(top) {
    None => Foreground::Neutral,
    Some(d) if d.kind != TargetKind::App => Foreground::Neutral,
    Some(_) => Foreground::OtherApp,
  }
}

/// Brings the target to the front and puts keyboard focus back on the control that had it.
/// Restoring focus also ends a menu/access-key mode the app may have entered. Returns true
/// only when the target is verifiably the foreground window afterwards.
pub fn activate(t: &Target) -> bool {
  let target = hwnd(t.hwnd);
  unsafe {
    if IsIconic(target).as_bool() {
      let _ = ShowWindow(target, SW_RESTORE);
    }
    let me = GetCurrentThreadId();
    let fg = GetForegroundWindow();
    let fg_thread = if fg.0.is_null() {
      0
    } else {
      GetWindowThreadProcessId(fg, None)
    };
    // Sharing input state with the current foreground thread and the target thread lets
    // SetForegroundWindow and SetFocus succeed despite the foreground lock.
    let attach = |a: u32, b: u32, on: bool| {
      if a != 0 && b != 0 && a != b {
        let _ = AttachThreadInput(a, b, BOOL::from(on));
      }
    };
    attach(me, fg_thread, true);
    attach(me, t.thread_id, true);
    let _ = SetForegroundWindow(target);
    let _ = BringWindowToTop(target);
    if t.focus_hwnd != 0 && IsWindow(hwnd(t.focus_hwnd)).as_bool() {
      let _ = SetFocus(hwnd(t.focus_hwnd));
    }
    attach(me, t.thread_id, false);
    attach(me, fg_thread, false);
  }
  let deadline = Instant::now() + Duration::from_millis(500);
  while Instant::now() < deadline {
    if foreground_top().0 as isize == t.hwnd {
      return true;
    }
    std::thread::sleep(Duration::from_millis(15));
  }
  false
}

unsafe extern "system" fn on_foreground(
  _hook: HWINEVENTHOOK,
  _event: u32,
  hwnd: HWND,
  _id_object: i32,
  _id_child: i32,
  _thread: u32,
  _time: u32,
) {
  let top = GetAncestor(hwnd, GA_ROOT);
  let top = if top.0.is_null() { hwnd } else { top };
  if let Some(t) = describe(top) {
    if t.kind == TargetKind::App {
      *LAST_APP.lock().unwrap() = Some(t);
    }
  }
}

/// Tracks the last foreground application window on a dedicated thread (out-of-context
/// WinEvent hook; needs a message loop).
pub fn start_foreground_tracker() {
  static STARTED: OnceLock<()> = OnceLock::new();
  if STARTED.set(()).is_err() {
    return;
  }
  if let Some(t) = describe(foreground_top()).filter(|t| t.kind == TargetKind::App) {
    *LAST_APP.lock().unwrap() = Some(t);
  }
  let _ = std::thread::Builder::new()
    .name("foreground-tracker".into())
    .spawn(|| unsafe {
      let hook = SetWinEventHook(
        EVENT_SYSTEM_FOREGROUND,
        EVENT_SYSTEM_FOREGROUND,
        None,
        Some(on_foreground),
        0,
        0,
        WINEVENT_OUTOFCONTEXT,
      );
      if hook.0.is_null() {
        tracing::warn!("foreground tracker unavailable");
        return;
      }
      let mut msg = MSG::default();
      while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
      }
    });
}
