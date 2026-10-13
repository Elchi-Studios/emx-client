// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Send later, the Scheduled list and snippets, against a server that
//! answers as the service does.

mod common;

use common::{emx, scripted, Reply};

const DRAFT: &str = r#"{"id":"0b4c2f0e-6a1d-4c3e-9f7a-2b8d1e5c7a90","account":"s1","version":1,"body":{"from":"contact@elchi.dev","to":"anna@example.ch","subject":"Offerte"},"updatedAt":"2026-10-13T07:00:00Z"}"#;
const HELD: &str = r#"{"id":"o1","draft":"0b4c2f0e-6a1d-4c3e-9f7a-2b8d1e5c7a90","account":"s1","kind":"later","sendAt":"2099-01-01T07:00:00Z","state":"held","createdAt":"2026-10-13T07:06:00Z","sender":{"id":"u1","name":"Samuel Krauss"}}"#;

fn service(method: &str, path: &str, _: usize) -> Reply {
    match (method, path) {
        ("PUT", p) if p.contains("/drafts/") => Reply::json(200, &format!(r#"{{"draft":{DRAFT}}}"#)),
        ("POST", p) if p.ends_with("/send") => Reply::json(202, &format!(r#"{{"held":true,"outbox":{HELD}}}"#)),
        ("GET", "/api/accounts/me/outbox") => Reply::json(
            200,
            &format!(
                r#"{{"outbox":[{},{}]}}"#,
                HELD.replacen("}}", &format!(r#"}},"message":{DRAFT}}}"#), 1),
                HELD.replace(r#""state":"held""#, r#""state":"failed","errorCode":"daily_limit","errorMessage":"the daily limit is reached""#)
                    .replace(r#""id":"o1""#, r#""id":"o2""#)
            ),
        ),
        ("POST", "/api/accounts/me/outbox/o1/cancel") => Reply::json(
            200,
            &format!(
                r#"{{"draft":{DRAFT},"attachments":[{{"filename":"Offerte.pdf","contentType":"application/pdf","data":"JVBERi0="}}]}}"#
            ),
        ),
        ("PATCH", "/api/accounts/me/outbox/o1") => Reply::json(200, &format!(r#"{{"outbox":{HELD}}}"#)),
        ("GET", "/api/accounts/me/snippets") => Reply::json(
            200,
            r#"{"snippets":[{"id":"n1","account":"u1","body":{"name":"thanks","text":"\nHello {firstName},\nthank you!"}},{"id":"n2","account":"u1","body":{"sealed":"YWdl"}}],"editable":true}"#,
        ),
        _ => Reply::json(404, r#"{"error":{"code":"not_found","message":"no such endpoint"}}"#),
    }
}

#[test]
fn send_later_holds_the_message_as_a_draft() {
    let (base, seen) = scripted(service);
    let out = emx(&base)
        .args(["send", "--from", "contact@elchi.dev", "--to", "anna@example.ch"])
        .args(["--subject", "Offerte", "--text", "Gerne.", "--at", "+2h"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("held until 2099-01-01 07:00 UTC"), "{text}");
    assert!(text.contains("emx scheduled cancel o1"), "{text}");
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].method, "PUT");
    assert!(
        seen[0].path.starts_with("/api/accounts/me/drafts/"),
        "{}",
        seen[0].path
    );
    let put: serde_json::Value = serde_json::from_str(&seen[0].body).unwrap();
    assert_eq!(put["version"], 0);
    assert_eq!(put["body"]["text"], "Gerne.");
    assert_eq!(
        seen[1].path, "/api/accounts/s1/drafts/0b4c2f0e-6a1d-4c3e-9f7a-2b8d1e5c7a90/send",
        "sent from the account the draft went to"
    );
    let send: serde_json::Value = serde_json::from_str(&seen[1].body).unwrap();
    assert_eq!(send["version"], 1);
    assert!(send["sendAt"].as_str().unwrap().ends_with('Z'));
    assert_eq!(send["message"]["subject"], "Offerte");
}

#[test]
fn a_wrong_time_is_refused_before_anything_is_sent() {
    let (base, seen) = scripted(service);
    for at in ["tomorrow", "2026-10-14T07:00:00", "2020-01-01T07:00Z", "+400d"] {
        let out = emx(&base)
            .args(["send", "--from", "contact@elchi.dev", "--to", "anna@example.ch"])
            .args(["--subject", "Offerte", "--text", "Gerne.", "--at", at])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{at}");
    }
    assert!(seen.lock().unwrap().is_empty());
}

#[test]
fn the_scheduled_list_and_its_changes() {
    let (base, seen) = scripted(service);
    let out = emx(&base).args(["scheduled"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("2099-01-01 07:00 UTC  held  anna@example.ch  Offerte"),
        "{text}"
    );
    assert!(text.contains("failed: the daily limit is reached"), "{text}");
    let out = emx(&base).args(["scheduled", "cancel", "o1"]).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("draft again, without its file"), "{text}");
    let out = emx(&base)
        .args(["scheduled", "move", "o1", "--at", "+3d"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let out = emx(&base).args(["scheduled", "move", "o1"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "move needs --at");
    let seen = seen.lock().unwrap();
    let moved = seen.iter().find(|s| s.method == "PATCH").unwrap();
    let v: serde_json::Value = serde_json::from_str(&moved.body).unwrap();
    let at = v["sendAt"].as_str().unwrap();
    assert!(at.len() == 20 && at.ends_with('Z'), "{at}");
}

#[test]
fn snippets_by_name() {
    let (base, _) = scripted(service);
    let out = emx(&base).args(["snippets", "list"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("thanks  Hello {firstName},"), "{text}");
    assert!(text.contains("sealed"), "{text}");
    let out = emx(&base).args(["--json", "snippets"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["snippets"][0]["body"]["name"], "thanks");
}

fn calendar(_: &str, path: &str, _: usize) -> Reply {
    if path == "/api/calendars" {
        return Reply::json(
            200,
            r##"{"calendars":[{"id":"c1","name":"Kalender","timeZone":"Europe/Zurich","default":true,"access":{"own":true,"read":true,"write":true}},
                {"id":"c2","name":"Kontakt","timeZone":"Europe/Zurich","access":{"read":true}}]}"##,
        );
    }
    assert!(path.starts_with("/api/calendars/occurrences?from="), "{path}");
    Reply::json(
        200,
        r#"{"events":[
          {"id":"e2","calendarId":"c2","summary":"Messe","allDay":true,"start":"2026-10-13T00:00:00Z","end":"2026-10-16T00:00:00Z","startLocal":"2026-10-13","endLocal":"2026-10-16","timeZone":"Europe/Zurich"},
          {"id":"e1","calendarId":"c1","summary":"Standup","allDay":false,"start":"2026-10-12T07:00:00Z","end":"2026-10-12T07:15:00Z","startLocal":"2026-10-12T09:00:00","endLocal":"2026-10-12T09:15:00","timeZone":"Europe/Zurich","myStatus":"NEEDS-ACTION"},
          {"id":"e3","calendarId":"c1","summary":"Call New York","location":"Zoom","allDay":false,"start":"2026-10-13T14:00:00Z","end":"2026-10-13T15:00:00Z","startLocal":"2026-10-13T10:00:00","endLocal":"2026-10-13T11:00:00","timeZone":"America/New_York"}
        ]}"#,
    )
}

#[test]
fn the_agenda_by_day() {
    let (base, seen) = scripted(calendar);
    let out = emx(&base)
        .args(["calendar", "agenda", "--days", "3"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    let want = "Mon 2026-10-12\n  09:00-09:15  Standup (not answered)  [Kalender]\n\n\
                Tue 2026-10-13\n  all day until 2026-10-15  Messe  [Kontakt]\n  \
                10:00-11:00 America/New_York  Call New York, Zoom  [Kalender]\n";
    assert_eq!(text, want);
    let seen = seen.lock().unwrap();
    assert!(seen[1].path.contains("&to="), "{}", seen[1].path);
    let out = emx(&base)
        .args(["calendar", "agenda", "--days", "0"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = emx(&base).args(["calendar", "list"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}
