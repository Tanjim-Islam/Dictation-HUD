//! Flow Bar window: visibility, size, and placement.
//!
//! Rust owns the geometry. The window is exactly the visible pill (no transparent margin, so
//! nothing invisible catches clicks): 100x28 docked at the top or bottom edge, 28x100 at the
//! left or right edge, or a wider one-line banner for notices and errors. It appears on the
//! monitor of the session's target window (falling back to the cursor's monitor), sized in
//! physical pixels for that monitor's scale. The window is never focusable, so showing it
//! never takes focus from the app being dictated into.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};

use crate::session::Phase;

pub const LABEL: &str = "flowbar";
pub const NOTICE_EVENT: &str = "session://notice";
const NOTICE_MS: u64 = 3_200;

/// Pill size in logical pixels (long side x short side).
pub const PILL_LONG: f64 = 100.0;
pub const PILL_SHORT: f64 = 28.0;
/// Distance from the work-area edge (logical pixels).
pub const EDGE_GAP: f64 = 14.0;
const BANNER_MIN: f64 = 140.0;
const BANNER_MAX: f64 = 520.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
  Top,
  #[default]
  Bottom,
  Left,
  Right,
}

impl Edge {
  pub fn parse(s: &str) -> Option<Edge> {
    match s {
      "top" => Some(Edge::Top),
      "bottom" => Some(Edge::Bottom),
      "left" => Some(Edge::Left),
      "right" => Some(Edge::Right),
      _ => None,
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Layout {
  /// The dictation pill, horizontal or vertical by edge.
  Pill,
  /// A one-line horizontal banner of this logical width.
  Banner(f64),
}

/// Physical-pixel rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
  pub x: i32,
  pub y: i32,
  pub w: u32,
  pub h: u32,
}

/// Banner width for a message: rough text width at 12.5 px plus padding, bounded.
pub fn banner_width(message: &str) -> f64 {
  (message.chars().count() as f64 * 6.6 + 44.0).clamp(BANNER_MIN, BANNER_MAX)
}

/// Where the window goes inside a monitor's work area. Pure, so docking and DPI are tested.
pub fn place(work: Rect, scale: f64, edge: Edge, layout: Layout) -> Rect {
  let px = |v: f64| (v * scale).round() as i32;
  let (w, h) = match (layout, edge) {
    (Layout::Pill, Edge::Top | Edge::Bottom) => (px(PILL_LONG), px(PILL_SHORT)),
    (Layout::Pill, Edge::Left | Edge::Right) => (px(PILL_SHORT), px(PILL_LONG)),
    (Layout::Banner(width), _) => (px(width.min(work.w as f64 / scale - 2.0 * EDGE_GAP)), px(PILL_SHORT)),
  };
  let gap = px(EDGE_GAP);
  let (ww, wh) = (work.w as i32, work.h as i32);
  let center_x = work.x + (ww - w) / 2;
  let center_y = work.y + (wh - h) / 2;
  let (x, y) = match edge {
    Edge::Top => (center_x, work.y + gap),
    Edge::Bottom => (center_x, work.y + wh - h - gap),
    Edge::Left => (work.x + gap, center_y),
    Edge::Right => (work.x + ww - w - gap, center_y),
  };
  Rect {
    x,
    y,
    w: w.max(1) as u32,
    h: h.max(1) as u32,
  }
}

/// While a notice is on screen the window must not be hidden by the Idle transition.
static NOTICE_UNTIL: AtomicU64 = AtomicU64::new(0);
/// The window the current (or last) session targets; decides the monitor.
static ANCHOR: Mutex<Option<isize>> = Mutex::new(None);

fn now_ms() -> u64 {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_millis() as u64)
    .unwrap_or(0)
}

fn notice_active() -> bool {
  now_ms() < NOTICE_UNTIL.load(Ordering::SeqCst)
}

/// Work area and scale of the monitor showing the anchor window, else the cursor's monitor.
fn monitor(app: &AppHandle) -> Option<(Rect, f64)> {
  let anchor = *ANCHOR.lock().unwrap();
  let point = anchor
    .and_then(crate::target::window_center)
    .map(|(x, y)| (x as f64, y as f64))
    .or_else(|| app.cursor_position().ok().map(|p| (p.x, p.y)));
  let m = match point {
    Some((x, y)) => app.monitor_from_point(x, y).ok().flatten(),
    None => None,
  }
  .or_else(|| app.primary_monitor().ok().flatten())?;
  let wa = m.work_area();
  Some((
    Rect {
      x: wa.position.x,
      y: wa.position.y,
      w: wa.size.width,
      h: wa.size.height,
    },
    m.scale_factor(),
  ))
}

fn apply(app: &AppHandle, layout: Layout) {
  let Some(win) = app.get_webview_window(LABEL) else {
    return;
  };
  let edge = Edge::parse(&crate::settings::load(app).flowbar_edge).unwrap_or_default();
  if let Some((work, scale)) = monitor(app) {
    let r = place(work, scale, edge, layout);
    // Move first (a monitor with another DPI may rescale the window), then size, then
    // re-apply the position for the final size.
    let _ = win.set_position(PhysicalPosition { x: r.x, y: r.y });
    let _ = win.set_size(PhysicalSize {
      width: r.w,
      height: r.h,
    });
    let _ = win.set_position(PhysicalPosition { x: r.x, y: r.y });
  }
  if !win.is_visible().unwrap_or(false) {
    let _ = win.show();
  }
  let _ = win.set_always_on_top(true);
}

fn hide_window(app: &AppHandle) {
  if let Some(win) = app.get_webview_window(LABEL) {
    let _ = win.hide();
  }
}

/// Follows the session phase. `anchor` is the target window of the session, if any.
pub fn present(app: &AppHandle, phase: Phase, anchor: Option<isize>, error: Option<&str>) {
  if anchor.is_some() {
    *ANCHOR.lock().unwrap() = anchor;
  }
  match phase {
    Phase::Idle => {
      if !notice_active() {
        hide_window(app);
      }
    }
    Phase::Error => apply(
      app,
      Layout::Banner(banner_width(error.unwrap_or("Something went wrong"))),
    ),
    _ => {
      // A new session replaces any notice still showing.
      NOTICE_UNTIL.store(0, Ordering::SeqCst);
      apply(app, Layout::Pill)
    }
  }
}

/// Startup: the window stays hidden until a session starts.
pub fn hide(app: &AppHandle) {
  if !notice_active() {
    hide_window(app);
  }
}

/// Shows a short message (for example why text was only copied). The banner hides again
/// after a few seconds unless a new session is running.
pub fn notice(app: &AppHandle, message: &str) {
  NOTICE_UNTIL.store(now_ms() + NOTICE_MS, Ordering::SeqCst);
  apply(app, Layout::Banner(banner_width(message)));
  let _ = app.emit_to(LABEL, NOTICE_EVENT, message);
  let app = app.clone();
  tauri::async_runtime::spawn(async move {
    tokio::time::sleep(std::time::Duration::from_millis(NOTICE_MS + 50)).await;
    let idle = match app.try_state::<crate::session::SessionHandle>() {
      Some(s) => s.snapshot().await.map(|s| s.phase == Phase::Idle).unwrap_or(true),
      None => true,
    };
    if idle && !notice_active() {
      hide_window(&app);
    }
  });
}

#[cfg(test)]
mod tests {
  use super::*;

