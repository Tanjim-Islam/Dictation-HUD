// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
  let context = tauri::generate_context!();
  dictation_hud::run(context).expect("error while running tauri application");
}

