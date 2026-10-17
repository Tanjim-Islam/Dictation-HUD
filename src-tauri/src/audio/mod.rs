//! Audio: one authoritative native capture per session (see `capture`).

pub mod capture;
pub mod dsp;
pub mod levels;
pub mod recording;
pub mod store;

use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DeviceInfo {
  pub id: String,
  pub name: String,
  pub is_default: bool,
}

/// Input devices with stable ids (persisted as the selected microphone).
pub fn list_input_devices() -> Vec<DeviceInfo> {
  let host = cpal::default_host();
  let default_id = host
    .default_input_device()
    .and_then(|d| d.id().ok())
    .map(|i| i.to_string());
  let Ok(devices) = host.input_devices() else {
    return Vec::new();
  };
  let mut out: Vec<DeviceInfo> = devices
    .filter_map(|d| {
      let id = d.id().ok()?.to_string();
      let name = d.description().ok()?.name().to_string();
      Some(DeviceInfo {
        is_default: Some(&id) == default_id.as_ref(),
        id,
        name,
      })
    })
    .collect();
  out.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.name.cmp(&b.name)));
  out
}
