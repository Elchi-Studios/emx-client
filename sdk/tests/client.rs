//! The client against a small server in the test itself, which answers
//! the way the service documents and records what it was asked. Every
//! call the command line and the desktop app make is here; the answers
//! follow the API reference and the service's own handlers, error codes
//! included.

use emx_sdk::{Client, Draft, Error, Event, ME};
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
            // JSON unless the answer names its own type.
            let kind = if extra.contains("Content-Type:") {
                ""
            } else {
                "Content-Type: application/json\r\n"
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\n{kind}Content-Length: {}\r\n{extra}Connection: close\r\n\r\n{answer}",
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
            r#"{"error":{"code":"bad_token","message":"the token is not valid","requestId":"r1","docs":"https://docs.elchi.dev/emx-errors#bad-token"}}"#,
        ),
        (404, "not json at all"),
    ]);
    let emx = Client::with_base_url(&base, "emx_bad").unwrap();
    let err = emx.me().unwrap_err();
    assert!(err.is_unauthorized());
    assert_eq!(err.code(), Some("bad_token"));
    assert_eq!(err.to_string(), "the token is not valid (bad_token, HTTP 401)");
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
fn plain_http_only_for_this_computer() {
    for ok in [
        "http://localhost",
        "http://localhost:8494",
        "http://LOCALHOST:8494/",
        "http://127.0.0.1",
        "http://127.0.0.1:8494",
        "http://[::1]:8494",
    ] {
        assert!(Client::with_base_url(ok, "emx_x").is_ok(), "{ok}");
    }
    for refused in [
        "http://localhost.example.ch:8494",
        "http://127.0.0.1.example.ch",
        "http://localhost@example.ch",
        "http://localhost:80@example.ch",
        "http://localhost:x",
        "http://localhost:",
        "http://[::1].example.ch",
        "http://[::1",
        "http://example.ch",
    ] {
        assert!(
            matches!(Client::with_base_url(refused, "emx_x"), Err(Error::Config(_))),
            "{refused}"
        );
    }
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
fn a_key_the_service_would_not_take_is_refused_before_sending() {
    let (base, seen) = server(vec![(200, SENT)]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    for key in ["", "  ", &"k".repeat(201), "rechnung 117", "rechnung-\u{e4}"] {
        let err = emx.send_mail_with_key(&draft(), key).unwrap_err();
        assert!(matches!(err, Error::Config(_)), "{key:?}: {err}");
    }
    assert!(seen.lock().unwrap().is_empty(), "nothing was sent");
    emx.send_mail_with_key(&draft(), &"k".repeat(200)).unwrap();
    assert_eq!(seen.lock().unwrap()[0].idempotency_key.len(), 200);
}

#[test]
fn a_new_key_is_one_the_service_takes() {
    let a = emx_sdk::idempotency_key();
    let b = emx_sdk::idempotency_key();
    assert_ne!(a, b);
    assert!(a.len() <= 200 && a.bytes().all(|c| c.is_ascii_graphic()), "{a}");
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

const MESSAGE: &str = r#"{"id":"x1","mailboxId":"m1","threadId":"t1","receivedAt":"2026-09-24T07:41:12Z","subject":"Offerte","from":{"name":"Anna","address":"anna@example.ch"},"to":["samuel@elchi.dev"],"snippet":"Danke","hasAttachments":true,"keywords":["$seen"],"size":48213,"modseq":811}"#;

#[test]
fn reading_messages() {
    let full = r#"{"message":{"id":"x1","mailboxId":"m1","threadId":"t1","receivedAt":"2026-09-24T07:41:12Z","subject":"Offerte","from":{"name":"Anna","address":"anna@example.ch"},"to":["samuel@elchi.dev"],"modseq":811},
      "body":{"text":"Danke.","html":"<p>Danke.</p>","remoteImages":1,"attachments":[{"part":"2","filename":"Offerte.pdf","contentType":"application/pdf","size":300000}],
      "replyTo":[],"messageId":"<a@example.ch>","inReplyTo":"","references":[]}}"#;
    let list = Box::leak(format!(r#"{{"messages":[{MESSAGE}]}}"#).into_boxed_str());
    let (base, seen) = server_with(vec![
        (200, full, ""),
        (
            200,
            "Subject: Offerte\r\n\r\nDanke.\r\n",
            "Content-Type: message/rfc822\r\n",
        ),
        (200, "%PDF-1.7", "Content-Type: application/pdf\r\n"),
        (200, list, ""),
        (200, list, ""),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let m = emx.message(ME, "x1").unwrap();
    assert_eq!(m.message.subject, "Offerte");
    assert_eq!(m.body.attachments[0].part, "2");
    assert_eq!(m.body.attachments[0].content_type, "application/pdf");
    assert_eq!(m.body.message_id, "<a@example.ch>");
    assert_eq!(emx.raw(ME, "x1").unwrap(), b"Subject: Offerte\r\n\r\nDanke.\r\n");
    let (data, kind) = emx.part(ME, "x1", "2").unwrap();
    assert_eq!(
        (data.as_slice(), kind.as_str()),
        (&b"%PDF-1.7"[..], "application/pdf")
    );
    assert_eq!(emx.thread(ME, "t1").unwrap()[0].id, "x1");
    assert_eq!(emx.search(ME, "offerte 2026").unwrap()[0].id, "x1");
    let seen = seen.lock().unwrap();
    let paths: Vec<&str> = seen.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "/api/accounts/me/messages/x1",
            "/api/accounts/me/messages/x1/raw",
            "/api/accounts/me/messages/x1/parts/2",
            "/api/accounts/me/threads/t1",
            "/api/accounts/me/search?q=offerte%202026",
        ]
    );
}

#[test]
fn changing_messages() {
    let (base, seen) = server(vec![
        (200, r#"{"changed":["x1"]}"#),
        (200, r#"{"changed":["x1","x2"]}"#),
        (200, r#"{"changed":["x1"]}"#),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    assert_eq!(emx.keywords(ME, &["x1"], &["$flagged"], &[]).unwrap(), vec!["x1"]);
    assert_eq!(emx.delete(ME, &["x1", "x2"]).unwrap().len(), 2);
    assert_eq!(
        emx.snooze(ME, &["x1"], "2026-09-28T07:00:00Z").unwrap(),
        vec!["x1"]
    );
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].path, "/api/accounts/me/messages/keywords");
    assert_eq!(seen[0].body, r#"{"add":["$flagged"],"ids":["x1"],"remove":[]}"#);
    assert_eq!(seen[1].path, "/api/accounts/me/messages/delete");
    assert_eq!(seen[1].body, r#"{"ids":["x1","x2"]}"#);
    assert_eq!(seen[2].path, "/api/accounts/me/messages/snooze");
    assert_eq!(seen[2].body, r#"{"ids":["x1"],"until":"2026-09-28T07:00:00Z"}"#);
    assert!(seen.iter().all(|s| s.method == "POST"));
}

#[test]
fn changes_and_a_reload() {
    let changes = Box::leak(
        format!(r#"{{"updated":[{MESSAGE}],"destroyed":["x0"],"modseq":812,"hasMore":true}}"#)
            .into_boxed_str(),
    );
    let (base, seen) = server(vec![
        (200, changes),
        (
            410,
            r#"{"error":{"code":"reload","message":"the state is too old; load the lists again"}}"#,
        ),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let c = emx.changes(ME, 811).unwrap();
    assert_eq!((c.updated[0].id.as_str(), c.destroyed[0].as_str()), ("x1", "x0"));
    assert_eq!(c.modseq, 812);
    assert!(c.has_more);
    let err = emx.changes(ME, 1).unwrap_err();
    assert_eq!(err.code(), Some("reload"));
    assert!(!err.is_retryable());
    assert_eq!(seen.lock().unwrap()[0].path, "/api/accounts/me/changes?since=811");
}

#[test]
fn the_event_stream() {
    let (base, seen) = server_with(vec![(
        200,
        "event: change\ndata: {\"modseq\": 812}\n\n: keepalive\n\nevent: change\ndata: {\"modseq\": 813}\n\n",
        "Content-Type: text/event-stream\r\n",
    )]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let events: Vec<Event> = emx.events(ME).unwrap().map(|e| e.unwrap()).collect();
    assert_eq!(
        events,
        [Event::Change { modseq: 812 }, Event::Change { modseq: 813 }]
    );
    assert_eq!(seen.lock().unwrap()[0].path, "/api/accounts/me/events");
}

#[test]
fn a_stream_the_service_does_not_offer_is_not_retried() {
    let (base, _) = server(vec![(
        501,
        r#"{"error":{"code":"unavailable","message":"live updates are off on this server"}}"#,
    )]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let err = emx.events(ME).err().unwrap();
    assert_eq!(err.code(), Some("unavailable"));
    assert!(!err.is_retryable());
}

#[test]
fn screener_decisions() {
    let (base, seen) = server(vec![(200, r#"{"ok":true}"#)]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    emx.set_contact(ME, "anna@example.ch", "approved", "Anna")
        .unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].path, "/api/accounts/me/contacts");
    assert_eq!(
        seen[0].body,
        r#"{"address":"anna@example.ch","name":"Anna","state":"approved"}"#
    );
}

#[test]
fn webhooks() {
    let hook = r#"{"id":"h1","accountId":"u1","url":"https://crm.example.ch/emx","events":["message.received"],"description":"CRM","createdAt":"2026-09-24T07:41:12Z","failures":0,"lastStatus":0}"#;
    let list = Box::leak(
        format!(r#"{{"webhooks":[{hook}],"events":["message.received","delivery.failed","ping"]}}"#)
            .into_boxed_str(),
    );
    let made = Box::leak(format!(r#"{{"webhook":{hook},"secret":"whsec_abc"}}"#).into_boxed_str());
    let (base, seen) = server(vec![
        (200, list),
        (200, made),
        (200, r#"{"ok":true}"#),
        (200, r#"{"ok":true}"#),
        (
            200,
            r#"{"deliveries":[{"id":"d1","event":"ping","attempts":1,"createdAt":"2026-09-24T07:41:12Z","deliveredAt":"2026-09-24T07:41:13Z","lastStatus":200}]}"#,
        ),
        (200, r#"{"ok":true}"#),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let (hooks, events) = emx.webhooks().unwrap();
    assert_eq!((hooks[0].id.as_str(), events.len()), ("h1", 3));
    let made = emx
        .create_webhook(ME, "https://crm.example.ch/emx", &["message.received"], "CRM")
        .unwrap();
    assert_eq!(made.secret, "whsec_abc");
    emx.test_webhook("h1").unwrap();
    emx.enable_webhook("h1").unwrap();
    let d = emx.webhook_deliveries("h1", 50).unwrap();
    assert!(d[0].delivered_at.is_some());
    emx.delete_webhook("h1").unwrap();
    let seen = seen.lock().unwrap();
    let calls: Vec<(&str, &str)> = seen
        .iter()
        .map(|s| (s.method.as_str(), s.path.as_str()))
        .collect();
    assert_eq!(
        calls,
        [
            ("GET", "/api/me/webhooks"),
            ("POST", "/api/accounts/me/webhooks"),
            ("POST", "/api/webhooks/h1/test"),
            ("POST", "/api/webhooks/h1/enable"),
            ("GET", "/api/webhooks/h1/deliveries?limit=50"),
            ("DELETE", "/api/webhooks/h1"),
        ]
    );
    assert_eq!(
        seen[1].body,
        r#"{"description":"CRM","events":["message.received"],"url":"https://crm.example.ch/emx"}"#
    );
}

#[test]
fn any_call() {
    let (base, seen) = server(vec![
        (200, r#"{"members":[]}"#),
        (200, r#"{"prefs":{"screener":true}}"#),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let v = emx.get_json("/api/admin/mine/members", &[("q", "anna")]).unwrap();
    assert!(v["members"].is_array());
    let v = emx
        .call_json("PATCH", "/api/me/prefs", &serde_json::json!({"screener": true}))
        .unwrap();
    assert_eq!(v["prefs"]["screener"], true);
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].path, "/api/admin/mine/members?q=anna");
    assert_eq!(
        (seen[1].method.as_str(), seen[1].body.as_str()),
        ("PATCH", r#"{"screener":true}"#)
    );
}
