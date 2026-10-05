//! `emx send` and its idempotency key, against a server that records the
//! key it was sent and answers, or drops the connection without a word.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Output, Stdio};
use std::sync::mpsc;

/// Starts a server for one request. It reports the request's
/// `Idempotency-Key`, then sends a successful answer, or, with `answer`
/// false, closes the connection as a timeout on the way would.
fn server(answer: bool) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut key = String::new();
        let mut length = 0usize;
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 && line != "\r\n" {
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("idempotency-key:") {
                key = line["idempotency-key:".len()..].trim().to_string();
            }
            if let Some(v) = lower.strip_prefix("content-length:") {
                length = v.trim().parse().unwrap_or(0);
            }
            line.clear();
        }
        let mut body = vec![0u8; length];
        let _ = reader.read_exact(&mut body);
        let _ = tx.send(key);
        if answer {
            let body = r#"{"sent":true,"recipients":1}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (base, rx)
}

fn send(base: &str, extra: &[&str]) -> Output {
    let home = std::env::temp_dir().join(format!("emx-cli-test-{}", std::process::id()));
    std::process::Command::new(env!("CARGO_BIN_EXE_emx"))
        .env("EMX_TOKEN", "emx_test")
        .env("EMX_BASE_URL", base)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &home)
        .env("APPDATA", &home)
        .args(["send", "--from", "samuel@elchi.dev", "--to", "anna@example.ch"])
        .args(["--subject", "Offerte", "--text", "Gerne."])
        .args(extra)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn a_given_key_is_sent() {
    let (base, key) = server(true);
    let out = send(&base, &["--idempotency-key", "rechnung-2026-117"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(key.recv().unwrap(), "rechnung-2026-117");
}

#[test]
fn without_a_key_one_is_made() {
    let (base, key) = server(true);
    let out = send(&base, &[]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(key.recv().unwrap().len() > 20);
}

/// When the answer is lost, the message may have gone out. The error
/// names the key, so that sending again with it cannot send twice.
#[test]
fn a_lost_answer_names_the_key_to_send_again_with() {
    let (base, key) = server(false);
    let out = send(&base, &[]);
    assert!(!out.status.success());
    let key = key.recv().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(&format!("--idempotency-key {key}")), "{err}");
}

#[test]
fn an_empty_key_is_refused() {
    let (base, _key) = server(true);
    let out = send(&base, &["--idempotency-key", ""]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("idempotency key"), "{err}");
}
