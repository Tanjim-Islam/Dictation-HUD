//! dictation-asr: local speech recognition for Dictation HUD, spoken over stdin/stdout frames
//! (see the `asr-protocol` crate). One model at a time. Exits when stdin closes.

mod engine;

use std::io::{Read, Write};
use std::time::Instant;

use asr_protocol::{bytes_to_f32, decode, encode, Decoder, ErrorKind, Reply, Request};

fn send(out: &mut impl Write, reply: &Reply) -> std::io::Result<()> {
  out.write_all(&encode(reply, &[]))?;
  out.flush()
}

fn handle(req: Request, body: &[u8], engine: &mut Option<engine::Engine>, out: &mut impl Write) -> std::io::Result<()> {
  match req {
    Request::Load {
      kind,
      files,
      vad,
      language,
      threads,
    } => {
      // Free the previous model before loading the next one.
      engine.take();
      let started = Instant::now();
      match engine::Engine::load(kind, &files, &vad, &language, threads) {
        Ok(e) => {
          *engine = Some(e);
          send(
            out,
            &Reply::Loaded {
              ms: started.elapsed().as_millis() as u64,
            },
          )
        }
        Err(e) => send(
          out,
          &Reply::Error {
            kind: e.kind,
            detail: e.detail,
          },
        ),
      }
    }
    Request::Audio | Request::Flush => {
      let flush = matches!(req, Request::Flush);
      let Some(e) = engine.as_mut() else {
        return send(
          out,
          &Reply::Error {
            kind: ErrorKind::NotLoaded,
            detail: "no model loaded".into(),
          },
        );
      };
      let segments = if flush {
        e.flush()
      } else {
        e.accept(&bytes_to_f32(body))
      };
      for text in segments {
        send(out, &Reply::Segment { text })?;
      }
      send(out, if flush { &Reply::Flushed } else { &Reply::Ack })
    }
    Request::Reset => {
      if let Some(e) = engine.as_mut() {
        e.reset();
      }
      send(out, &Reply::Ack)
    }
    Request::Ping => send(out, &Reply::Pong),
    Request::Shutdown => std::process::exit(0),
  }
}

fn main() {
  let mut stdin = std::io::stdin().lock();
  let mut stdout = std::io::stdout().lock();
  let mut decoder = Decoder::default();
  let mut engine: Option<engine::Engine> = None;
  let mut buf = vec![0u8; 64 * 1024];
  loop {
    let n = match stdin.read(&mut buf) {
      Ok(0) | Err(_) => return,
      Ok(n) => n,
    };
    decoder.push(&buf[..n]);
    loop {
      let (header, body) = match decoder.next_frame() {
        Ok(Some(f)) => f,
        Ok(None) => break,
        Err(_) => return,
      };
      let reply = match decode::<Request>(&header) {
        Ok(req) => handle(req, &body, &mut engine, &mut stdout),
        Err(_) => send(
          &mut stdout,
          &Reply::Error {
            kind: ErrorKind::BadRequest,
            detail: "bad header".into(),
          },
        ),
      };
      if reply.is_err() {
        return;
      }
    }
  }
}
