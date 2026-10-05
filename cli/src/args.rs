//! Argument parsing, the small part of it a command line needs: flags
//! with values, switches, and what is left in order.

use std::collections::BTreeMap;

pub struct Args {
    pub positional: Vec<String>,
    values: BTreeMap<String, Vec<String>>,
    switches: Vec<String>,
}

/// Flags that take a value. Everything else that starts with `--` is a
/// switch. `--html` is both: `send --html H` takes the HTML body, `read
/// <id> --html` is a switch; see [`takes_value`].
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
    "idempotency-key",
];

/// Flags that may be given more than once: recipients and attachments
/// collect. Any other flag given twice is refused, so that a second value
/// never silently wins over the first.
const REPEATABLE: &[&str] = &["to", "cc", "bcc", "attach"];

/// Whether a flag takes a value, for the command given so far. Only
/// `send` gives `--html` a value; for every other command it is a switch.
fn takes_value(command: Option<&str>, name: &str) -> bool {
    match (command, name) {
        (Some(c), "html") => c == "send",
        _ => WITH_VALUE.contains(&name),
    }
}

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
            let command = a.positional.first().map(|s| s.as_str());
            if arg == "--" {
                only_positional = true;
                continue;
            }
            let name = arg.trim_start_matches('-');
            let (name, inline) = match name.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (name.to_string(), None),
            };
            if takes_value(command, &name) {
                let value = match inline {
                    Some(v) => v,
                    None => raw.next().ok_or_else(|| format!("--{name} needs a value"))?,
                };
                let list = a.values.entry(name.clone()).or_default();
                if !list.is_empty() && !REPEATABLE.contains(&name.as_str()) {
                    return Err(format!("--{name} is given twice"));
                }
                list.push(value);
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

    /// Every value of a repeatable address flag as one list, the way a
    /// mail header writes it: `--to a --to b` is `a, b`. Empty when the
    /// flag is not given.
    pub fn addresses(&self, name: &str) -> String {
        self.values(name)
            .iter()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
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

    fn parse(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn repeated_recipients_are_all_kept() {
        let a = parse(&[
            "send",
            "--to",
            "nina@example.ch",
            "--to",
            "anna@example.ch, marco@example.ch",
            "--cc=x@example.ch",
            "--cc",
            "y@example.ch",
        ])
        .unwrap();
        assert_eq!(
            a.addresses("to"),
            "nina@example.ch, anna@example.ch, marco@example.ch"
        );
        assert_eq!(a.addresses("cc"), "x@example.ch, y@example.ch");
        assert_eq!(a.addresses("bcc"), "");
    }

    #[test]
    fn html_is_a_value_for_send_and_a_switch_for_read() {
        let a = parse(&["read", "x1", "--html"]).unwrap();
        assert!(a.switch("html"));
        assert_eq!(a.positional, vec!["read", "x1"]);
        let a = parse(&["--json", "read", "--html", "x1"]).unwrap();
        assert!(a.switch("html"));
        assert_eq!(a.positional, vec!["read", "x1"]);
        let a = parse(&["send", "--to", "a@example.ch", "--html", "<p>Hi</p>"]).unwrap();
        assert_eq!(a.value("html"), Some("<p>Hi</p>"));
        assert!(!a.switch("html"));
        assert!(parse(&["read", "x1", "--html=1"]).is_err());
    }

    #[test]
    fn a_single_value_flag_given_twice_is_refused() {
        let err = parse(&["send", "--subject", "a", "--subject", "b"])
            .err()
            .unwrap();
        assert_eq!(err, "--subject is given twice");
    }
}
