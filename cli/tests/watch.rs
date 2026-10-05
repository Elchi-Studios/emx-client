//! `emx watch` against a scripted account: the first connection announces
//! the modseq the watch started from, then two changes follow, of which
//! the first turns out to have nothing in it for this token.

mod common;

use common::{emx, server};
use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::time::{Duration, Instant};

fn account(path: &str) -> (&'static str, String) {
    let json = "application/json";
    if path.ends_with("/mailboxes") {
        let body = r#"{"mailboxes":[{"id":"m1","name":"INBOX","role":"inbox","modseq":5}]}"#;
        return (json, body.into());
    }
    if path.ends_with("/events") {
        let feed = "event: change\ndata: {\"modseq\": 5}\n\n: keepalive\n\n\
                    event: change\ndata: {\"modseq\": 6}\n\n\
                    event: change\ndata: {\"modseq\": 7}\n\n";
        return ("text/event-stream", feed.into());
    }
    if path.contains("/changes?since=5") {
        return (
            json,
            r#"{"updated":[],"destroyed":[],"modseq":6,"hasMore":false}"#.into(),
        );
    }
    if path.contains("/changes?since=6") {
        return (
            json,
            r#"{"updated":[],"destroyed":["x1"],"modseq":7,"hasMore":false}"#.into(),
        );
    }
    (
        json,
        r#"{"updated":[],"destroyed":[],"modseq":7,"hasMore":false}"#.into(),
    )
}

/// A command for `--exec` that appends what it reads to `file`, one line
/// per run, in the language of the system's shell.
#[cfg(not(windows))]
fn append_stdin_to(file: &std::path::Path) -> String {
    format!("cat >> '{0}'; echo >> '{0}'", file.display())
}

/// cmd has no `cat`; `sort` copies its input, a single line here, and
/// ends it with a line break. The quotes keep a path with spaces whole.
#[cfg(windows)]
fn append_stdin_to(file: &std::path::Path) -> String {
    format!("sort >> \"{}\"", file.display())
}

#[test]
fn watch_reports_real_changes_only() {
    let base = server(account);
    let dir = std::env::temp_dir().join(format!("emx-watch-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ran = dir.join("exec.out");
    let _ = std::fs::remove_file(&ran);
    let exec = append_stdin_to(&ran);
    let mut child = emx(&base)
        .args(["--json", "watch", "--exec", &exec])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    out.read_line(&mut first).unwrap();
    // The stream ends after the scripted events; give the watch time to
    // open the next one, which announces nothing new.
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
    let ran = std::fs::read_to_string(&ran).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(first.contains(r#""destroyed":["x1"]"#), "first batch: {first}");
    assert!(first.contains(r#""modseq":7"#), "first batch: {first}");
    let batches: Vec<&str> = ran.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(batches.len(), 1, "--exec ran for: {batches:?}");
    assert!(batches[0].contains("x1"));
}
