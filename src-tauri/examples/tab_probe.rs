//! Prints the selected tab of a window: `cargo run --example tab_probe -- <hwnd>`.
//! Used to verify tab detection on tabbed windows without sending any input.

fn main() {
  let hwnd: isize = std::env::args()
    .nth(1)
    .and_then(|a| a.parse().ok())
    .expect("usage: tab_probe <hwnd>");
  let started = std::time::Instant::now();
  let tab = dictation_hud::target::selected_tab(hwnd);
  println!("tab={tab:?} ms={}", started.elapsed().as_millis());
}
