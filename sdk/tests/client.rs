// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

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
/// (status, body) pair and remembers what it saw. A 429 or 503 says
/// `Retry-After: 1`, as the service gives every one a `Retry-After`.
fn server(answers: Vec<(u16, &'static str)>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    server_with(
        answers
            .into_iter()
            .map(|(status, body)| {
                (
                    status,
                    body,
                    if status == 429 || status == 503 {
                        "Retry-After: 1\r\n"
                    } else {
                        ""
                    },
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

/// Preferences that are not an object, as a mistaken call can store,
/// must not stop `me` from working: it is how every program signs in.
#[test]
fn me_with_preferences_that_are_not_an_object() {
    for prefs in [r#"[{"screener":true},null]"#, "null", "\"x\""] {
        let me: emx_sdk::Me = serde_json::from_str(&format!(
            r#"{{"id":"u1","tenant":{{"id":"t1"}},"prefs":{prefs}}}"#
        ))
        .unwrap();
        assert!(me.prefs.is_empty(), "{prefs}");
    }
    let me: emx_sdk::Me = serde_json::from_str(r#"{"id":"u1","tenant":{"id":"t1"}}"#).unwrap();
    assert!(me.prefs.is_empty());
}

/// Fields the service sends that a client shows: a cancelled
/// organisation, list mail limits, mail sent on someone's behalf.
#[test]
fn optional_fields_are_read() {
    let me: emx_sdk::Me = serde_json::from_str(
        r#"{"id":"u1","tenant":{"id":"t1","hasLogo":true,"cancelledAt":"2026-10-01T00:00:00Z","purgeAfter":"2026-12-30T00:00:00Z"},
          "limits":{"listMail":{"hourlyRecipients":5000,"dailyRecipients":50000}},
          "orgs":[{"tenantId":"t1","principalId":"p1","state":"active"}]}"#,
    )
    .unwrap();
    assert!(me.tenant.has_logo);
    assert_eq!(me.tenant.cancelled_at.as_deref(), Some("2026-10-01T00:00:00Z"));
    assert_eq!(me.tenant.lapses_at, None);
    assert_eq!(me.limits.list_mail.unwrap().hourly_recipients, 5000);
    assert_eq!(me.orgs[0].principal_id, "p1");
    let body: emx_sdk::Body =
        serde_json::from_str(r#"{"sender":{"name":"Anna","address":"anna@example.ch"}}"#).unwrap();
    assert_eq!(body.sender.unwrap().address, "anna@example.ch");
    let c: emx_sdk::Contact =
        serde_json::from_str(r#"{"address":"tok","state":"approved","label":"YWdl"}"#).unwrap();
    assert_eq!(c.label, "YWdl");
}

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

/// More recipients outside the organisation than the hour allows: the
/// same send is refused until the hour is over, so it is not repeated,
/// even with a short `Retry-After`.
#[test]
fn the_hourly_limit_is_not_retried() {
    let (base, seen) = server(vec![(
        429,
        r#"{"error":{"code":"hourly_limit","message":"more than 300 external recipients this hour"}}"#,
    )]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let started = std::time::Instant::now();
    let err = emx.send_mail(&draft()).unwrap_err();
    assert_eq!(err.code(), Some("hourly_limit"));
    assert!(!err.is_retryable());
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(seen.lock().unwrap().len(), 1);
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
    let paged = Box::leak(format!(r#"{{"messages":[{MESSAGE}],"cursor":"c2"}}"#).into_boxed_str());
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
        (200, paged, ""),
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
    let page = emx.search_page(ME, "offerte", "m1", 500, "c1").unwrap();
    assert_eq!(page.items[0].id, "x1");
    assert_eq!(page.cursor, "c2");
    let seen = seen.lock().unwrap();
    let paths: Vec<&str> = seen.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "/api/accounts/me/messages/x1",
            "/api/accounts/me/messages/x1/raw",
            "/api/accounts/me/messages/x1/parts/2",
            "/api/accounts/me/threads/t1",
            "/api/accounts/me/search?q=offerte%202026&limit=50",
            "/api/accounts/me/search?q=offerte&limit=200&mailbox=m1&cursor=c1",
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

#[test]
fn a_redirect_names_where_it_points_and_is_not_followed() {
    let (base, seen) = server_with(vec![
        (301, "", "Location: https://mail.emxmail.app/api/me\r\n"),
        (302, "", ""),
    ]);
    let c = Client::with_base_url(&base, "emx_test").unwrap();
    let e = c.me().unwrap_err();
    assert_eq!(e.code(), Some("redirect"));
    assert!(!e.is_retryable());
    let text = e.to_string();
    assert!(text.contains("https://mail.emxmail.app/api/me"), "{text}");
    assert!(text.contains("base URL"), "{text}");
    let e = c.me().unwrap_err();
    assert!(e.to_string().contains("redirect"), "{e}");
    assert_eq!(seen.lock().unwrap().len(), 2, "nothing was followed");
}

const TRY_LATER: &str = r#"{"error":{"code":"try_later","message":"the server cannot check your sign-in right now; try again in a moment"}}"#;

/// A fault while the service checks the token is no sign-out: `me` is
/// asked again after the pause, and works.
#[test]
fn try_later_is_retried_for_a_read_and_is_no_sign_out() {
    let (base, seen) = server(vec![(503, TRY_LATER), (200, ME_JSON)]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let started = std::time::Instant::now();
    assert_eq!(emx.me().unwrap().name, "Samuel Krauss");
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(900),
        "waited Retry-After"
    );
    assert_eq!(seen.lock().unwrap().len(), 2);

    // Still refused after the retries: an error that passes, not 401.
    let (base, seen) = server(vec![(503, TRY_LATER), (503, TRY_LATER), (503, TRY_LATER)]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let err = emx.raw(ME, "x1").unwrap_err();
    assert!(err.is_try_later());
    assert!(err.is_retryable());
    assert!(!err.is_unauthorized());
    assert!(err.changed_nothing());
    assert_eq!(err.retry_after(), Some(1));
    assert_eq!(seen.lock().unwrap().len(), 3, "three attempts at most");
}

/// Changes that leave the same state however often they are made are
/// made again; one that makes something new is not, after a 503.
#[test]
fn try_later_is_retried_only_where_a_repeat_is_harmless() {
    let (base, seen) = server(vec![
        (503, TRY_LATER),
        (200, r#"{"changed":["x1"]}"#),
        (503, TRY_LATER),
        (503, TRY_LATER),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    assert_eq!(emx.delete(ME, &["x1"]).unwrap(), vec!["x1"]);
    let err = emx.create_mailbox(ME, "Projekte", None).unwrap_err();
    assert!(err.is_try_later());
    let err = emx.test_webhook("h1").unwrap_err();
    assert!(err.is_try_later());
    let seen = seen.lock().unwrap();
    let calls: Vec<&str> = seen.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(
        calls,
        [
            "/api/accounts/me/messages/delete",
            "/api/accounts/me/messages/delete",
            "/api/accounts/me/mailboxes",
            "/api/webhooks/h1/test",
        ]
    );
}

/// The rate limit refuses before anything is done, so even a call that
/// makes something is made again after it.
#[test]
fn the_rate_limit_is_retried_for_every_call() {
    let (base, seen) = server(vec![
        (429, r#"{"error":{"code":"rate_limited","message":"slow down"}}"#),
        (200, r#"{"id":"m9","name":"Projekte"}"#),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    assert_eq!(emx.create_mailbox(ME, "Projekte", None).unwrap().id, "m9");
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[test]
fn a_pause_above_the_limit_is_left_to_the_caller() {
    let (base, seen) = server_with(vec![(503, TRY_LATER, "Retry-After: 30\r\n")]);
    let emx = Client::with_base_url(&base, "emx_test")
        .unwrap()
        .max_wait(std::time::Duration::from_secs(5));
    let started = std::time::Instant::now();
    let err = emx.me().unwrap_err();
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(err.retry_after(), Some(30));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

/// The scanner away: the send is made again with the same key, which the
/// service runs again because it keeps no answer for a 503.
#[test]
fn a_send_refused_for_a_passing_fault_goes_again_with_its_key() {
    let (base, seen) = server_with(vec![
        (
            503,
            r#"{"error":{"code":"try_later","message":"the virus scanner is not answering"}}"#,
            "Retry-After: 1\r\n",
        ),
        (200, SENT, ""),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let sent = emx.send_mail_with_key(&draft(), "rechnung-118").unwrap();
    assert!(!sent.replayed);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert!(seen.iter().all(|s| s.idempotency_key == "rechnung-118"));
}

#[test]
fn final_refusals_of_a_send_are_not_repeated() {
    let (base, seen) = server_with(vec![
        (
            400,
            r#"{"error":{"code":"too_many_recipients","message":"at most 100 recipients"}}"#,
            "",
        ),
        (
            422,
            r#"{"error":{"code":"idempotency_mismatch","message":"this Idempotency-Key was used for another request; use a new key for a new message"}}"#,
            "",
        ),
        (
            429,
            r#"{"error":{"code":"hourly_limit","message":"more than 300 external recipients this hour"}}"#,
            "Retry-After: 1234\r\n",
        ),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let err = emx.send_mail(&draft()).unwrap_err();
    assert_eq!(err.code(), Some("too_many_recipients"));
    assert!(!err.is_retryable() && err.changed_nothing());
    let err = emx.send_mail(&draft()).unwrap_err();
    assert_eq!(err.code(), Some("idempotency_mismatch"));
    assert!(!err.is_retryable() && err.changed_nothing());
    let err = emx.send_mail(&draft()).unwrap_err();
    assert_eq!(err.code(), Some("hourly_limit"));
    assert_eq!(err.retry_after(), Some(1234), "the exact wait");
    assert!(!err.is_retryable());
    assert_eq!(seen.lock().unwrap().len(), 3, "one request each");
}

/// The answer kept under a key comes back with `Idempotent-Replayed`:
/// a message sent before, or the refusal it got.
#[test]
fn a_replayed_answer_says_so() {
    let (base, _) = server_with(vec![
        (200, SENT, "Idempotent-Replayed: true\r\n"),
        (
            400,
            r#"{"error":{"code":"unknown_recipient","message":"no such recipient: nobody@elchi.dev"}}"#,
            "Idempotent-Replayed: true\r\n",
        ),
        (200, SENT, ""),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    assert!(emx.send_mail_with_key(&draft(), "k1").unwrap().replayed);
    let err = emx.send_mail_with_key(&draft(), "k2").unwrap_err();
    assert!(err.is_replayed());
    assert_eq!(err.code(), Some("unknown_recipient"));
    assert!(!emx.send_mail_with_key(&draft(), "k3").unwrap().replayed);
}

#[test]
fn a_method_the_path_does_not_take() {
    let (base, _) = server_with(vec![
        (
            405,
            r#"{"error":{"code":"method_not_allowed","message":"PATCH is not allowed here; use GET or POST"}}"#,
            "Allow: GET, POST\r\n",
        ),
        (405, "", "Allow: GET, POST\r\nContent-Type: text/plain\r\n"),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let err = emx
        .call_json("PATCH", "/api/me/webhooks", &serde_json::json!({}))
        .unwrap_err();
    assert_eq!(err.code(), Some("method_not_allowed"));
    assert!(!err.is_retryable() && !err.is_not_found());
    let err = emx
        .call_json("PATCH", "/api/me/webhooks", &serde_json::json!({}))
        .unwrap_err();
    assert_eq!(err.code(), Some("method_not_allowed"));
    assert!(err.to_string().contains("GET, POST"), "{err}");
    let err = emx
        .call_json("FETCH", "/api/me", &serde_json::json!({}))
        .unwrap_err();
    assert!(matches!(err, Error::Config(_)), "{err}");
}

#[test]
fn the_limits_a_send_is_held_to() {
    let me: emx_sdk::Me = serde_json::from_str(
        r#"{"id":"u1","tenant":{"id":"t1"},"mailHost":"mail.emxmail.app",
          "limits":{"maxMessageBytes":52428800,"maxRecipients":100,"dailyRecipients":1000,"hourlyRecipients":300,"monthlyRecipients":20000,"monthlyUsed":1520},
          "sendFrom":[{"address":"news@elchi.dev","mode":"as","listMail":{"hourlyRecipients":5000,"dailyRecipients":50000}},
                      {"address":"samuel@elchi.dev","mode":"own","primary":true}]}"#,
    )
    .unwrap();
    assert_eq!(me.limits.max_recipients, 100);
    assert_eq!(me.limits.hourly_recipients, 300);
    assert_eq!(me.limits.monthly_used, 1520);
    assert_eq!(me.mail_host, "mail.emxmail.app");
    assert_eq!(
        me.send_from[0].list_mail.as_ref().unwrap().daily_recipients,
        50000
    );
    assert!(me.send_from[1].list_mail.is_none());
    // An older service without them.
    let me: emx_sdk::Me = serde_json::from_str(r#"{"id":"u1","tenant":{"id":"t1"},"limits":{}}"#).unwrap();
    assert_eq!((me.limits.max_recipients, me.limits.hourly_recipients), (0, 0));
}

#[test]
fn list_mail_and_forwarded_files_are_sent_as_named() {
    let (base, seen) = server(vec![(200, SENT)]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let mut d = draft();
    d.headers
        .insert("List-Id".into(), "Elchi Studios <newsletter.elchi.dev>".into());
    d.forward_parts.push(emx_sdk::PartRef {
        account: "me".into(),
        message: "x1".into(),
        part: "2".into(),
    });
    emx.send_mail(&d).unwrap();
    let body: serde_json::Value = serde_json::from_str(&seen.lock().unwrap()[0].body).unwrap();
    assert_eq!(body["headers"]["List-Id"], "Elchi Studios <newsletter.elchi.dev>");
    assert_eq!(body["forwardParts"][0]["part"], "2");
}

/// The SHA-256 of "Subject: Offerte\r\n\r\nDanke.\r\n", as the service
/// gives it for the raw source.
const RAW: &str = "Subject: Offerte\r\n\r\nDanke.\r\n";

fn raw_etag() -> String {
    let sum = emx_sdk::webhook::sha256(RAW.as_bytes());
    format!(
        "\"{}\"",
        sum.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}

#[test]
fn a_message_is_saved_under_the_name_the_service_gives() {
    let headers = Box::leak(
        format!(
            "Content-Type: message/rfc822\r\nContent-Disposition: attachment; filename=\"Offerte fur Muller.eml\"; filename*=UTF-8''Offerte%20f%C3%BCr%20M%C3%BCller.eml\r\nETag: {}\r\nLast-Modified: Wed, 24 Sep 2026 07:41:12 GMT\r\nAccept-Ranges: bytes\r\n",
            raw_etag()
        )
        .into_boxed_str(),
    );
    let (base, _) = server_with(vec![
        (200, RAW, headers),
        (200, "changed on the way", headers),
        (
            200,
            "%PDF-1.7",
            "Content-Type: application/pdf\r\nContent-Disposition: inline; filename=\"Offerte.pdf\"\r\n",
        ),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let d = emx.raw_download(ME, "x1").unwrap();
    assert_eq!(d.data, RAW.as_bytes());
    assert_eq!(d.filename, "Offerte f\u{fc}r M\u{fc}ller.eml");
    assert_eq!(d.file_name("message.eml"), "Offerte f\u{fc}r M\u{fc}ller.eml");
    assert_eq!(d.content_type, "message/rfc822");
    assert_eq!(d.last_modified, "Wed, 24 Sep 2026 07:41:12 GMT");
    // Bytes that are not the ones the ETag names are refused.
    let err = emx.raw_download(ME, "x1").unwrap_err();
    assert!(matches!(err, Error::Transport(_)), "{err}");
    assert!(err.to_string().contains("damaged"), "{err}");
    let d = emx.part_download(ME, "x1", "2").unwrap();
    assert_eq!(d.filename, "Offerte.pdf");
}

#[test]
fn file_names_from_a_disposition() {
    use emx_sdk::disposition_filename as name;
    assert_eq!(
        name("attachment; filename=\"a.eml\"; filename*=UTF-8''%E2%82%AC%20rates.eml").as_deref(),
        Some("\u{20ac} rates.eml")
    );
    assert_eq!(
        name("attachment; filename*=utf-8'de'Gr%C3%BCsse.txt; filename=\"Grusse.txt\"").as_deref(),
        Some("Gr\u{fc}sse.txt")
    );
    assert_eq!(
        name("attachment; filename=\"say \\\"hi\\\"; now.eml\"").as_deref(),
        Some("say \"hi\"; now.eml")
    );
    assert_eq!(
        name("attachment; filename=plain.pdf").as_deref(),
        Some("plain.pdf")
    );
    assert_eq!(
        name("attachment; filename*=iso-8859-1''M%FCller.pdf").as_deref(),
        Some("M\u{fc}ller.pdf")
    );
    // A broken extended value falls back to the plain name.
    assert_eq!(
        name("attachment; filename*=UTF-8''%ZZbad; filename=\"ok.pdf\"").as_deref(),
        Some("ok.pdf")
    );
    assert_eq!(name("attachment; filename*=UTF-8''%C3").as_deref(), None);
    assert_eq!(name("attachment"), None);
    assert_eq!(name(""), None);
    assert_eq!(name("inline; filename=\"\""), None);
}

#[test]
fn file_names_fit_every_file_system() {
    use emx_sdk::safe_file_name as safe;
    assert_eq!(safe("Rechnung 2026.pdf", "x"), "Rechnung 2026.pdf");
    assert_eq!(safe("../../etc/passwd", "x"), "_.._etc_passwd");
    assert_eq!(safe(" .. ", "attachment"), "attachment");
    assert_eq!(safe("CON", "x"), "_CON");
    assert_eq!(safe("nul.pdf", "x"), "_nul.pdf");
    assert_eq!(safe("com1.txt", "x"), "_com1.txt");
    assert_eq!(safe("LPT9", "x"), "_LPT9");
    assert_eq!(safe("com\u{b9}.log", "x"), "_com\u{b9}.log");
    assert_eq!(safe("console.pdf", "x"), "console.pdf");
    assert_eq!(safe("com10.txt", "x"), "com10.txt");
    assert_eq!(safe("Rechnung\u{202e}fdp.exe", "x"), "Rechnungfdp.exe");
    assert_eq!(safe("a\u{0}b\nc.eml", "x"), "a_b_c.eml");
    let long = format!("{}.pdf", "\u{8acb}".repeat(90));
    let short = safe(&long, "x");
    assert!(short.len() <= emx_sdk::FILE_NAME_MAX, "{}", short.len());
    assert!(short.ends_with("\u{8acb}.pdf"), "{short}");
    assert_eq!(safe(&"a".repeat(300), "x").len(), emx_sdk::FILE_NAME_MAX);
}

/// A transfer that breaks after its first bytes goes on with a range of
/// the same version, and the whole is checked against its ETag.
#[test]
fn a_broken_download_goes_on_where_it_broke() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let etag = raw_etag();
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for (n, stream) in listener.incoming().enumerate() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 && line != "\r\n" {
                head.push_str(&line);
                line.clear();
            }
            log.lock().unwrap().push(head.to_ascii_lowercase());
            if n == 0 {
                // The length of the whole, and only its first ten bytes.
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: message/rfc822\r\nETag: {etag}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    RAW.len(),
                    &RAW[..10]
                );
                let _ = stream.flush();
                drop(stream);
            } else {
                let rest = &RAW[10..];
                let _ = write!(
                    stream,
                    "HTTP/1.1 206 Partial Content\r\nContent-Type: message/rfc822\r\nETag: {etag}\r\nContent-Range: bytes 10-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{rest}",
                    RAW.len() - 1,
                    RAW.len(),
                    rest.len()
                );
                break;
            }
        }
    });
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let d = emx.raw_download(ME, "x1").unwrap();
    assert_eq!(d.data, RAW.as_bytes());
    assert_eq!(d.filename, "message.eml", "no name given: the default");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert!(seen[1].contains("range: bytes=10-"), "{}", seen[1]);
    assert!(
        seen[1].contains(&format!("if-range: {}", raw_etag())),
        "{}",
        seen[1]
    );
}

const SAVED: &str = r#"{"id":"d1","account":"s1","version":4,"body":{"from":"contact@elchi.dev","to":"anna@example.ch","subject":"Offerte","text":"Gerne.","answers":"x9"},
  "createdAt":"2026-10-13T07:00:00Z","createdBy":{"id":"u1","name":"Samuel Krauss"},"updatedAt":"2026-10-13T07:05:00Z","editedBy":{"id":"u2","name":"Anna Beispiel"}}"#;

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

#[test]
fn shared_drafts_with_versions() {
    let (base, seen) = server(vec![
        (200, leak(format!(r#"{{"drafts":[{SAVED}]}}"#))),
        (200, leak(format!(r#"{{"draft":{SAVED}}}"#))),
        (200, leak(format!(r#"{{"draft":{}}}"#, SAVED.replace(r#""version":4"#, r#""version":5"#)))),
        (
            409,
            leak(format!(
                r#"{{"error":{{"code":"conflict","message":"the draft changed meanwhile; this is the newer version"}},"draft":{}}}"#,
                SAVED.replace(r#""version":4"#, r#""version":6"#)
            )),
        ),
        (
            409,
            leak(format!(
                r#"{{"error":{{"code":"sent","message":"the draft was sent already"}},"draft":{}}}"#,
                SAVED.replace(r#""version":4"#, r#""version":6,"sentAt":"2026-10-13T07:06:00Z","sentBy":{"id":"u2","name":"Anna Beispiel"}"#)
            )),
        ),
        (200, r#"{"ok":true}"#),
        (409, r#"{"error":{"code":"sealed","message":"this mailbox is sealed; its drafts are saved sealed"}}"#),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let list = emx.drafts("s1").unwrap();
    assert_eq!(list[0].edited_by.as_ref().unwrap().name, "Anna Beispiel");
    let f = list[0].fields();
    assert_eq!(
        (f.from.as_str(), f.subject.as_str(), f.sealed),
        ("contact@elchi.dev", "Offerte", false)
    );
    let d = emx.draft("s1", "d1").unwrap();
    assert_eq!(d.version, 4);
    // A change on version 4: saved as 5, the web client's own fields kept.
    let mut body = d.body.clone();
    body["text"] = "Gerne, bis Freitag.".into();
    let saved = emx.save_draft("s1", "d1", 4, &body).unwrap().done().unwrap();
    assert_eq!(saved.version, 5);
    // Made on 5 while somebody saved 6.
    match emx.save_draft("s1", "d1", 5, &body).unwrap() {
        emx_sdk::OnVersion::Changed(newer) => assert_eq!(newer.version, 6),
        other => panic!("{other:?}"),
    }
    match emx.delete_draft("s1", "d1", Some(6)).unwrap() {
        emx_sdk::OnVersion::Sent(d) => assert_eq!(d.sent_by.unwrap().name, "Anna Beispiel"),
        other => panic!("{other:?}"),
    }
    assert!(emx.delete_draft("s1", "d1", None).unwrap().done().is_some());
    let err = emx.save_draft("me", "d2", 0, &body).unwrap_err();
    assert_eq!(err.code(), Some("sealed"));
    let seen = seen.lock().unwrap();
    let calls: Vec<(&str, &str)> = seen
        .iter()
        .map(|s| (s.method.as_str(), s.path.as_str()))
        .collect();
    assert_eq!(
        calls,
        [
            ("GET", "/api/accounts/s1/drafts"),
            ("GET", "/api/accounts/s1/drafts/d1"),
            ("PUT", "/api/accounts/s1/drafts/d1"),
            ("PUT", "/api/accounts/s1/drafts/d1"),
            ("DELETE", "/api/accounts/s1/drafts/d1?version=6"),
            ("DELETE", "/api/accounts/s1/drafts/d1"),
            ("PUT", "/api/accounts/me/drafts/d2"),
        ]
    );
    let put: serde_json::Value = serde_json::from_str(&seen[2].body).unwrap();
    assert_eq!(put["version"], 4);
    assert_eq!(put["body"]["answers"], "x9", "fields of the web client are kept");
    assert_eq!(put["body"]["text"], "Gerne, bis Freitag.");
    assert_eq!(seen[5].body, "", "a deletion sends no body");
}

const HELD: &str = r#"{"id":"o1","draft":"d1","account":"s1","kind":"later","sendAt":"2026-10-14T07:00:00Z","state":"held","createdAt":"2026-10-13T07:06:00Z","sender":{"id":"u1","name":"Samuel Krauss"}}"#;

#[test]
fn a_draft_is_sent_held_and_taken_back() {
    let (base, seen) = server_with(vec![
        (202, leak(format!(r#"{{"held":true,"outbox":{HELD}}}"#)), ""),
        (200, r#"{"sent":true,"recipients":2}"#, "Idempotent-Replayed: true\r\n"),
        (
            409,
            leak(format!(
                r#"{{"error":{{"code":"sent","message":"the draft was sent already"}},"draft":{}}}"#,
                SAVED.replace(r#""version":4"#, r#""version":5,"sentAt":"2026-10-13T07:06:00Z","sentBy":{"id":"u2","name":"Anna Beispiel"}"#)
            )),
            "",
        ),
        (
            200,
            leak(format!(
                r#"{{"outbox":[{},{}]}}"#,
                HELD.replace("}}", &format!(r#"}},"message":{SAVED}}}"#)),
                HELD.replace(r#""state":"held""#, r#""state":"failed","errorCode":"daily_limit","errorMessage":"the daily limit is reached""#)
            )),
            "",
        ),
        (200, leak(format!(r#"{{"outbox":{}}}"#, HELD.replace("2026-10-14T07:00:00Z", "2026-10-15T07:00:00Z"))), ""),
        (
            200,
            leak(format!(
                r#"{{"draft":{},"attachments":[{{"filename":"Offerte.pdf","contentType":"application/pdf","data":"JVBERi0="}}]}}"#,
                SAVED.replace(r#""version":4"#, r#""version":7"#)
            )),
            "",
        ),
        (404, r#"{"error":{"code":"not_found","message":"no such held message; it may have been sent"}}"#, ""),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let later = emx_sdk::Hold {
        send_at: Some("2026-10-14T07:00:00Z".into()),
        remind_days: Some(3),
        ..Default::default()
    };
    let sent = emx
        .send_draft("s1", "d1", 4, &draft(), &later, "k-later")
        .unwrap()
        .done()
        .unwrap();
    assert!(sent.held);
    assert_eq!(sent.outbox.unwrap().send_at, "2026-10-14T07:00:00Z");
    let undo = emx_sdk::Hold {
        undo_seconds: Some(10),
        ..Default::default()
    };
    let sent = emx
        .send_draft("s1", "d1", 4, &draft(), &undo, "k-undo")
        .unwrap()
        .done()
        .unwrap();
    assert!(sent.sent && sent.replayed && !sent.held);
    assert!(matches!(
        emx.send_draft("s1", "d1", 4, &draft(), &undo, "k-other").unwrap(),
        emx_sdk::OnVersion::Sent(_)
    ));
    let held = emx.outbox("s1").unwrap();
    assert_eq!(held.len(), 2);
    assert_eq!(held[0].message.as_ref().unwrap().fields().subject, "Offerte");
    assert_eq!(
        (held[1].state.as_str(), held[1].error_code.as_str()),
        ("failed", "daily_limit")
    );
    let moved = emx.reschedule_held("s1", "o1", "2026-10-15T07:00:00Z").unwrap();
    assert_eq!(moved.send_at, "2026-10-15T07:00:00Z");
    let back = emx.cancel_held("s1", "o1").unwrap();
    assert_eq!(back.draft.version, 7);
    assert_eq!(back.attachments[0].filename, "Offerte.pdf");
    assert!(emx.cancel_held("s1", "o1").unwrap_err().is_not_found());
    let seen = seen.lock().unwrap();
    let calls: Vec<(&str, &str)> = seen
        .iter()
        .map(|s| (s.method.as_str(), s.path.as_str()))
        .collect();
    assert_eq!(
        calls,
        [
            ("POST", "/api/accounts/s1/drafts/d1/send"),
            ("POST", "/api/accounts/s1/drafts/d1/send"),
            ("POST", "/api/accounts/s1/drafts/d1/send"),
            ("GET", "/api/accounts/s1/outbox"),
            ("PATCH", "/api/accounts/s1/outbox/o1"),
            ("POST", "/api/accounts/s1/outbox/o1/cancel"),
            ("POST", "/api/accounts/s1/outbox/o1/cancel"),
        ]
    );
    let first: serde_json::Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(first["version"], 4);
    assert_eq!(first["sendAt"], "2026-10-14T07:00:00Z");
    assert_eq!(first["remindDays"], 3);
    assert!(first.get("undoSeconds").is_none());
    assert_eq!(first["message"]["to"], "anna@example.ch");
    assert_eq!(seen[0].idempotency_key, "k-later");
    let second: serde_json::Value = serde_json::from_str(&seen[1].body).unwrap();
    assert_eq!(second["undoSeconds"], 10);
    let moved: serde_json::Value = serde_json::from_str(&seen[4].body).unwrap();
    assert_eq!(moved, serde_json::json!({"sendAt": "2026-10-15T07:00:00Z"}));
}

/// Send later in one call: the message is saved as a new draft, of the
/// account the service puts it in, and sent from there.
#[test]
fn a_message_is_held_as_a_new_draft() {
    let (base, seen) = server_with(vec![
        (
            200,
            leak(format!(
                r#"{{"draft":{}}}"#,
                SAVED.replace(r#""version":4"#, r#""version":1"#)
            )),
            "",
        ),
        (202, leak(format!(r#"{{"held":true,"outbox":{HELD}}}"#)), ""),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let mut d = draft();
    d.attachments.push(emx_sdk::DraftAttachment {
        filename: "Offerte.pdf".into(),
        data: "JVBERi0=".into(),
        ..Default::default()
    });
    let hold = emx_sdk::Hold {
        send_at: Some("2026-10-14T07:00:00Z".into()),
        ..Default::default()
    };
    let sent = emx.send_held(ME, &d, &hold, "k1").unwrap();
    assert_eq!(sent.outbox.unwrap().id, "o1");
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].method, "PUT");
    assert!(seen[0].path.starts_with("/api/accounts/me/drafts/"));
    let id = seen[0].path.rsplit('/').next().unwrap().to_string();
    assert_eq!(seen[1].path, format!("/api/accounts/s1/drafts/{}/send", "d1"));
    let put: serde_json::Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(put["version"], 0);
    assert_eq!(put["body"]["subject"], "Offerte");
    assert_eq!(put["body"]["cc"], "");
    assert!(put["body"].get("attachments").is_none(), "a draft keeps no files");
    let send: serde_json::Value = serde_json::from_str(&seen[1].body).unwrap();
    assert_eq!(send["version"], 1);
    assert_eq!(send["message"]["attachments"][0]["filename"], "Offerte.pdf");
    // A UUID of version 4.
    assert_eq!(id.len(), 36);
    assert_eq!(&id[14..15], "4");
    assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"), "{id}");
    assert_ne!(emx_sdk::new_draft_id(), emx_sdk::new_draft_id());
}

#[test]
fn snippets() {
    let snippet = r#"{"id":"n1","account":"u1","body":{"name":"thanks","text":"Hello {firstName},\n\nthank you!\n{myName}"},"createdAt":"2026-10-13T07:00:00Z","updatedAt":"2026-10-13T07:00:00Z"}"#;
    let (base, seen) = server(vec![
        (
            200,
            leak(format!(
                r#"{{"snippets":[{snippet},{{"id":"n2","account":"u1","body":{{"sealed":"YWdl"}}}}],"editable":true}}"#
            )),
        ),
        (200, leak(format!(r#"{{"snippet":{snippet}}}"#))),
        (
            409,
            r#"{"error":{"code":"too_many","message":"an account keeps at most 200 snippets"}}"#,
        ),
        (200, r#"{"ok":true}"#),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let list = emx.snippets(ME).unwrap();
    assert!(list.editable);
    assert_eq!(list.snippets[0].name(), "thanks");
    assert!(list.snippets[0].text().starts_with("Hello {firstName},"));
    assert!(list.snippets[1].is_sealed() && list.snippets[1].name().is_empty());
    assert_eq!(emx.save_snippet(ME, "n1", "thanks", "Hello").unwrap().id, "n1");
    assert_eq!(
        emx.save_snippet(ME, "n3", "more", "x").unwrap_err().code(),
        Some("too_many")
    );
    emx.delete_snippet(ME, "n1").unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen[1].method, "PUT");
    assert_eq!(seen[1].path, "/api/accounts/me/snippets/n1");
    assert_eq!(seen[1].body, r#"{"body":{"name":"thanks","text":"Hello"}}"#);
    assert_eq!(
        (seen[3].method.as_str(), seen[3].path.as_str()),
        ("DELETE", "/api/accounts/me/snippets/n1")
    );
}

/// The outcome of a change on a version, as the desktop app's window
/// reads it.
#[test]
fn an_outcome_on_a_version_serializes_with_its_kind() {
    let changed: emx_sdk::OnVersion<()> = emx_sdk::OnVersion::Changed(serde_json::from_str(SAVED).unwrap());
    let v = serde_json::to_value(&changed).unwrap();
    assert_eq!(v["outcome"], "changed");
    assert_eq!(v["value"]["version"], 4);
    let done = serde_json::to_value(emx_sdk::OnVersion::Done(1)).unwrap();
    assert_eq!(done, serde_json::json!({"outcome": "done", "value": 1}));
}

/// The same held send made again with its key after a lost answer finds
/// the draft of the first and gets the first answer, instead of holding
/// the message twice.
#[test]
fn a_held_send_made_again_with_its_key_is_held_once() {
    let sent_draft = leak(format!(
        r#"{{"error":{{"code":"sent","message":"the draft was sent already"}},"draft":{}}}"#,
        SAVED.replace(r#""version":4"#, r#""version":1,"sentAt":"2026-10-13T07:06:00Z""#)
    ));
    let (base, seen) = server_with(vec![
        (
            200,
            leak(format!(
                r#"{{"draft":{}}}"#,
                SAVED.replace(r#""version":4"#, r#""version":1"#)
            )),
            "",
        ),
        (202, leak(format!(r#"{{"held":true,"outbox":{HELD}}}"#)), ""),
        (409, sent_draft, ""),
        (
            202,
            leak(format!(r#"{{"held":true,"outbox":{HELD}}}"#)),
            "Idempotent-Replayed: true\r\n",
        ),
        (
            200,
            leak(format!(
                r#"{{"draft":{}}}"#,
                SAVED.replace(r#""version":4"#, r#""version":1"#)
            )),
            "",
        ),
    ]);
    let emx = Client::with_base_url(&base, "emx_test").unwrap();
    let hold = emx_sdk::Hold {
        send_at: Some("2026-10-14T07:00:00Z".into()),
        ..Default::default()
    };
    assert!(!emx.send_held(ME, &draft(), &hold, "k-held").unwrap().replayed);
    let again = emx.send_held(ME, &draft(), &hold, "k-held").unwrap();
    assert!(again.replayed && again.held);
    // Another key names another draft.
    let _ = emx.send_held(ME, &draft(), &hold, "k-other");
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].path, seen[2].path, "the same draft for the same key");
    assert_ne!(seen[0].path, seen[4].path);
    let send: serde_json::Value = serde_json::from_str(&seen[3].body).unwrap();
    assert_eq!(send["version"], 1, "on the version the first call saved");
    assert_eq!(seen[3].idempotency_key, "k-held");
}
