//! The `emx` binary against a small server in the test itself, for what
//! only shows when the program runs as a whole: how it writes its output.

mod common;

use common::{emx, server};
use std::io::{BufRead, BufReader, Read};
use std::process::Stdio;

/// Runs `emx`, reads the first line of its output and closes the pipe, as
/// `| head -1` does. Answers the exit status and standard error.
fn run_and_close_early(base: &str, args: &[&str]) -> (std::process::ExitStatus, String) {
    let mut child = emx(base)
        .args(args)
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

fn many_messages(path: &str) -> (&'static str, String) {
    if path.contains("/mailboxes") {
        let body =
            r#"{"mailboxes":[{"id":"m1","name":"INBOX","role":"inbox","total":200,"unseen":0,"modseq":9}]}"#;
        return ("application/json", body.into());
    }
    let one = r#"{"id":"x","mailboxId":"m1","threadId":"t","receivedAt":"2026-09-24T07:41:12Z","subject":"Offerte fuer das Projekt, mit einem langen Betreff","from":{"name":"Anna","address":"anna@example.ch"},"to":["samuel@elchi.dev"],"snippet":"Danke fuer die Offerte, wir sind dabei und melden uns bald wieder","keywords":["$seen"],"size":48213,"modseq":8}"#;
    let list = vec![one; 200].join(",");
    (
        "application/json",
        format!(r#"{{"messages":[{list}],"cursor":""}}"#),
    )
}

fn big_raw(_: &str) -> (&'static str, String) {
    let raw = "Subject: x\r\n\r\n".to_string() + &"line of a long message\r\n".repeat(50_000);
    ("message/rfc822", raw)
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
