//! Runs the real helper binary and checks the protocol without loading a model.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

use asr_protocol::{decode, encode, Decoder, ErrorKind, ModelKind, Reply, Request};

fn ask(child: &mut std::process::Child, dec: &mut Decoder, req: &Request, body: &[u8]) -> Reply {
  child.stdin.as_mut().unwrap().write_all(&encode(req, body)).unwrap();
  child.stdin.as_mut().unwrap().flush().unwrap();
  let out = child.stdout.as_mut().unwrap();
  let mut buf = [0u8; 4096];
  loop {
    if let Some((h, _)) = dec.next_frame().unwrap() {
      return decode(&h).unwrap();
    }
    let n = out.read(&mut buf).unwrap();
    assert!(n > 0, "helper closed stdout");
    dec.push(&buf[..n]);
  }
}

#[test]
fn ping_errors_and_shutdown() {
  let mut child = Command::new(env!("CARGO_BIN_EXE_dictation-asr"))
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .spawn()
    .unwrap();
  let mut dec = Decoder::default();
  assert_eq!(ask(&mut child, &mut dec, &Request::Ping, &[]), Reply::Pong);
  match ask(&mut child, &mut dec, &Request::Audio, &[0, 0]) {
    Reply::Error { kind, .. } => assert_eq!(kind, ErrorKind::NotLoaded),
    other => panic!("unexpected {other:?}"),
  }
  let load = Request::Load {
    kind: ModelKind::Dolphin,
    files: [
      ("model".to_string(), r"Z:\no\such\model.int8.onnx".to_string()),
      ("tokens".to_string(), r"Z:\no\such\tokens.txt".to_string()),
    ]
    .into(),
    vad: r"Z:\no\such\vad.onnx".into(),
    language: String::new(),
    threads: 1,
  };
  match ask(&mut child, &mut dec, &load, &[]) {
    Reply::Error { kind, detail } => {
      assert_eq!(kind, ErrorKind::Load);
      assert!(detail.contains("missing"), "{detail}");
    }
    other => panic!("unexpected {other:?}"),
  }
  assert_eq!(ask(&mut child, &mut dec, &Request::Reset, &[]), Reply::Ack);
  child
    .stdin
    .as_mut()
    .unwrap()
    .write_all(&encode(&Request::Shutdown, &[]))
    .unwrap();
  assert!(child.wait().unwrap().success());
}
