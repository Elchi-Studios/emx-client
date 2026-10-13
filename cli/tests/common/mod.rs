// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A small server for running the `emx` binary against, and the command
//! to run it with.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// Starts a server that answers every request with `answer(path)`, a
/// content type and a body, and returns its base URL. The path includes
/// the query.
#[allow(dead_code)]
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

/// A request as [`scripted`] saw it.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct Seen {
    pub method: String,
    /// With the query.
    pub path: String,
    pub body: String,
}

/// An answer for [`scripted`]: the status, header lines (each ending in
/// `\r\n`, a content type among them) and the body.
#[allow(dead_code)]
pub struct Reply {
    pub status: u16,
    pub headers: String,
    pub body: String,
}

#[allow(dead_code)]
impl Reply {
    pub fn json(status: u16, body: &str) -> Reply {
        Reply {
            status,
            headers: "Content-Type: application/json\r\n".into(),
            body: body.into(),
        }
    }

    pub fn with(mut self, header: &str) -> Reply {
        self.headers.push_str(header);
        self.headers.push_str("\r\n");
        self
    }
}

/// Starts a server that answers every request with `answer(method, path,
/// n)`, `n` counting the requests from 0, and records what it was asked.
#[allow(dead_code)]
pub fn scripted(answer: fn(&str, &str, usize) -> Reply) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for (n, stream) in listener.incoming().enumerate() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap_or("").to_string();
            let path = parts.next().unwrap_or("").to_string();
            let mut length = 0usize;
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim_end().is_empty() {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; length];
            let _ = reader.read_exact(&mut body);
            let r = answer(&method, &path, n);
            log.lock().unwrap().push(Seen {
                method,
                path,
                body: String::from_utf8_lossy(&body).into(),
            });
            let _ = write!(
                stream,
                "HTTP/1.1 {} X\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                r.status,
                r.headers,
                r.body.len(),
                r.body
            );
        }
    });
    (format!("http://127.0.0.1:{}", addr.port()), seen)
}

/// A folder of its own for a test, empty.
#[allow(dead_code)]
pub fn folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("emx-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
