// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::error::Error;
use std::io::{BufRead, BufReader, Read};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};
// The transport traits are outside ureq's semver promise, which is why
// the workspace holds ureq to one minor version.
use ureq::unversioned::resolver::DefaultResolver;
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, DefaultConnector, NextTimeout, Transport,
};
use ureq::Timeout;

/// How long the stream may stay silent before it counts as broken. The
/// service sends a keepalive every 25 seconds, so a minute without a
/// line means at least two were lost: the connection is gone, even if
/// the system has not noticed, as after a laptop wakes up.
pub(crate) const IDLE: Duration = Duration::from_secs(60);

/// The agent for event streams. Its connections give up a read after
/// `idle` without a byte, the same silence after which [`Events`] ends
/// the stream. Without that, the thread reading a connection that died
/// unnoticed would wait in its read, and keep the socket, until the
/// body's limit of over an hour ran out, once for every reconnect.
pub(crate) fn agent(idle: Duration) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .build();
    let connector = DefaultConnector::new().chain(ReadLimit {
        idle,
        bodies_only: false,
    });
    ureq::Agent::with_parts(config, connector, DefaultResolver::default())
}

/// The agent for every other call. A body, a large attachment down or a
/// send with attachments up, may take as long as it takes while bytes
/// keep moving: only `idle` without one, in a read or a write, ends it.
/// A limit for the whole transfer would fail every large attachment on a
/// slow line, and fail it again on each attempt. Connecting and waiting
/// for the answer to begin have limits of their own, set per call.
pub(crate) fn body_idle_agent(config: ureq::config::Config, idle: Duration) -> ureq::Agent {
    let connector = DefaultConnector::new().chain(ReadLimit {
        idle,
        bodies_only: true,
    });
    ureq::Agent::with_parts(config, connector, DefaultResolver::default())
}

/// Wraps each connection, plain or TLS, in [`Limited`].
#[derive(Debug)]
struct ReadLimit {
    idle: Duration,
    bodies_only: bool,
}

impl Connector<Box<dyn Transport>> for ReadLimit {
    type Out = Limited;

    fn connect(
        &self,
        _: &ConnectionDetails,
        chained: Option<Box<dyn Transport>>,
    ) -> Result<Option<Limited>, ureq::Error> {
        Ok(chained.map(|inner| Limited {
            inner,
            idle: self.idle,
            bodies_only: self.bodies_only,
        }))
    }
}

/// A connection whose reads and writes wait at most `idle` each, however
/// long the request's own limits would allow; with `bodies_only`, only
/// while a body is sent or received. A read or write that times out
/// fails, the reader ends, and the connection is closed.
#[derive(Debug)]
struct Limited {
    inner: Box<dyn Transport>,
    idle: Duration,
    bodies_only: bool,
}

impl Limited {
    fn cap(&self, mut timeout: NextTimeout) -> NextTimeout {
        let body = matches!(timeout.reason, Timeout::SendBody | Timeout::RecvBody);
        if (body || !self.bodies_only) && *timeout.after > self.idle {
            timeout.after = self.idle.into();
        }
        timeout
    }
}

impl Transport for Limited {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        let timeout = if self.bodies_only {
            self.cap(timeout)
        } else {
            timeout
        };
        self.inner.transmit_output(amount, timeout)
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let timeout = self.cap(timeout);
        self.inner.await_input(timeout)
    }

    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

/// One event from the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Something changed; ask for changes since the modseq you had.
    Change { modseq: i64 },
    /// An event of a kind this version does not know, with its data.
    Other { name: String, data: String },
}

/// The live stream of an account: server-sent events, one `change` for
/// every modseq step, a keepalive comment every 25 seconds. The first
/// event carries the modseq at the time of connecting. The service ends
/// a stream after an hour, and the iterator ends with an error when the
/// stream has been silent for a minute; either way a client opens a new
/// one, then asks for changes since the last modseq it handled, so
/// nothing in between is missed. A change event whose modseq is not
/// above the one already handled needs no call.
///
/// ```no_run
/// # let emx = emx_sdk::Client::new("emx_x")?;
/// let mut modseq = emx.mailboxes("me")?.iter().map(|m| m.modseq).max().unwrap_or(0);
/// loop {
///     for event in emx.events("me")? {
///         match event {
///             Ok(emx_sdk::Event::Change { modseq: now }) if now > modseq => {
///                 let changes = emx.changes("me", modseq)?;
///                 modseq = changes.modseq;
///                 // ...
///             }
///             Ok(_) => {}
///             // The connection broke; open a new stream.
///             Err(_) => break,
///         }
///     }
/// }
/// # #[allow(unreachable_code)]
/// # Ok::<(), emx_sdk::Error>(())
/// ```
pub struct Events {
    lines: Receiver<std::io::Result<String>>,
    idle: Duration,
    done: bool,
    /// The event read so far, kept across [`Events::next_within`] calls
    /// that end before it is complete.
    name: String,
    data: String,
    /// When the last line came, to tell silence from a short wait.
    heard: Instant,
}

