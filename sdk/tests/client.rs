//! The client against a small server in the test itself, which answers
//! the way the service documents and records what it was asked.

use emx_sdk::{Client, Draft, Error, ME};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

struct Seen {
    method: String,
    path: String,
    auth: String,
    idempotency_key: String,
    body: String,
}

/// Starts a server that answers each request with the next scripted
/// (status, body) pair and remembers what it saw. A 429 says
/// `Retry-After: 1`.
fn server(answers: Vec<(u16, &'static str)>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    server_with(
        answers
            .into_iter()
            .map(|(status, body)| {
                (
                    status,
                    body,
                    if status == 429 { "Retry-After: 1\r\n" } else { "" },
                )
            })
            .collect(),
    )
}

/// [`server`] with the extra header lines of each answer given.
fn server_with(answers: Vec<(u16, &'static str, &'static str)>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        let mut answers = answers.into_iter();
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap_or("").to_string();
            let path = parts.next().unwrap_or("").to_string();
            let mut auth = String::new();
            let mut idempotency_key = String::new();
            let mut length = 0usize;
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                let l = line.trim_end();
                if l.is_empty() {
                    break;
                }
                let lower = l.to_ascii_lowercase();
                if let Some(v) = lower.strip_prefix("authorization:") {
                    auth = v.trim().to_string();
                }
                if lower.starts_with("idempotency-key:") {
                    idempotency_key = l["idempotency-key:".len()..].trim().to_string();
                }
                if let Some(v) = lower.strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; length];
            if length > 0 {
                reader.read_exact(&mut body).unwrap();
            }
            log.lock().unwrap().push(Seen {
                method,
                path,
                auth,
                idempotency_key,
                body: String::from_utf8_lossy(&body).into(),
            });
            let Some((status, answer, extra)) = answers.next() else {
                break;
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{answer}",
                answer.len()
            );
        }
    });
    (format!("http://127.0.0.1:{}", addr.port()), seen)
}

const ME_JSON: &str = r#"{"id":"u1","name":"Samuel Krauss","role":"owner","tenant":{"id":"t1","name":"Elchi Studios","plan":"team"},
  "accounts":[{"id":"u1","kind":"user","name":"Samuel Krauss","address":"samuel@elchi.dev","rights":{"own":true,"read":true,"write":true,"delete":true,"sendAs":true}}],
  "sendFrom":[{"address":"samuel@elchi.dev","name":"Samuel Krauss","mode":"own","primary":true}],
  "limits":{"maxMessageBytes":52428800,"dailyRecipients":1000},"prefs":{"screener":true},"newField":1}"#;

#[test]
fn me_and_mailboxes() {
    let (base, seen) = server(vec![
        (200, ME_JSON),
        (
            200,
            r#"{"mailboxes":[{"id":"m1","name":"INBOX","role":"inbox","total":120,"unseen":4,"bytes":8123456,"modseq":812}]}"#,
        ),
    ]);
    let emx = Client::with_base_url(&base, "emx_test")
        .unwrap()
        .user_agent("test/1");
    let me = emx.me().unwrap();
    assert_eq!(me.name, "Samuel Krauss");
    assert_eq!(me.accounts[0].address, "samuel@elchi.dev");
    assert_eq!(me.prefs["screener"], true);
    let boxes = emx.mailboxes(ME).unwrap();
    assert_eq!(boxes[0].role, "inbox");
    assert_eq!(boxes[0].unseen, 4);
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].path, "/api/me");
    assert_eq!(seen[0].auth, "bearer emx_test");
    assert_eq!(seen[1].path, "/api/accounts/me/mailboxes");
}

