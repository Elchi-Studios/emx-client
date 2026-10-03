//! A small server for running the `emx` binary against, and the command
//! to run it with.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};

/// Starts a server that answers every request with `answer(path)`, a
/// content type and a body, and returns its base URL. The path includes
/// the query.
pub fn server(answer: fn(&str) -> (&'static str, String)) -> String {
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
            let (kind, body) = answer(&path);
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    format!("http://127.0.0.1:{}", addr.port())
}

/// `emx` with a token and the server's URL in the environment, and a
/// home folder without a config file in it.
pub fn emx(base: &str) -> Command {
    let home = std::env::temp_dir().join(format!("emx-cli-test-{}", std::process::id()));
    let mut c = Command::new(env!("CARGO_BIN_EXE_emx"));
    c.env("EMX_TOKEN", "emx_test")
        .env("EMX_BASE_URL", base)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &home)
        .env("APPDATA", &home)
        .stdin(Stdio::null());
    c
}