  const WORK: Rect = Rect {
    x: 0,
    y: 0,
    w: 1920,
    h: 1032,
  };

  #[test]
  fn pill_docks_to_each_edge_centered() {
    let b = place(WORK, 1.0, Edge::Bottom, Layout::Pill);
    assert_eq!(
      b,
      Rect {
        x: 910,
        y: 1032 - 28 - 14,
        w: 100,
        h: 28
      }
    );
    let t = place(WORK, 1.0, Edge::Top, Layout::Pill);
    assert_eq!((t.x, t.y, t.w, t.h), (910, 14, 100, 28));
    let l = place(WORK, 1.0, Edge::Left, Layout::Pill);
    assert_eq!((l.x, l.y, l.w, l.h), (14, 466, 28, 100), "vertical on side edges");
    let r = place(WORK, 1.0, Edge::Right, Layout::Pill);
    assert_eq!((r.x, r.y, r.w, r.h), (1920 - 28 - 14, 466, 28, 100));
  }

  #[test]
  fn scales_for_high_dpi_and_offset_monitors() {
    // Second monitor to the right at 150 %.
    let work = Rect {
      x: 1920,
      y: 0,
      w: 2560,
      h: 1400,
    };
    let b = place(work, 1.5, Edge::Bottom, Layout::Pill);
    assert_eq!((b.w, b.h), (150, 42));
    assert_eq!(b.x, 1920 + (2560 - 150) / 2);
    assert_eq!(b.y, 1400 - 42 - 21);
    assert!(b.x >= work.x && b.x + b.w as i32 <= work.x + work.w as i32);
  }

  #[test]
  fn banner_is_horizontal_on_every_edge_and_fits() {
    let w = banner_width("Copied. You switched to another app.");
    assert!(w > BANNER_MIN && w < BANNER_MAX);
    let l = place(WORK, 1.0, Edge::Left, Layout::Banner(w));
    assert_eq!(l.h, 28);
    assert_eq!(l.x, 14);
    assert_eq!(banner_width(&"x".repeat(500)), BANNER_MAX);
    let tiny = Rect {
      x: 0,
      y: 0,
      w: 300,
      h: 300,
    };
    let b = place(tiny, 1.0, Edge::Bottom, Layout::Banner(BANNER_MAX));
    assert!(
      b.x >= 0 && b.x + b.w as i32 <= 300,
      "banner never leaves a small screen"
    );
  }

  #[test]
  fn edge_parsing() {
    assert_eq!(Edge::parse("left"), Some(Edge::Left));
    assert_eq!(Edge::parse("middle"), None);
    assert_eq!(Edge::default(), Edge::Bottom);
  }
}
