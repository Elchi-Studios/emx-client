//! Drafts kept on the server, as the web client keeps them: saved as the
//! person types, so a message begun here can be picked up on another
//! device. A draft belongs to an account at a version; a save made on an
//! older version is refused with `409`, and the window is told what is
//! there now so it can ask the person.

use emx_sdk::{Client, Error};
use serde::Serialize;
use serde_json::{json, Value};

/// A draft as the server holds it.
#[derive(Serialize, Debug, PartialEq)]
pub struct Saved {
    pub id: String,
    pub account: String,
    pub version: i64,
    pub body: Value,
}

/// What came of a save.
#[derive(Serialize, Debug, PartialEq)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Outcome {
    /// Saved, at the version given.
    Saved { draft: Saved },
    /// Somebody saved a newer version meanwhile; this is it.
    Conflict { draft: Saved },
    /// The draft was sent from somewhere else.
    Sent,
    /// The mailbox is sealed: the server refuses a draft in clear, and
    /// the app has no key to seal it, so nothing is saved anywhere.
    Sealed,
}

/// An ID or an account is put into a path: only what the service itself
/// makes is accepted, so nothing can climb out of it.
fn plain(what: &str, s: &str) -> Result<(), Error> {
    if !s.is_empty()
        && s.len() <= 80
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        Ok(())
    } else {
        Err(Error::Config(format!("not a valid {what}")))
    }
}

fn path(account: &str, id: &str) -> Result<String, Error> {
    plain("account", account)?;
    plain("draft ID", id)?;
    Ok(format!("/api/accounts/{account}/drafts/{id}"))
}

fn read(v: &Value) -> Option<Saved> {
    let d = v.get("draft")?;
    Some(Saved {
        id: d.get("id")?.as_str()?.to_string(),
        account: d.get("account")?.as_str()?.to_string(),
        version: d.get("version")?.as_i64()?,
        body: d.get("body").cloned().unwrap_or(Value::Null),
    })
}

fn unexpected() -> Error {
    Error::Decode("the draft is not in the answer".into())
}

/// Saves `body` as draft `id` in `account`, on `version`; 0 makes the
/// draft under the ID given. `account` may be `me` the first time: the
/// answer says where the server put it.
pub fn save(c: &Client, account: &str, id: &str, version: i64, body: &Value) -> Result<Outcome, Error> {
    let p = path(account, id)?;
    match c.call_json("PUT", &p, &json!({"version": version, "body": body})) {
        Ok(v) => Ok(Outcome::Saved {
            draft: read(&v).ok_or_else(unexpected)?,
        }),
        Err(Error::Api {
            status: 409, code, ..
        }) if code == "sealed" => Ok(Outcome::Sealed),
        Err(Error::Api {
            status: 409, code, ..
        }) => {
            // The refusal carries the newer draft, which the SDK's error
            // does not keep: ask for it.
            let v = c.get_json(&p, &[])?;
            if code == "sent" || v.pointer("/draft/sentAt").is_some() {
                return Ok(Outcome::Sent);
            }
            Ok(Outcome::Conflict {
                draft: read(&v).ok_or_else(unexpected)?,
            })
        }
        Err(e) => Err(e),
    }
}

/// The person's own unsent drafts, newest first, to be picked up again.
/// Sent ones and those of a sealed mailbox (their body is ciphertext the
/// app cannot open) are left out.
pub fn list(c: &Client) -> Result<Vec<Saved>, Error> {
    let v = c.get_json("/api/accounts/me/drafts", &[])?;
    Ok(v.get("drafts")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|d| d.get("sentAt").is_none_or(Value::is_null))
                .filter(|d| d.pointer("/body/sealed").is_none())
                .filter_map(|d| read(&json!({ "draft": d })))
                .collect()
        })
        .unwrap_or_default())
}

