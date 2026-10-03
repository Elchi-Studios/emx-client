use crate::error::Error;
use std::io::{BufRead, BufReader, Read};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

/// How long the stream may stay silent before it counts as broken. The
/// service sends a keepalive every 25 seconds, so a minute without a
/// line means at least two were lost: the connection is gone, even if
/// the system has not noticed, as after a laptop wakes up.
const IDLE: Duration = Duration::from_secs(60);

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
}

impl Events {
    pub(crate) fn new(reader: Box<dyn Read + Send>) -> Events {
        Events::with_idle(reader, IDLE)
    }

    /// Reads the stream on a thread of its own and hands the lines over,
    /// so that waiting for the next one can give up after `idle`. A read
    /// on a connection that died without a word blocks for as long as
    /// the request's own timeout allows; the thread ends then, or as
    /// soon as the next line finds nobody listening.
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
        Events { lines, idle, done }
    }
}

impl Iterator for Events {
    type Item = Result<Event, Error>;

    /// The next event, blocking until one arrives. `None` once the service
    /// ended the stream; an error, and then `None`, when the connection
    /// broke or stayed silent past the keepalives.
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let mut name = String::new();
        let mut data = String::new();
        loop {
            let line = match self.lines.recv_timeout(self.idle) {
                Ok(Ok(line)) => line,
                Ok(Err(e)) => {
                    self.done = true;
                    return Some(Err(Error::Transport(e.to_string())));
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.done = true;
                    return Some(Err(Error::Transport(format!(
                        "the event stream was silent for {} seconds",
                        self.idle.as_secs()
                    ))));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.done = true;
                    return None;
                }
            };
            let l = line.trim_end_matches(['\r', '\n']);
            if l.is_empty() {
                if name.is_empty() && data.is_empty() {
                    continue;
                }
                return Some(Ok(parse(&name, &data)));
            }
            if let Some(rest) = l.strip_prefix("event:") {
                name = rest.trim().to_string();
            } else if let Some(rest) = l.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
            }
            // Comments (keepalives) and ids are passed over.
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
        let mut ev = Events::new(Box::new(std::io::Cursor::new(feed.as_bytes().to_vec())));
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