/// What [`Events::next_within`] found.
#[derive(Debug)]
pub enum Next {
    /// The next event.
    Event(Event),
    /// The connection broke or stayed silent past the keepalives; open a
    /// new stream.
    Broken(Error),
    /// The service ended the stream; open a new one.
    Ended,
    /// Nothing within the wait, and the stream is still alive.
    Quiet,
}

impl Events {
    /// Reads the stream on a thread of its own and hands the lines over,
    /// so that waiting for the next one can give up after `idle`. A read
    /// on a connection that died without a word fails after the same
    /// time (see [`agent`]), and the thread ends then, or as soon as the
    /// next line finds nobody listening.
    pub(crate) fn with_idle(reader: Box<dyn Read + Send>, idle: Duration) -> Events {
        let (tx, rx) = mpsc::sync_channel(64);
        let started = std::thread::Builder::new()
            .name("emx-events".into())
            .spawn(move || {
                let mut reader = BufReader::new(reader);
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => return,
                        Ok(_) => {
                            if tx.send(Ok(line)).is_err() {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e));
                            return;
                        }
                    }
                }
            });
        let (lines, done) = match started {
            Ok(_) => (rx, false),
            // No thread, no stream: the iterator ends at once, and the
            // caller opens a new one later as it would after a break.
            Err(_) => (mpsc::sync_channel(0).1, true),
        };
        Events {
            lines,
            idle,
            done,
            name: String::new(),
            data: String::new(),
            heard: Instant::now(),
        }
    }

    /// The next event, waiting at most `wait` for it, so that a program
    /// can look up in between, for instance to stop following the
    /// account at once instead of after the next event or the next
    /// minute of silence. A stream silent past the keepalives is
    /// [`Next::Broken`], however the waits were cut.
    pub fn next_within(&mut self, wait: Duration) -> Next {
        if self.done {
            return Next::Ended;
        }
        let until = Instant::now() + wait;
        loop {
            let silent_until = self.heard + self.idle;
            let now = Instant::now();
            let quiet = until <= silent_until;
            let deadline = if quiet { until } else { silent_until };
            let line = match self.lines.recv_timeout(deadline.saturating_duration_since(now)) {
                Ok(Ok(line)) => line,
                Ok(Err(e)) => {
                    self.done = true;
                    return Next::Broken(Error::Transport(e.to_string()));
                }
                Err(RecvTimeoutError::Timeout) if quiet => return Next::Quiet,
                Err(RecvTimeoutError::Timeout) => {
                    self.done = true;
                    return Next::Broken(Error::Transport(format!(
                        "the event stream was silent for {} seconds",
                        self.idle.as_secs()
                    )));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.done = true;
                    return Next::Ended;
                }
            };
            self.heard = Instant::now();
            let l = line.trim_end_matches(['\r', '\n']);
            if l.is_empty() {
                if self.name.is_empty() && self.data.is_empty() {
                    continue;
                }
                let event = parse(&self.name, &self.data);
                self.name.clear();
                self.data.clear();
                return Next::Event(event);
            }
            if let Some(rest) = l.strip_prefix("event:") {
                self.name = rest.trim().to_string();
            } else if let Some(rest) = l.strip_prefix("data:") {
                if !self.data.is_empty() {
                    self.data.push('\n');
                }
                self.data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
            }
            // Comments (keepalives) and ids are passed over.
        }
    }
}

impl Iterator for Events {
    type Item = Result<Event, Error>;

    /// The next event, blocking until one arrives. `None` once the service
    /// ended the stream; an error, and then `None`, when the connection
    /// broke or stayed silent past the keepalives.
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.next_within(self.idle) {
                Next::Event(e) => return Some(Ok(e)),
                Next::Broken(e) => return Some(Err(e)),
                Next::Ended => return None,
                Next::Quiet => {}
            }
        }
    }
}

