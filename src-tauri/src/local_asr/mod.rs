//! Local speech to text: model catalog, model store, downloads, linking, and the helper client.

pub mod catalog;
pub mod client;
pub mod commands;
pub mod download;
pub mod link;
pub mod store;

/// Free physical memory, used to refuse a model that cannot fit before trying to load it.
#[cfg(windows)]
pub fn available_memory() -> Option<u64> {
  use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
  let mut s = MEMORYSTATUSEX {
    dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
    ..Default::default()
  };
  unsafe { GlobalMemoryStatusEx(&mut s) }.ok()?;
  Some(s.ullAvailPhys)
}

#[cfg(not(windows))]
pub fn available_memory() -> Option<u64> {
  None
}
