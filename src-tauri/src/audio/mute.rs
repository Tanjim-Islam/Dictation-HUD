//! Mutes other apps' audio while a dictation records, then restores it.
//!
//! Works on Windows audio sessions (the per-app entries of the volume mixer) on every active
//! output device. Left alone: this app, system sounds, known call and meeting apps, and any app
//! that is capturing a microphone right now (a call or meeting, including one in a browser tab).
//! Only sessions this module muted are unmuted again; an app the user muted stays muted.
//!
//! Windows remembers a session's mute per app, so a crash while muted would leave those apps
//! muted. The muted executables are written to a small file and unmuted at the next start.

/// Executables (lower case, without `.exe`) of call and meeting apps that are never muted.
pub const CALL_APPS: &[&str] = &[
  "discord",
  "discordptb",
  "discordcanary",
  "zoom",
  "cpthost",
  "teams",
  "ms-teams",
  "msteams",
  "whatsapp",
  "whatsapp.root",
  "slack",
  "skype",
  "lync",
  "telegram",
  "signal",
  "viber",
  "messenger",
  "webex",
  "ciscowebex",
  "ciscocollabhost",
  "atmgr",
  "ringcentral",
  "gotomeeting",
  "g2mcomm",
  "bluejeans",
  "chime",
  "element",
  "wire",
  "wechat",
  "weixin",
  "line",
  "zalo",
  "dialpad",
  "aircall",
  "phoneexperiencehost",
];

/// Whether a session of `exe` may be muted, given the executables currently capturing audio.
pub fn should_mute(exe: &str, own_exe: &str, capturing: &[String]) -> bool {
  let name = exe.trim_end_matches(".exe").trim_end_matches(".EXE").to_lowercase();
  if name.is_empty() || name == own_exe {
    return false;
  }
  !CALL_APPS.contains(&name.as_str()) && !capturing.contains(&name)
}

#[cfg(windows)]
mod imp {
  use std::path::PathBuf;
  use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
  use std::sync::{Mutex, OnceLock};
  use std::time::Duration;