fn parse(name: &str, data: &str) -> Event {
    if name == "change" {
        let modseq = serde_json::from_str::<serde_json::Value>(data)
            .ok()
            .and_then(|v| v.get("modseq").and_then(|m| m.as_i64()))
            .unwrap_or(0);
        return Event::Change { modseq };
    }
    Event::Other {
        name: name.to_string(),
        data: data.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_events_and_skips_keepalives() {
        let feed = ": keepalive\n\nevent: change\ndata: {\"modseq\": 812}\n\n: keepalive\n\nevent: hello\ndata: a\ndata: b\n\n";
        let mut ev = Events::with_idle(Box::new(std::io::Cursor::new(feed.as_bytes().to_vec())), IDLE);
        assert_eq!(ev.next().unwrap().unwrap(), Event::Change { modseq: 812 });
        assert_eq!(
            ev.next().unwrap().unwrap(),
            Event::Other {
                name: "hello".into(),
                data: "a\nb".into()
            }
        );
        assert!(ev.next().is_none());
    }

    /// A connection that delivers nothing, the way one does after the
    /// network went away without a word.
    struct Silent;

    impl Read for Silent {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_secs(3600));
            Ok(0)
        }
    }

    #[test]
    fn a_silent_stream_ends_with_an_error() {
        let mut ev = Events::with_idle(Box::new(Silent), Duration::from_millis(200));
        let started = std::time::Instant::now();
        let err = ev.next().unwrap().unwrap_err();
        assert!(matches!(err, Error::Transport(_)), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(ev.next().is_none());
    }

    #[test]
    fn a_short_wait_comes_back_without_ending_the_stream() {
        let mut ev = Events::with_idle(Box::new(Silent), Duration::from_millis(600));
        let started = std::time::Instant::now();
        assert!(matches!(ev.next_within(Duration::from_millis(50)), Next::Quiet));
        assert!(started.elapsed() < Duration::from_millis(500));
        // Short waits add up: the silence still breaks the stream.
        let broken = loop {
            match ev.next_within(Duration::from_millis(50)) {
                Next::Quiet => assert!(started.elapsed() < Duration::from_secs(5)),
                other => break other,
            }
        };
        assert!(matches!(broken, Next::Broken(Error::Transport(_))), "{broken:?}");
        assert!(matches!(ev.next_within(Duration::from_millis(50)), Next::Ended));
    }

    #[test]
    fn an_event_split_across_waits_is_read_whole() {
        struct Halves(Vec<&'static str>);
        impl Read for Halves {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let Some(next) = self.0.pop() else { return Ok(0) };
                std::thread::sleep(Duration::from_millis(150));
                buf[..next.len()].copy_from_slice(next.as_bytes());
                Ok(next.len())
            }
        }
        let feed = vec!["data: {\"modseq\": 5}\n\n", "event: change\n"];
        let mut ev = Events::with_idle(Box::new(Halves(feed)), IDLE);
        let mut quiet = 0;
        loop {
            match ev.next_within(Duration::from_millis(20)) {
                Next::Quiet => quiet += 1,
                Next::Event(e) => {
                    assert_eq!(e, Event::Change { modseq: 5 });
                    break;
                }
                other => panic!("{other:?}"),
            }
        }
        assert!(quiet > 0, "the waits came back in between");
    }

    #[test]
    fn keepalives_keep_a_stream_open() {
        // Keepalives arrive faster than the idle limit, then a change.
        struct Slow(Vec<&'static str>);
        impl Read for Slow {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let Some(next) = self.0.pop() else { return Ok(0) };
                std::thread::sleep(Duration::from_millis(100));
                buf[..next.len()].copy_from_slice(next.as_bytes());
                Ok(next.len())
            }
        }
        let feed = vec![
            "event: change\ndata: {\"modseq\": 9}\n\n",
            ": keepalive\n\n",
            ": keepalive\n\n",
            ": keepalive\n\n",
        ];
        let mut ev = Events::with_idle(Box::new(Slow(feed)), Duration::from_millis(300));
        assert_eq!(ev.next().unwrap().unwrap(), Event::Change { modseq: 9 });
        assert!(ev.next().is_none());
    }
}
