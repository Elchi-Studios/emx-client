//! Argument parsing, the small part of it a command line needs: flags
//! with values, switches, and what is left in order.

use std::collections::BTreeMap;

pub struct Args {
    pub positional: Vec<String>,
    values: BTreeMap<String, Vec<String>>,
    switches: Vec<String>,
}

/// Flags that take a value. Everything else that starts with `--` is a
/// switch.
const WITH_VALUE: &[&str] = &[
    "account",
    "mailbox",
    "limit",
    "from",
    "to",
    "cc",
    "bcc",
    "subject",
    "text",
    "html",
    "body-file",
    "attach",
    "reply-to",
    "in-reply-to",
    "state",
    "events",
    "description",
    "until",
    "exec",
    "data",
    "base-url",
    "part",
    "out",
    "name",
    "cursor",
];

impl Args {
    pub fn parse(raw: impl Iterator<Item = String>) -> Result<Args, String> {
        let mut a = Args {
            positional: Vec::new(),
            values: BTreeMap::new(),
            switches: Vec::new(),
        };
        let mut raw = raw.peekable();
        let mut only_positional = false;
        while let Some(arg) = raw.next() {
            if only_positional || !arg.starts_with('-') || arg == "-" {
                a.positional.push(arg);
                continue;
            }
            if arg == "--" {
                only_positional = true;
                continue;
            }
            let name = arg.trim_start_matches('-');
            let (name, inline) = match name.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (name.to_string(), None),
            };
            if WITH_VALUE.contains(&name.as_str()) {
                let value = match inline {
                    Some(v) => v,
                    None => raw.next().ok_or_else(|| format!("--{name} needs a value"))?,
                };
                a.values.entry(name).or_default().push(value);
            } else {
                if inline.is_some() {
                    return Err(format!("--{name} takes no value"));
                }
                a.switches.push(name);
            }
        }
        Ok(a)
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).and_then(|v| v.last()).map(|s| s.as_str())
    }

    pub fn values(&self, name: &str) -> &[String] {
        self.values.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn switch(&self, name: &str) -> bool {
        self.switches.iter().any(|s| s == name)
    }

    pub fn number(&self, name: &str, default: u32) -> Result<u32, String> {
        match self.value(name) {
            None => Ok(default),
            Some(v) => v.parse().map_err(|_| format!("--{name} must be a number")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mixed_arguments() {
        let a = Args::parse(
            [
                "list",
                "--mailbox",
                "Inbox",
                "--json",
                "--limit=5",
                "--attach",
                "a",
                "--attach",
                "b",
                "--",
                "--x",
            ]
            .iter()
            .map(|s| s.to_string()),
        )
        .unwrap();
        assert_eq!(a.positional, vec!["list", "--x"]);
        assert_eq!(a.value("mailbox"), Some("Inbox"));
        assert!(a.switch("json"));
        assert_eq!(a.number("limit", 50).unwrap(), 5);
        assert_eq!(a.values("attach"), &["a", "b"]);
        assert!(Args::parse(["--mailbox"].iter().map(|s| s.to_string())).is_err());
        assert!(Args::parse(["--json=1"].iter().map(|s| s.to_string())).is_err());
    }
}