  use windows::core::{Interface, PWSTR};
  use windows::Win32::Foundation::CloseHandle;
  use windows::Win32::Media::Audio::{
    eCapture, eRender, AudioSessionStateActive, EDataFlow, IAudioSessionControl2, IAudioSessionManager2,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
  };
  use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
  use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
  };

  use super::should_mute;

  /// While muted, newly started audio (a video that begins playing) is caught this often.
  const RESCAN: Duration = Duration::from_millis(1_000);

  enum Cmd {
    Mute,
    /// The sender, if any, is told when the audio is back (used on exit).
    Unmute(Option<Sender<()>>),
  }

  struct Muted {
    volume: ISimpleAudioVolume,
    exe: String,
  }

  fn exe_name(pid: u32) -> String {
    unsafe {
      let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
        return String::new();
      };
      let mut buf = [0u16; 1024];
      let mut len = buf.len() as u32;
      let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
      let _ = CloseHandle(handle);
      if !ok {
        return String::new();
      }
      let path = String::from_utf16_lossy(&buf[..len as usize]);
      path.rsplit(['\\', '/']).next().unwrap_or_default().to_string()
    }
  }

  fn lower_stem(exe: &str) -> String {
    exe.to_lowercase().trim_end_matches(".exe").to_string()
  }

  /// Every audio session on every active device of one direction.
  fn sessions(devices: &IMMDeviceEnumerator, flow: EDataFlow) -> Vec<IAudioSessionControl2> {
    let mut out = Vec::new();
    unsafe {
      let Ok(list) = devices.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) else {
        return out;
      };
      for d in 0..list.GetCount().unwrap_or(0) {
        let Ok(device) = list.Item(d) else { continue };
        let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
          continue;
        };
        let Ok(all) = manager.GetSessionEnumerator() else {
          continue;
        };
        for i in 0..all.GetCount().unwrap_or(0) {
          if let Some(s) = all
            .GetSession(i)
            .ok()
            .and_then(|s| s.cast::<IAudioSessionControl2>().ok())
          {
            out.push(s);
          }
        }
      }
    }
    out
  }

  /// Executables that are recording from a microphone right now (calls, meetings).
  fn capturing(devices: &IMMDeviceEnumerator) -> Vec<String> {
    let mut out: Vec<String> = sessions(devices, eCapture)
      .into_iter()
      .filter(|s| unsafe { s.GetState() }.is_ok_and(|st| st == AudioSessionStateActive))
      .filter_map(|s| unsafe { s.GetProcessId() }.ok())
      .map(|pid| lower_stem(&exe_name(pid)))
      .filter(|n| !n.is_empty())
      .collect();
    out.sort();
    out.dedup();
    out
  }

  struct Worker {
    devices: IMMDeviceEnumerator,
    own_pid: u32,
    own_exe: String,
    muted: Vec<Muted>,
  }

  impl Worker {
    fn mute_new(&mut self) {
      let capturing = capturing(&self.devices);
      let mut newly: Vec<String> = Vec::new();
      for s in sessions(&self.devices, eRender) {
        unsafe {
          if s.IsSystemSoundsSession().0 == 0 {
            continue; // S_OK: the system sounds session
          }
          let Ok(pid) = s.GetProcessId() else { continue };
          if pid == 0 || pid == self.own_pid {
            continue;
          }
          let exe = exe_name(pid);
          if !should_mute(&exe, &self.own_exe, &capturing) {
            continue;
          }
          let Ok(volume) = s.cast::<ISimpleAudioVolume>() else {
            continue;
          };
          if volume.GetMute().is_ok_and(|m| m.as_bool()) {
            continue; // already muted (by the user, or by us on an earlier scan)
          }
          if volume.SetMute(true, std::ptr::null()).is_ok() {
            newly.push(lower_stem(&exe));
            self.muted.push(Muted {
              volume,
              exe: lower_stem(&exe),
            });
          }
        }
      }
      if !newly.is_empty() {
        tracing::info!(apps = ?newly, kept = ?capturing, "muted other apps");
      }
      // A call that started during the dictation: give its app its sound back.
      let (keep, release): (Vec<Muted>, Vec<Muted>) = self.muted.drain(..).partition(|m| !capturing.contains(&m.exe));
      for m in release {
        unsafe {
          let _ = m.volume.SetMute(false, std::ptr::null());
        }
      }
      self.muted = keep;
      save_marker(&self.muted.iter().map(|m| m.exe.clone()).collect::<Vec<_>>());
    }

    fn unmute_all(&mut self) {
      let n = self.muted.len();
      for m in self.muted.drain(..) {
        unsafe {
          let _ = m.volume.SetMute(false, std::ptr::null());
        }
      }
      save_marker(&[]);
      if n > 0 {
        tracing::info!(sessions = n, "restored other apps' audio");
      }
    }

    /// After a crash: unmute sessions of the apps the marker file lists.
    fn recover(&mut self) {
      let names = load_marker();
      if names.is_empty() {
        return;
      }
      let mut n = 0;
      for s in sessions(&self.devices, eRender) {
        unsafe {
          let Ok(pid) = s.GetProcessId() else { continue };
          if !names.contains(&lower_stem(&exe_name(pid))) {
            continue;
          }
          if let Ok(v) = s.cast::<ISimpleAudioVolume>() {
            if v.GetMute().is_ok_and(|m| m.as_bool()) && v.SetMute(false, std::ptr::null()).is_ok() {
              n += 1;
            }
          }
        }
      }
      save_marker(&[]);
      tracing::info!(
        sessions = n,
        apps = names.len(),
        "unmuted apps left muted by an earlier run"
      );
    }
  }

  fn run(rx: Receiver<Cmd>) {
    unsafe {
      let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let devices: IMMDeviceEnumerator = match unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) } {
      Ok(d) => d,
      Err(e) => {
        tracing::warn!(error = %e, "audio muting unavailable");
        return;
      }
    };
    let own_pid = std::process::id();
    let mut w = Worker {
      own_exe: lower_stem(&exe_name(own_pid)),
      devices,
      own_pid,
      muted: Vec::new(),
    };
    w.recover();
    let mut active = false;
    loop {
      let cmd = if active {
        rx.recv_timeout(RESCAN)
      } else {
        rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
      };
      match cmd {
        Ok(Cmd::Mute) => {
          active = true;
          w.mute_new();
        }
        Ok(Cmd::Unmute(done)) => {
          active = false;
          w.unmute_all();
          if let Some(done) = done {
            let _ = done.send(());
          }
        }
        Err(RecvTimeoutError::Timeout) => w.mute_new(),
        Err(RecvTimeoutError::Disconnected) => {
          w.unmute_all();
          return;
        }
      }
    }
  }

  static TX: Mutex<Option<Sender<Cmd>>> = Mutex::new(None);
  static MARKER: OnceLock<Option<PathBuf>> = OnceLock::new();

  fn marker() -> Option<&'static PathBuf> {
    MARKER.get().and_then(|p| p.as_ref())
  }

  fn save_marker(names: &[String]) {
    let Some(path) = marker() else { return };
    if names.is_empty() {
      let _ = std::fs::remove_file(path);
    } else {
      let _ = std::fs::write(path, names.join("\n"));
    }
  }

  fn load_marker() -> Vec<String> {
    marker()
      .and_then(|p| std::fs::read_to_string(p).ok())
      .map(|s| s.lines().map(str::to_string).filter(|l| !l.is_empty()).collect())
      .unwrap_or_default()
  }

  /// Starts the worker thread. `state_dir` holds the crash-recovery marker file.
  pub fn init(state_dir: Option<PathBuf>) {
    let _ = MARKER.set(state_dir.map(|d| d.join("muted-apps.txt")));
    let (tx, rx) = mpsc::channel();
    if std::thread::Builder::new()
      .name("audio-mute".into())
      .spawn(move || run(rx))
      .is_ok()
    {
      *TX.lock().unwrap() = Some(tx);
    }
  }

  fn send(cmd: Cmd) {
    if let Some(tx) = TX.lock().unwrap().as_ref() {
      let _ = tx.send(cmd);
    }
  }

  pub fn mute_others() {
    send(Cmd::Mute);
  }

  pub fn restore() {
    send(Cmd::Unmute(None));
  }

  /// Restores and waits briefly (the app is about to exit).
  pub fn restore_now() {
    let (tx, rx) = mpsc::channel();
    send(Cmd::Unmute(Some(tx)));
    let _ = rx.recv_timeout(Duration::from_millis(800));
  }
}