/// Removes a draft, at `version` when that is not 0. One that is gone
/// already is nothing to report.
pub fn delete(c: &Client, account: &str, id: &str, version: i64) -> Result<(), Error> {
    let mut p = path(account, id)?;
    if version > 0 {
        p.push_str(&format!("?version={version}"));
    }
    match c.call_json("DELETE", &p, &Value::Null) {
        Ok(_) => Ok(()),
        Err(e) if e.is_not_found() => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::sync::mpsc::Receiver;

    /// A server that answers each request with the next scripted
    /// (status, body) and reports "METHOD path body" of each.
    fn server(answers: Vec<(u16, &'static str)>) -> (String, Receiver<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut answers = answers.into_iter();
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let (mut length, mut line) = (0usize, String::new());
                while reader.read_line(&mut line).unwrap_or(0) > 0 && line != "\r\n" {
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                    line.clear();
                }
                let mut body = vec![0u8; length];
                let _ = reader.read_exact(&mut body);
                let mut parts = first.split_whitespace();
                let (m, p) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                let _ = tx.send(format!("{m} {p} {}", String::from_utf8_lossy(&body)));
                let (status, text) = answers.next().unwrap_or((500, "{}"));
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
            }
        });
        (base, rx)
    }

    const V1: &str = r#"{"draft":{"id":"d-1","account":"acc9","version":1,"body":{"subject":"Hi"}}}"#;

    #[test]
    fn a_draft_is_saved_on_the_server_at_its_version() {
        let (base, seen) = server(vec![(200, V1)]);
        let c = Client::with_base_url(&base, "emx_test").unwrap();
        let body = json!({"from": "a@example.ch", "to": "b@example.ch", "subject": "Hi", "text": "Hello"});
        let out = save(&c, "me", "d-1", 0, &body).unwrap();
        let Outcome::Saved { draft } = out else {
            panic!("not saved: {out:?}")
        };
        assert_eq!((draft.account.as_str(), draft.version), ("acc9", 1));
        let sent = seen.recv().unwrap();
        assert!(sent.starts_with("PUT /api/accounts/me/drafts/d-1 "), "{sent}");
        let sent_body: Value =
            serde_json::from_str(sent.split_once(' ').unwrap().1.split_once(' ').unwrap().1).unwrap();
        assert_eq!(sent_body["version"], 0);
        assert_eq!(sent_body["body"]["text"], "Hello");
    }

    #[test]
    fn a_save_on_an_old_version_answers_with_the_newer_draft() {
        let (base, seen) = server(vec![
            (
                409,
                r#"{"error":{"code":"conflict","message":"changed meanwhile"}}"#,
            ),
            (
                200,
                r#"{"draft":{"id":"d-1","account":"acc9","version":4,"body":{"subject":"Theirs"}}}"#,
            ),
        ]);
        let c = Client::with_base_url(&base, "emx_test").unwrap();
        let out = save(&c, "acc9", "d-1", 3, &json!({"subject": "Mine"})).unwrap();
        let Outcome::Conflict { draft } = out else {
            panic!("not a conflict: {out:?}")
        };
        assert_eq!(draft.version, 4);
        assert_eq!(draft.body["subject"], "Theirs");
        seen.recv().unwrap();
        assert!(seen
            .recv()
            .unwrap()
            .starts_with("GET /api/accounts/acc9/drafts/d-1 "));
    }

    #[test]
    fn a_draft_sent_elsewhere_is_said_to_be_sent() {
        let (base, _) = server(vec![
            (409, r#"{"error":{"code":"sent","message":"already sent"}}"#),
            (
                200,
                r#"{"draft":{"id":"d-1","account":"acc9","version":4,"body":{},"sentAt":"2026-11-27T10:00:00Z"}}"#,
            ),
        ]);
        let c = Client::with_base_url(&base, "emx_test").unwrap();
        assert_eq!(save(&c, "acc9", "d-1", 3, &json!({})).unwrap(), Outcome::Sent);
    }

    #[test]
    fn a_sealed_mailbox_is_told_and_nothing_is_kept() {
        let (base, seen) = server(vec![(
            409,
            r#"{"error":{"code":"sealed","message":"this mailbox is sealed"}}"#,
        )]);
        let c = Client::with_base_url(&base, "emx_test").unwrap();
        assert_eq!(
            save(&c, "me", "d-1", 0, &json!({"text": "x"})).unwrap(),
            Outcome::Sealed
        );
        seen.recv().unwrap();
        // No second request, such as a fetch of the draft, followed.
        assert!(seen.recv_timeout(std::time::Duration::from_millis(200)).is_err());
    }

    #[test]
    fn saved_drafts_are_listed_without_sent_or_sealed_ones() {
        let (base, seen) = server(vec![(
            200,
            r#"{"drafts":[
              {"id":"a","account":"acc9","version":2,"body":{"subject":"One"}},
              {"id":"b","account":"acc9","version":1,"body":{"sealed":"xyz"}},
              {"id":"c","account":"acc9","version":3,"body":{},"sentAt":"2026-11-27T10:00:00Z"}
            ]}"#,
        )]);
        let c = Client::with_base_url(&base, "emx_test").unwrap();
        let got = list(&c).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].id.as_str(), got[0].version), ("a", 2));
        assert!(seen.recv().unwrap().starts_with("GET /api/accounts/me/drafts "));
    }

    #[test]
    fn other_refusals_pass_through() {
        let (base, _) = server(vec![(
            403,
            r#"{"error":{"code":"scope","message":"needs mail:write"}}"#,
        )]);
        let c = Client::with_base_url(&base, "emx_test").unwrap();
        let e = save(&c, "me", "d-1", 0, &json!({})).unwrap_err();
        assert_eq!(e.code(), Some("scope"));
    }

    #[test]
    fn a_delete_names_the_version_and_a_gone_draft_is_fine() {
        let (base, seen) = server(vec![
            (200, r#"{"ok":true}"#),
            (404, r#"{"error":{"code":"not_found","message":"no such draft"}}"#),
        ]);
        let c = Client::with_base_url(&base, "emx_test").unwrap();
        delete(&c, "acc9", "d-1", 3).unwrap();
        assert!(seen
            .recv()
            .unwrap()
            .starts_with("DELETE /api/accounts/acc9/drafts/d-1?version=3 "));
        delete(&c, "acc9", "d-1", 0).unwrap();
        assert!(seen
            .recv()
            .unwrap()
            .starts_with("DELETE /api/accounts/acc9/drafts/d-1 "));
    }

    #[test]
    fn an_id_cannot_leave_its_place_in_the_path() {
        let c = Client::with_base_url("http://127.0.0.1:9", "emx_test").unwrap();
        for bad in ["../x", "a/b", "", "a?b", "a b"] {
            assert!(save(&c, "me", bad, 0, &json!({})).is_err(), "{bad}");
            assert!(save(&c, bad, "d-1", 0, &json!({})).is_err(), "{bad}");
        }
    }
}
