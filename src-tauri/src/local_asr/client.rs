//! The local speech helper process (`dictation-asr.exe`): launch, Job Object, one request at a
//! time with a watchdog, and exit after idle minutes to free memory. One session at a time.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use asr_protocol::{decode, encode, pcm_to_bytes, Decoder, ModelKind, Reply, Request};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{Mutex, OwnedMutexGuard};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperError {
  /// The helper executable is not next to the app (deleted or quarantined).
  Missing,
  Crashed,
  Hang,
  Load(String),
  Oom,
  NotInstalled,
}

impl HelperError {
  /// Short reason, shown as "Speech provider error (<kind>)".
  pub fn kind(&self) -> String {
    match self {
      HelperError::Missing => "local engine missing, reinstall the app".into(),
      HelperError::Crashed => "local engine stopped".into(),
      HelperError::Hang => "local engine timed out".into(),
      HelperError::Load(_) => "model failed to load".into(),
      HelperError::Oom => "not enough memory for this model".into(),
      HelperError::NotInstalled => "model not installed".into(),
    }
  }
}

pub struct HelperIo {
  pub stdin: Box<dyn AsyncWrite + Send + Unpin>,
  pub stdout: Box<dyn AsyncRead + Send + Unpin>,
  pub kill: Box<dyn FnMut() + Send>,
}

pub trait Launcher: Send + Sync {
  fn launch(&self) -> Result<HelperIo, HelperError>;
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
  pub load: Duration,
  pub audio: Duration,
  pub flush: Duration,
  pub idle: Duration,
}