#[cfg(windows)]
pub use imp::{init, mute_others, restore, restore_now};

#[cfg(not(windows))]
pub fn init(_state_dir: Option<std::path::PathBuf>) {}
#[cfg(not(windows))]
pub fn mute_others() {}
#[cfg(not(windows))]
pub fn restore() {}
#[cfg(not(windows))]
pub fn restore_now() {}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn calls_meetings_and_self_are_never_muted() {
    let none: Vec<String> = vec![];
    assert!(should_mute("Spotify.exe", "dictation-hud", &none));
    assert!(should_mute("brave.exe", "dictation-hud", &none));
    assert!(!should_mute("Discord.exe", "dictation-hud", &none));
    assert!(!should_mute("ms-teams.exe", "dictation-hud", &none));
    assert!(!should_mute("Zoom.exe", "dictation-hud", &none));
    assert!(!should_mute("WhatsApp.exe", "dictation-hud", &none));
    assert!(!should_mute("dictation-hud.exe", "dictation-hud", &none));
    assert!(!should_mute("", "dictation-hud", &none));
  }

  #[test]
  fn an_app_using_the_microphone_is_in_a_call() {
    // A Google Meet tab: the browser is capturing the microphone.
    let capturing = vec!["chrome".to_string()];
    assert!(!should_mute("chrome.exe", "dictation-hud", &capturing));
    assert!(should_mute("spotify.exe", "dictation-hud", &capturing));
  }
}
