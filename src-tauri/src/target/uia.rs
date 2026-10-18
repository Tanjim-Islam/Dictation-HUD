//! UI Automation reads of the focused control (Windows).
//!
//! Used for three things: knowing whether the focused field is secure (password, PIN), reading
//! the text before the caret for context awareness, and noticing a tab switch inside one
//! window before pasting. Every read runs on its own short-lived COM thread and the caller
//! waits with a timeout, so an unresponsive app can never stall dictation. Nothing read here is
//! logged or stored.

use std::sync::mpsc;
use std::time::Duration;

use windows::core::BSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
  CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, SAFEARRAY,
};
use windows::Win32::System::Ole::{SafeArrayDestroy, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound};
use windows::Win32::UI::Accessibility::{
  CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationSelectionItemPattern, IUIAutomationTextPattern,
  IUIAutomationValuePattern, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, UIA_SelectionItemPatternId,
  UIA_TabItemControlTypeId, UIA_TextPatternId, UIA_ValuePatternId,
};

use super::focus::{is_secure_hint, FocusSnapshot, TabPos};

/// Runs `f` with a fresh UI Automation client on its own thread; `None` on timeout or error.
fn with_uia<T: Send + 'static>(
  timeout: Duration,
  f: impl FnOnce(&IUIAutomation) -> Option<T> + Send + 'static,
) -> Option<T> {
  let (tx, rx) = mpsc::channel();
  let spawned = std::thread::Builder::new().name("uia".into()).spawn(move || unsafe {
    let init = CoInitializeEx(None, COINIT_MULTITHREADED);
    let result = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
      .ok()
      .and_then(|uia| f(&uia));
    let _ = tx.send(result);
    if init.is_ok() {
      CoUninitialize();
    }
  });
  if spawned.is_err() {
    return None;
  }
  rx.recv_timeout(timeout).ok().flatten()
}

fn bstr(r: windows::core::Result<BSTR>) -> String {
  r.map(|b| b.to_string()).unwrap_or_default()
}

unsafe fn runtime_id(el: &IUIAutomationElement) -> Vec<i32> {
  let Ok(sa) = el.GetRuntimeId() else {
    return Vec::new();
  };
  let sa: *const SAFEARRAY = sa;
  let mut out = Vec::new();
  if let (Ok(lb), Ok(ub)) = (SafeArrayGetLBound(sa, 1), SafeArrayGetUBound(sa, 1)) {
    for i in lb..=ub {
      let mut v: i32 = 0;
      if SafeArrayGetElement(sa, &i, &mut v as *mut i32 as *mut core::ffi::c_void).is_ok() {
        out.push(v);
      }
    }
  }
  let _ = SafeArrayDestroy(sa);
  out
}

unsafe fn secure(el: &IUIAutomationElement) -> bool {
  if el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false) {
    return true;
  }
  is_secure_hint(
    &bstr(el.CurrentName()),
    &bstr(el.CurrentAutomationId()),
    &bstr(el.CurrentClassName()),
  )
}

/// Position of the selected tab among the tab items near the top of `hwnd`'s tree (breadth
/// first, bounded so web content is never walked).
unsafe fn selected_tab_in(uia: &IUIAutomation, hwnd: isize) -> Option<TabPos> {
  let root = uia.ElementFromHandle(HWND(hwnd as *mut core::ffi::c_void)).ok()?;
  let walker = uia.ControlViewWalker().ok()?;
  let mut level = vec![root];
  let mut tabs: Vec<IUIAutomationElement> = Vec::new();
  let mut visited = 0;
  for _depth in 0..10 {
    let mut next = Vec::new();
    for parent in &level {
      let mut child = walker.GetFirstChildElement(parent).ok();
      while let Some(c) = child {
        visited += 1;
        if visited > 800 {
          break;
        }
        if c
          .CurrentControlType()
          .map(|t| t == UIA_TabItemControlTypeId)
          .unwrap_or(false)
        {
          tabs.push(c.clone());
        } else {
          next.push(c.clone());
        }
        child = walker.GetNextSiblingElement(&c).ok();
      }
    }
    if !tabs.is_empty() || next.is_empty() || visited > 800 {
      break;
    }
    level = next;
  }
  if tabs.len() < 2 {
    return None;
  }
  for (i, t) in tabs.iter().enumerate() {
    if let Ok(p) = t.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId) {
      if p.CurrentIsSelected().map(|b| b.as_bool()).unwrap_or(false) {
        return Some(TabPos {
          index: i,
          count: tabs.len(),
          id: runtime_id(t),
        });
      }
    }
  }
  None
}

/// Focused control identity, secure-field flag, and selected tab of the target window.
pub fn focus_snapshot(hwnd: isize) -> Option<FocusSnapshot> {
  with_uia(Duration::from_millis(400), move |uia| unsafe {
    let el = uia.GetFocusedElement().ok()?;
    Some(FocusSnapshot {
      runtime_id: runtime_id(&el),
      secure: secure(&el),
      tab: selected_tab_in(uia, hwnd),
    })
  })
}

/// The selected tab of `hwnd` right now.
pub fn selected_tab(hwnd: isize) -> Option<TabPos> {
  with_uia(Duration::from_millis(300), move |uia| unsafe {
    selected_tab_in(uia, hwnd)
  })
}

fn last_chars(s: &str, n: usize) -> String {
  let count = s.chars().count();
  s.chars().skip(count.saturating_sub(n)).collect()
}

/// Text before the caret in the focused control (at most `max` chars). `None` for secure
/// fields or controls that expose no text.
pub fn text_before_caret(max: usize) -> Option<String> {
  with_uia(Duration::from_millis(400), move |uia| unsafe {
    let el = uia.GetFocusedElement().ok()?;
    if secure(&el) {
      return None;
    }
    if let Ok(tp) = el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
      let doc = tp.DocumentRange().ok()?;
      if let Ok(sel) = tp.GetSelection() {
        if sel.Length().unwrap_or(0) > 0 {
          if let Ok(caret) = sel.GetElement(0) {
            let before = doc.Clone().ok()?;
            before
              .MoveEndpointByRange(TextPatternRangeEndpoint_End, &caret, TextPatternRangeEndpoint_Start)
              .ok()?;
            let text = before.GetText(-1).map(|b| b.to_string()).ok()?;
            return Some(last_chars(&text, max));
          }
        }
      }
      let text = doc.GetText(-1).map(|b| b.to_string()).ok()?;
      return Some(last_chars(&text, max));
    }
    let vp = el
      .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
      .ok()?;
    Some(last_chars(&bstr(vp.CurrentValue()), max))
  })
}

/// Full text of the focused control with its runtime id (correction learning). `None` for
/// secure fields, unreadable controls, or text longer than `max_chars`.
pub fn focused_text(max_chars: usize) -> Option<(Vec<i32>, String)> {
  with_uia(Duration::from_millis(500), move |uia| unsafe {
    let el = uia.GetFocusedElement().ok()?;
    if secure(&el) {
      return None;
    }
    let text = if let Ok(tp) = el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
      tp.DocumentRange()
        .ok()?
        .GetText(max_chars as i32 + 1)
        .map(|b| b.to_string())
        .ok()?
    } else {
      let vp = el
        .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
        .ok()?;
      bstr(vp.CurrentValue())
    };
    if text.chars().count() > max_chars {
      return None;
    }
    Some((runtime_id(&el), text))
  })
}