impl Default for Limits {
  fn default() -> Self {
    Self {
      load: Duration::from_secs(120),
      audio: Duration::from_secs(30),
      flush: Duration::from_secs(45),
      idle: Duration::from_secs(600),
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadSpec {
  pub kind: ModelKind,
  /// Each file role of `kind` mapped to its path.
  pub files: BTreeMap<String, PathBuf>,
  pub vad: PathBuf,
  /// Model language code, or empty for auto-detect.
  pub language: String,
}

struct Live {
  io: HelperIo,
  dec: Decoder,
  loaded: Option<LoadSpec>,
}

impl Live {
  /// Sends one request; returns the segments and the terminal reply.
  async fn request(
    &mut self,
    req: &Request,
    body: &[u8],
    limit: Duration,
  ) -> Result<(Vec<String>, Reply), HelperError> {
    let work = async {
      self
        .io
        .stdin
        .write_all(&encode(req, body))
        .await
        .map_err(|_| HelperError::Crashed)?;
      self.io.stdin.flush().await.map_err(|_| HelperError::Crashed)?;
      let mut segments = Vec::new();
      let mut buf = vec![0u8; 16 * 1024];
      loop {
        while let Some((h, _)) = self.dec.next_frame().map_err(|_| HelperError::Crashed)? {
          match decode::<Reply>(&h).map_err(|_| HelperError::Crashed)? {
            Reply::Segment { text } => segments.push(text),
            other => return Ok((segments, other)),
          }
        }
        let n = self.io.stdout.read(&mut buf).await.map_err(|_| HelperError::Crashed)?;
        if n == 0 {
          return Err(HelperError::Crashed);
        }
        self.dec.push(&buf[..n]);
      }
    };
    tokio::time::timeout(limit, work)
      .await
      .unwrap_or(Err(HelperError::Hang))
  }
}

fn kill(slot: &mut Option<Live>) {
  if let Some(mut live) = slot.take() {
    (live.io.kill)();
  }
}

pub struct Helper {
  launcher: Arc<dyn Launcher>,
  live: Arc<Mutex<Option<Live>>>,
  last_used: std::sync::Mutex<Instant>,
  limits: Limits,
  threads: i32,
}

impl Helper {
  pub fn new(launcher: Arc<dyn Launcher>, limits: Limits) -> Arc<Self> {
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get().clamp(1, 4)) as i32;
    Arc::new(Self {
      launcher,
      live: Arc::new(Mutex::new(None)),
      last_used: std::sync::Mutex::new(Instant::now()),
      limits,
      threads,
    })
  }

  fn touch(&self) {
    *self.last_used.lock().unwrap() = Instant::now();
  }

  /// Exclusive use of the helper with `spec` loaded and its buffers reset. Waits while another
  /// session (for example a History retry) is running.
  pub async fn session(self: &Arc<Self>, spec: &LoadSpec) -> Result<Session, HelperError> {
    let mut guard = self.live.clone().lock_owned().await;
    self.touch();
    if guard.is_none() {
      *guard = Some(Live {
        io: self.launcher.launch()?,
        dec: Decoder::default(),
        loaded: None,
      });
    }
    let result = self.prepare(guard.as_mut().expect("just launched"), spec).await;
    if let Err(e) = result {
      kill(&mut guard);
      return Err(e);
    }
    Ok(Session {
      guard,
      helper: self.clone(),
    })
  }

  async fn prepare(&self, live: &mut Live, spec: &LoadSpec) -> Result<(), HelperError> {
    if live.loaded.as_ref() != Some(spec) {
      let req = Request::Load {
        kind: spec.kind,
        files: spec
          .files
          .iter()
          .map(|(role, path)| (role.clone(), path.to_string_lossy().into_owned()))
          .collect(),
        vad: spec.vad.to_string_lossy().into_owned(),
        language: spec.language.clone(),
        threads: self.threads,
      };
      match live.request(&req, &[], self.limits.load).await {
        Ok((_, Reply::Loaded { ms })) => {
          tracing::info!(ms, kind = ?spec.kind, "local model loaded");
          live.loaded = Some(spec.clone());
        }
        Ok((_, Reply::Error { detail, .. })) => return Err(HelperError::Load(detail)),
        Ok(_) => return Err(HelperError::Crashed),
        // A crash while loading is almost always the runtime giving up on the model.
        Err(HelperError::Crashed) => return Err(HelperError::Load("helper stopped while loading".into())),
        Err(e) => return Err(e),
      }
    }
    match live.request(&Request::Reset, &[], self.limits.audio).await? {
      (_, Reply::Ack) => Ok(()),
      _ => Err(HelperError::Crashed),
    }
  }

  /// Stops an idle helper to free its memory. Returns true when it stopped one.
  pub fn reap_if_idle(&self) -> bool {
    if self.last_used.lock().unwrap().elapsed() < self.limits.idle {
      return false;
    }
    match self.live.try_lock() {
      Ok(mut g) if g.is_some() => {
        kill(&mut g);
        tracing::info!("local engine idle; stopped");
        true
      }
      _ => false,
    }
  }
}

pub struct Session {
  guard: OwnedMutexGuard<Option<Live>>,
  helper: Arc<Helper>,
}

impl Session {
  async fn call(
    &mut self,
    req: Request,
    body: &[u8],
    limit: Duration,
    done: fn(&Reply) -> bool,
  ) -> Result<Vec<String>, HelperError> {
    let live = self.guard.as_mut().ok_or(HelperError::Crashed)?;
    let r = live.request(&req, body, limit).await;
    self.helper.touch();
    match r {
      Ok((segments, reply)) if done(&reply) => Ok(segments),
      Ok(_) => {
        kill(&mut self.guard);
        Err(HelperError::Crashed)
      }
      Err(e) => {
        kill(&mut self.guard);
        Err(e)
      }
    }
  }

  pub async fn audio(&mut self, pcm: &[i16]) -> Result<Vec<String>, HelperError> {
    let limit = self.helper.limits.audio;
    self
      .call(Request::Audio, &pcm_to_bytes(pcm), limit, |r| matches!(r, Reply::Ack))
      .await
  }

  pub async fn flush(&mut self) -> Result<Vec<String>, HelperError> {
    let limit = self.helper.limits.flush;
    self
      .call(Request::Flush, &[], limit, |r| matches!(r, Reply::Flushed))
      .await
  }
}

/// Runs the bundled `dictation-asr.exe` hidden, tied to this app by a Job Object.
pub struct ProcessLauncher {
  pub exe: PathBuf,
}

impl ProcessLauncher {
  /// Tauri places `externalBin` next to the app; examples and tests run one folder deeper.
  pub fn find() -> Self {
    let name = if cfg!(windows) {
      "dictation-asr.exe"
    } else {
      "dictation-asr"
    };
    let here = std::env::current_exe()
      .ok()
      .and_then(|p| p.parent().map(|d| d.to_path_buf()))
      .unwrap_or_default();
    let beside = here.join(name);
    let exe = if beside.exists() {
      beside
    } else {
      here.parent().map(|p| p.join(name)).unwrap_or(beside)
    };
    Self { exe }
  }
}

impl Launcher for ProcessLauncher {
  fn launch(&self) -> Result<HelperIo, HelperError> {
    if !self.exe.exists() {
      return Err(HelperError::Missing);
    }
    let mut cmd = tokio::process::Command::new(&self.exe);
    cmd
      .stdin(std::process::Stdio::piped())
      .stdout(std::process::Stdio::piped())
      .stderr(std::process::Stdio::null())
      .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = cmd.spawn().map_err(|_| HelperError::Missing)?;
    #[cfg(windows)]
    job::assign(&child);
    let stdin = child.stdin.take().ok_or(HelperError::Crashed)?;
    let stdout = child.stdout.take().ok_or(HelperError::Crashed)?;
    Ok(HelperIo {
      stdin: Box::new(stdin),
      stdout: Box::new(stdout),
      kill: Box::new(move || {
        let _ = child.start_kill();
      }),
    })
  }
}

#[cfg(windows)]
mod job {
  use std::sync::OnceLock;

  use windows::core::PCWSTR;
  use windows::Win32::Foundation::HANDLE;
  use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
  };

  struct Job(HANDLE);
  // The handle is only passed to AssignProcessToJobObject, which is thread-safe.
  unsafe impl Send for Job {}
  unsafe impl Sync for Job {}

  /// One job for the app's lifetime. When the app exits or crashes, Windows closes the handle and
  /// kills the helper with it.
  fn job() -> Option<&'static Job> {
    static JOB: OnceLock<Option<Job>> = OnceLock::new();
    JOB
      .get_or_init(|| unsafe {
        let h = CreateJobObjectW(None, PCWSTR::null()).ok()?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
          h,
          JobObjectExtendedLimitInformation,
          &info as *const _ as *const std::ffi::c_void,
          std::mem::size_of_val(&info) as u32,
        )
        .ok()?;
        Some(Job(h))
      })
      .as_ref()
  }

  pub fn assign(child: &tokio::process::Child) {
    let (Some(job), Some(raw)) = (job(), child.raw_handle()) else {
      return;
    };
    if unsafe { AssignProcessToJobObject(job.0, HANDLE(raw)) }.is_err() {
      tracing::warn!("could not tie the local engine to the app's lifetime");
    }
  }
}

