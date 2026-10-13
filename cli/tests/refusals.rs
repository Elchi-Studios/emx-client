// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! How `emx` reports what EMX answers a send and a download with: the
//! exit status a script acts on and the words a person reads.

mod common;

use common::{emx, folder, scripted, Reply};

fn send(base: &str, extra: &[&str]) -> std::process::Output {
    emx(base)
        .args(["send", "--from", "samuel@elchi.dev", "--to", "anna@example.ch"])
        .args(["--subject", "Offerte", "--text", "Gerne."])
        .args(extra)
        .output()
        .unwrap()
}

fn try_later(_: &str, _: &str, _: usize) -> Reply {
    Reply::json(
        503,
        r#"{"error":{"code":"try_later","message":"the virus scanner is not answering"}}"#,
    )
    .with("Retry-After: 1")
}

/// A passing fault EMX names is no uncertain send: nothing went out, and
/// the same command works in a moment.
#[test]
fn try_later_is_reported_as_not_sent() {
    let (base, seen) = scripted(try_later);
    let out = send(&base, &["--idempotency-key", "k-try"]);
    assert_eq!(out.status.code(), Some(4));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("nothing was done or sent"), "{err}");
    assert!(!err.contains("may have been sent"), "{err}");
    assert_eq!(seen.lock().unwrap().len(), 3, "made again with the same key");
}

fn hourly(_: &str, _: &str, _: usize) -> Reply {
    Reply::json(
        429,
        r#"{"error":{"code":"hourly_limit","message":"more than 300 external recipients this hour"}}"#,
    )
    .with("Retry-After: 1740")
}

#[test]
fn a_limit_says_when_the_message_can_go() {
    let (base, seen) = scripted(hourly);
    let out = send(&base, &[]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not sent, and can go in 29 minutes"), "{err}");
    assert_eq!(seen.lock().unwrap().len(), 1);
}

fn mismatch(_: &str, _: &str, _: usize) -> Reply {
    Reply::json(
        422,
        r#"{"error":{"code":"idempotency_mismatch","message":"this Idempotency-Key was used for another request; use a new key for a new message"}}"#,
    )
}

#[test]
fn a_key_used_for_another_message_is_explained() {
    let (base, _) = scripted(mismatch);
    let out = send(&base, &["--idempotency-key", "rechnung-117"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("key of its own"), "{err}");
}

fn replayed(_: &str, _: &str, n: usize) -> Reply {
    if n == 0 {
        Reply::json(200, r#"{"sent":true,"recipients":1}"#).with("Idempotent-Replayed: true")
    } else {
        Reply::json(
            400,
            r#"{"error":{"code":"too_many_recipients","message":"a message goes to at most 100 recipients"}}"#,
        )
        .with("Idempotent-Replayed: true")
    }
}

#[test]
fn a_replayed_answer_says_nothing_went_out_again() {
    let (base, _) = scripted(replayed);
    let out = send(&base, &["--idempotency-key", "k1"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("not sent again"), "{text}");
    let out = send(&base, &["--idempotency-key", "k1"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("at most 100 recipients"), "{err}");
    assert!(err.contains("the first send with this key"), "{err}");
}

fn raw(_: &str, path: &str, _: usize) -> Reply {
    if path.ends_with("/raw") {
        Reply {
            status: 200,
            headers: "Content-Type: message/rfc822\r\nContent-Disposition: attachment; filename=\"Offerte fur Muller.eml\"; filename*=UTF-8''Offerte%20f%C3%BCr%20M%C3%BCller.eml\r\n".into(),
            body: "Subject: Offerte\r\n\r\nDanke.\r\n".into(),
        }
    } else {
        Reply {
            status: 200,
            headers: "Content-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"..%2F..%2Fx.pdf\"; filename*=UTF-8''..%2F..%2FRechnung%20M%C3%A4rz.pdf\r\n".into(),
            body: "%PDF-1.7".into(),
        }
    }
}

#[test]
fn a_message_and_a_part_are_saved_under_their_names() {
    let (base, _) = scripted(raw);
    let dir = folder("raw");
    let d = dir.to_str().unwrap();
    for _ in 0..2 {
        let out = emx(&base)
            .args(["read", "x1", "--raw", "--out", d])
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
    let out = emx(&base)
        .args(["part", "x1", "--part", "2", "--out", d])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "Offerte f\u{fc}r M\u{fc}ller (2).eml",
            "Offerte f\u{fc}r M\u{fc}ller.eml",
            "_.._Rechnung M\u{e4}rz.pdf"
        ]
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("Offerte f\u{fc}r M\u{fc}ller.eml")).unwrap(),
        "Subject: Offerte\r\n\r\nDanke.\r\n"
    );
    // A file named: written there.
    let file = dir.join("named.eml");
    let out = emx(&base)
        .args(["--json", "read", "x1", "--raw", "--out", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["filename"], "Offerte f\u{fc}r M\u{fc}ller.eml");
    assert!(file.exists());
    // --out without --raw is a wrong command line.
    let out = emx(&base).args(["read", "x1", "--out", d]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}

fn me(_: &str, _: &str, _: usize) -> Reply {
    Reply::json(
        200,
        r#"{"id":"u1","name":"Samuel Krauss","tenant":{"id":"t1","name":"Elchi Studios","plan":"team"},
          "accounts":[{"id":"u1","address":"samuel@elchi.dev","rights":{"own":true}}],
          "sendFrom":[{"address":"samuel@elchi.dev","primary":true},{"address":"news@elchi.dev","listMail":{"hourlyRecipients":5000,"dailyRecipients":50000}}],
          "limits":{"maxMessageBytes":52428800,"maxRecipients":100,"dailyRecipients":1000,"hourlyRecipients":300,"monthlyRecipients":20000,"monthlyUsed":1520}}"#,
    )
}

#[test]
fn me_names_the_limits() {
    let (base, _) = scripted(me);
    let out = emx(&base).arg("me").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for want in [
        "100 recipients a message",
        "50 MB a message",
        "300 recipients outside the organisation an hour",
        "1000 recipients a day",
        "20000 recipients a month for the organisation, 1520 used",
        "news@elchi.dev (list mail: 5000 an hour, 50000 a day)",
    ] {
        assert!(text.contains(want), "{want}: {text}");
    }
}

fn hooks(method: &str, _: &str, _: usize) -> Reply {
    if method == "GET" {
        Reply::json(
            200,
            r#"{"webhooks":[],"events":["message.received","delivery.failed","complaint.received"]}"#,
        )
    } else {
        Reply::json(
            200,
            r#"{"webhook":{"id":"h1","url":"https://crm.example.ch/emx","events":["message.received","delivery.failed","complaint.received"]},"secret":"whsec_abc"}"#,
        )
    }
}

#[test]
fn a_webhook_for_every_event() {
    let (base, seen) = scripted(hooks);
    let out = emx(&base)
        .args(["webhook", "add", "https://crm.example.ch/emx", "--events", "all"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let seen = seen.lock().unwrap();
    let body: serde_json::Value = serde_json::from_str(&seen[1].body).unwrap();
    assert_eq!(
        body["events"],
        serde_json::json!(["message.received", "delivery.failed", "complaint.received"])
    );
    drop(seen);
    let out = emx(&base)
        .args(["webhook", "add", "https://crm.example.ch/emx"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "events are named");
    assert!(String::from_utf8_lossy(&out.stderr).contains("complaint.received"));
}