#[test]
fn messages_page_and_writes() {
    let (base, seen) = server(vec![
        (
            200,
            r#"{"messages":[{"id":"x1","mailboxId":"m1","threadId":"t","receivedAt":"2026-09-24T07:41:12Z","subject":"Offerte","from":{"name":"Anna","address":"anna@example.ch"},"to":["samuel@elchi.dev"],"snippet":"Danke","hasAttachments":true,"keywords":["$seen"],"size":48213,"modseq":811}],"cursor":"next"}"#,
        ),
        (200, r#"{"changed":["x1"]}"#),
        (200, r#"{"changed":["x1"]}"#),
        (200, r#"{"sent":true,"recipients":2}"#),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let page = emx.messages(ME, "m1", 500, "").unwrap();
    assert_eq!(page.cursor, "next");
    assert!(page.items[0].is_seen());
    assert_eq!(page.items[0].from.to_string(), "Anna <anna@example.ch>");
    assert_eq!(emx.mark_seen(ME, &["x1"], false).unwrap(), vec!["x1"]);
    assert_eq!(emx.r#move(ME, &["x1"], "m2").unwrap(), vec!["x1"]);
    let sent = emx
        .send_mail(&Draft {
            from: "samuel@elchi.dev".into(),
            to: "anna@example.ch, marco@example.ch".into(),
            subject: "Re: Offerte".into(),
            text: "Gerne.".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(sent.recipients, 2);
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].path, "/api/accounts/me/messages?mailbox=m1&limit=200");
    assert_eq!(seen[1].method, "POST");
    assert_eq!(seen[1].body, r#"{"add":[],"ids":["x1"],"remove":["$seen"]}"#);
    assert_eq!(seen[3].path, "/api/send");
    assert!(seen[3]
        .body
        .contains(r#""to":"anna@example.ch, marco@example.ch""#));
    assert!(
        !seen[3].body.contains("html"),
        "empty fields are left out: {}",
        seen[3].body
    );
}

#[test]
fn refusals_become_errors() {
    let (base, _) = server(vec![
        (
            401,
            r#"{"error":{"code":"unauthorized","message":"the token is not valid","requestId":"r1"}}"#,
        ),
        (404, "not json at all"),
    ]);
    let emx = Client::with_base_url(&base, "emx_bad").unwrap();
    let err = emx.me().unwrap_err();
    assert!(err.is_unauthorized());
    assert_eq!(err.code(), Some("unauthorized"));
    assert_eq!(err.to_string(), "the token is not valid (unauthorized, HTTP 401)");
    let err = emx.mailboxes(ME).unwrap_err();
    assert!(err.is_not_found());
    assert_eq!(err.code(), Some("not_found"));
}

#[test]
fn rate_limit_is_retried_once() {
    let (base, seen) = server(vec![
        (429, r#"{"error":{"code":"rate_limited","message":"slow down"}}"#),
        (200, ME_JSON),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    emx.me().unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[test]
fn bad_configuration_is_refused_early() {
    assert!(matches!(Client::new(""), Err(Error::Config(_))));
    assert!(matches!(Client::new("emx_a b"), Err(Error::Config(_))));
    assert!(matches!(
        Client::with_base_url("http://mail.example.ch", "emx_x"),
        Err(Error::Config(_))
    ));
    assert!(Client::with_base_url("https://mail.example.ch/", "emx_x").is_ok());
}

#[test]
fn segments_are_encoded() {
    let (base, seen) = server(vec![(200, r#"{"contacts":[]}"#), (200, r#"{"ok":true}"#)]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    emx.contacts("me", "approved").unwrap();
    emx.remove_contact("me", "a b@example.ch").unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].path, "/api/accounts/me/contacts?state=approved");
    assert_eq!(seen[1].method, "DELETE");
    assert_eq!(seen[1].path, "/api/accounts/me/contacts/a%20b%40example.ch");
}

const SENT: &str = r#"{"sent":true,"recipients":1}"#;

fn draft() -> Draft {
    Draft {
        from: "samuel@elchi.dev".into(),
        to: "anna@example.ch".into(),
        subject: "Offerte".into(),
        text: "Gerne.".into(),
        ..Default::default()
    }
}

#[test]
fn a_send_carries_one_idempotency_key_over_its_attempts() {
    let (base, seen) = server(vec![
        (
            503,
            r#"{"error":{"code":"try_later","message":"try again later"}}"#,
        ),
        (200, SENT),
        (200, SENT),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    emx.send_mail(&draft()).unwrap();
    emx.send_mail(&draft()).unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(seen[0].idempotency_key.len() > 20, "{}", seen[0].idempotency_key);
    assert_eq!(
        seen[0].idempotency_key, seen[1].idempotency_key,
        "a retry keeps the key"
    );
    assert_ne!(
        seen[1].idempotency_key, seen[2].idempotency_key,
        "a new send gets a new key"
    );
}

#[test]
fn a_send_with_a_given_key_uses_it() {
    let (base, seen) = server(vec![(200, SENT)]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    emx.send_mail_with_key(&draft(), "invoice-2026-117").unwrap();
    assert_eq!(seen.lock().unwrap()[0].idempotency_key, "invoice-2026-117");
}

#[test]
fn lasting_refusals_are_not_retried() {
    let (base, seen) = server_with(vec![
        (
            429,
            r#"{"error":{"code":"daily_limit","message":"the daily limit is reached"}}"#,
            "",
        ),
        (
            503,
            r#"{"error":{"code":"unavailable","message":"sending is not configured"}}"#,
            "",
        ),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let err = emx.send_mail(&draft()).unwrap_err();
    assert_eq!(err.code(), Some("daily_limit"));
    assert!(!err.is_retryable());
    let err = emx.send_mail(&draft()).unwrap_err();
    assert_eq!(err.code(), Some("unavailable"));
    assert_eq!(seen.lock().unwrap().len(), 2, "one request each");
}

#[test]
fn a_long_pause_is_left_to_the_caller() {
    let (base, seen) = server_with(vec![(
        429,
        r#"{"error":{"code":"rate_limited","message":"at most 600 requests per minute per token"}}"#,
        "Retry-After: 60\r\n",
    )]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let started = std::time::Instant::now();
    let err = emx.me().unwrap_err();
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(err.is_retryable());
    assert!(matches!(
        err,
        Error::Api {
            retry_after: Some(60),
            ..
        }
    ));
    assert_eq!(seen.lock().unwrap().len(), 1);
}