/// In-process stand-in for dictation-asr, shared by unit and integration tests.
#[cfg(any(test, feature = "test-helpers"))]
pub mod testing {
  use super::*;
  use asr_protocol::ErrorKind;
  use std::sync::atomic::{AtomicUsize, Ordering};

  #[derive(Clone, Copy, PartialEq)]
  pub enum Fake {
    Normal,
    CrashOnAudio,
    HangOnFlush,
    LoadFails,
  }

  pub struct FakeLauncher {
    pub mode: Fake,
    pub launches: Arc<AtomicUsize>,
  }

  impl Launcher for FakeLauncher {
    fn launch(&self) -> Result<HelperIo, HelperError> {
      self.launches.fetch_add(1, Ordering::SeqCst);
      // Each duplex pair is one pipe: the app writes requests into `app_in` and the fake reads
      // them from `helper_in`; the fake writes replies into `helper_out` for `app_out`.
      let (app_in, mut helper_in) = tokio::io::duplex(1 << 20);
      let (mut helper_out, app_out) = tokio::io::duplex(1 << 20);
      let mode = self.mode;
      let task = tokio::spawn(async move {
        let mut dec = Decoder::default();
        let mut buf = vec![0u8; 1 << 16];
        let mut frames = 0usize;
        loop {
          let n = helper_in.read(&mut buf).await.unwrap_or(0);
          if n == 0 {
            return;
          }
          dec.push(&buf[..n]);
          while let Ok(Some((h, _))) = dec.next_frame() {
            let req: Request = decode(&h).unwrap();
            let mut out = Vec::new();
            match req {
              Request::Load { .. } if mode == Fake::LoadFails => out.push(Reply::Error {
                kind: ErrorKind::Load,
                detail: "missing tokens.txt".into(),
              }),
              Request::Load { .. } => out.push(Reply::Loaded { ms: 1 }),
              Request::Audio if mode == Fake::CrashOnAudio => return,
              Request::Audio => {
                frames += 1;
                if frames.is_multiple_of(3) {
                  out.push(Reply::Segment { text: "hello".into() });
                }
                out.push(Reply::Ack);
              }
              Request::Flush if mode == Fake::HangOnFlush => continue,
              Request::Flush => {
                out.push(Reply::Segment { text: "world".into() });
                out.push(Reply::Flushed);
              }
              Request::Reset => out.push(Reply::Ack),
              Request::Ping => out.push(Reply::Pong),
              Request::Shutdown => return,
            }
            for r in out {
              if helper_out.write_all(&encode(&r, &[])).await.is_err() {
                return;
              }
            }
          }
        }
      });
      Ok(HelperIo {
        stdin: Box::new(app_in),
        stdout: Box::new(app_out),
        kill: Box::new(move || task.abort()),
      })
    }
  }

