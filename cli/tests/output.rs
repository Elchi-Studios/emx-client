//! The `emx` binary against a small server in the test itself, for what
//! only shows when the program runs as a whole: how it writes its output.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};

/// Starts a server that answers every request with `answer(path)` as
/// JSON, and returns its base URL.
fn server(answer: fn(&str) -> String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim_end().is_empty() {
                    break;
                }
            }
            let body = answer(&path);
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    format!("http://127.0.0.1:{}", addr.port())
}

/// Runs `emx` with the token and base URL in the environment and no
/// config file, reads the first line of its output and closes the pipe,
/// as `| head -1` does. Answers the exit status and standard error.
fn run_and_close_early(base: &str, args: &[&str]) -> (std::process::ExitStatus, String) {
    let home = std::env::temp_dir().join(format!("emx-cli-test-{}", std::process::id()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_emx"))
        .args(args)
        .env("EMX_TOKEN", "emx_test")
        .env("EMX_BASE_URL", base)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &home)
        .env("APPDATA", &home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    out.read_line(&mut first).unwrap();
    assert!(!first.is_empty(), "no output at all");
    drop(out);
    let mut err = String::new();
    child.stderr.take().unwrap().read_to_string(&mut err).unwrap();
    (child.wait().unwrap(), err)
}

fn many_messages(path: &str) -> String {
    if path.contains("/mailboxes") {
        return r#"{"mailboxes":[{"id":"m1","name":"INBOX","role":"inbox","total":200,"unseen":0,"modseq":9}]}"#.into();
    }
    let one = r#"{"id":"x","mailboxId":"m1","threadId":"t","receivedAt":"2026-09-24T07:41:12Z","subject":"Offerte fuer das Projekt, mit einem langen Betreff","from":{"name":"Anna","address":"anna@example.ch"},"to":["samuel@elchi.dev"],"snippet":"Danke fuer die Offerte, wir sind dabei und melden uns bald wieder","keywords":["$seen"],"size":48213,"modseq":8}"#;
    let list = vec![one; 200].join(",");
    format!(r#"{{"messages":[{list}],"cursor":""}}"#)
}

fn big_raw(_: &str) -> String {
    "Subject: x\r\n\r\n".to_string() + &"line of a long message\r\n".repeat(50_000)
}

#[test]
fn a_closed_pipe_ends_list_quietly() {
    let base = server(many_messages);
    let (status, err) = run_and_close_early(&base, &["--json", "list", "--limit", "200"]);
    assert!(status.success(), "{status}: {err}");
    assert!(!err.contains("panicked"), "{err}");
}

#[test]
fn a_closed_pipe_ends_raw_output_quietly() {
    let base = server(big_raw);
    let (status, err) = run_and_close_early(&base, &["read", "x", "--raw"]);
    assert!(status.success(), "{status}: {err}");
    assert!(!err.contains("panicked"), "{err}");
}
