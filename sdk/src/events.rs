use crate::error::Error;
use std::io::{BufRead, BufReader, Read};

/// One event from the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Something changed; ask for changes since the modseq you had.
    Change { modseq: i64 },
    /// An event of a kind this version does not know, with its data.
    Other { name: String, data: String },
}

/// The live stream of an account: server-sent events, one `change` for
/// every modseq step, a keepalive comment every 25 seconds. The service
/// ends a stream after an hour; a client opens a new one, then asks for
/// changes since the last modseq it handled, so nothing in between is
/// missed.
///
/// ```no_run
/// # let emx = emx_sdk::Client::new("emx_x")?;
/// let mut modseq = emx.mailboxes("me")?.iter().map(|m| m.modseq).max().unwrap_or(0);
/// loop {
///     for event in emx.events("me")? {
///         if let emx_sdk::Event::Change { .. } = event? {
///             let changes = emx.changes("me", modseq)?;
///             modseq = changes.modseq;
///             // ...
///         }
///     }
/// }
/// # #[allow(unreachable_code)]
/// # Ok::<(), emx_sdk::Error>(())
/// ```
pub struct Events {
    reader: BufReader<Box<dyn Read + Send>>,
    done: bool,
}

impl Events {
    pub(crate) fn new(reader: Box<dyn Read + Send>) -> Events {
        Events {
            reader: BufReader::new(reader),
            done: false,
        }
    }
}

impl Iterator for Events {
    type Item = Result<Event, Error>;

    /// The next event, blocking until one arrives. `None` once the service
    /// ended the stream.
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let mut name = String::new();
        let mut data = String::new();
        let mut line = String::new();
        loop {
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) => {
                    self.done = true;
                    return None;
                }
                Ok(_) => {}
                Err(e) => {
                    self.done = true;
                    return Some(Err(Error::Transport(e.to_string())));
                }
            }
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
}