  pub fn spec() -> LoadSpec {
    LoadSpec {
      kind: ModelKind::Transducer,
      files: BTreeMap::new(),
      vad: "v".into(),
      language: String::new(),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::testing::*;
  use super::*;
  use std::sync::atomic::{AtomicUsize, Ordering};

  fn helper(mode: Fake, limits: Limits) -> (Arc<Helper>, Arc<AtomicUsize>) {
    let launches = Arc::new(AtomicUsize::new(0));
    (
      Helper::new(
        Arc::new(FakeLauncher {
          mode,
          launches: launches.clone(),
        }),
        limits,
      ),
      launches,
    )
  }

  fn quick() -> Limits {
    Limits {
      load: Duration::from_secs(2),
      audio: Duration::from_secs(2),
      flush: Duration::from_millis(300),
      idle: Duration::ZERO,
    }
  }

  #[tokio::test]
  async fn session_collects_segments_and_reuses_the_process() {
    let (h, launches) = helper(Fake::Normal, quick());
    let mut s = h.session(&spec()).await.unwrap();
    let mut text = Vec::new();
    for _ in 0..3 {
      text.extend(s.audio(&[0; 1600]).await.unwrap());
    }
    text.extend(s.flush().await.unwrap());
    assert_eq!(text, vec!["hello", "world"]);
    drop(s);
    let _again = h.session(&spec()).await.unwrap();
    assert_eq!(launches.load(Ordering::SeqCst), 1);
  }

  #[tokio::test]
  async fn crash_is_reported_and_next_session_relaunches() {
    let (h, launches) = helper(Fake::CrashOnAudio, quick());
    let mut s = h.session(&spec()).await.unwrap();
    assert_eq!(s.audio(&[0; 160]).await, Err(HelperError::Crashed));
    drop(s);
    let _s = h.session(&spec()).await.unwrap();
    assert_eq!(launches.load(Ordering::SeqCst), 2);
  }

  #[tokio::test]
  async fn hang_trips_the_watchdog() {
    let (h, _) = helper(Fake::HangOnFlush, quick());
    let mut s = h.session(&spec()).await.unwrap();
    assert_eq!(s.flush().await, Err(HelperError::Hang));
  }

  #[tokio::test]
  async fn load_failure_carries_the_reason() {
    let (h, _) = helper(Fake::LoadFails, quick());
    assert_eq!(
      h.session(&spec()).await.err(),
      Some(HelperError::Load("missing tokens.txt".into()))
    );
  }

  #[tokio::test]
  async fn idle_helper_is_reaped() {
    let (h, launches) = helper(Fake::Normal, quick());
    drop(h.session(&spec()).await.unwrap());
    assert!(h.reap_if_idle());
    assert!(!h.reap_if_idle());
    drop(h.session(&spec()).await.unwrap());
    assert_eq!(launches.load(Ordering::SeqCst), 2);
  }
}
