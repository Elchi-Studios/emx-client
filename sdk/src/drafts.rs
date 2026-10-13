// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Drafts saved on the server with versions, messages held there (undo
//! send and send later), and snippets.

use crate::client::{check_key, seg, Repeat, SEND_TIMEOUT};
use crate::error::Error;
use crate::types::*;
use crate::Client;

impl Client {
    // --- Drafts -------------------------------------------------------------

    /// An account's newest 100 drafts not sent, the person's own (`me`)
    /// or a shared mailbox's they may send from.
    pub fn drafts(&self, account: &str) -> Result<Vec<SavedDraft>, Error> {
        #[derive(serde::Deserialize)]
        struct List {
            #[serde(default)]
            drafts: Vec<SavedDraft>,
        }
        let v: List = self.get(&format!("/api/accounts/{}/drafts", seg(account)), &[])?;
        Ok(v.drafts)
    }

    /// One draft; a sent one too, for an hour after, with `sent_at` and
    /// `sent_by`.
    pub fn draft(&self, account: &str, id: &str) -> Result<SavedDraft, Error> {
        let v: One = self.get(&format!("/api/accounts/{}/drafts/{}", seg(account), seg(id)), &[])?;
        Ok(v.draft)
    }

    /// Saves a change of a draft made on `version`; version 0 makes the
    /// draft under `id`, which the caller chooses ([`new_draft_id`]).
    /// A `from` of a shared mailbox the person may send from moves the
    /// draft there, one of their own addresses to their own account: the
    /// answer's `account` says where it is now. A change made on a
    /// version that is no longer the newest comes back as
    /// [`OnVersion::Changed`] with the newer draft.
    pub fn save_draft(
        &self,
        account: &str,
        id: &str,
        version: i64,
        body: &serde_json::Value,
    ) -> Result<OnVersion<SavedDraft>, Error> {
        let raw = serde_json::to_vec(&serde_json::json!({"version": version, "body": body}))?;
        let url = self.url(&format!("/api/accounts/{}/drafts/{}", seg(account), seg(id)), &[]);
        let resp = self.exchange("PUT", &url, Some(&raw), &[], None, Repeat::Safe)?;
        Ok(self.versioned::<One>(resp)?.0.map(|v| v.draft))
    }

    /// Deletes a draft; with a `version`, only at that version, so that a
    /// colleague's change made meanwhile is not thrown away unseen.
    pub fn delete_draft(
        &self,
        account: &str,
        id: &str,
        version: Option<i64>,
    ) -> Result<OnVersion<()>, Error> {
        let v = version.map(|v| v.to_string());
        let q: Vec<(&str, &str)> = v.iter().map(|v| ("version", v.as_str())).collect();
        let url = self.url(&format!("/api/accounts/{}/drafts/{}", seg(account), seg(id)), &q);
        let resp = self.exchange("DELETE", &url, None, &[], None, Repeat::Safe)?;
        Ok(self.versioned::<serde_json::Value>(resp)?.0.map(|_| ()))
    }

    /// Sends a saved draft at `version`, the message being `message` as
    /// [`Client::send_mail`] takes it, at once or held as `hold` says.
    /// The draft is marked sent in the transaction that sends or holds
    /// the message, on that version: of two people sending at once, one
    /// message goes, and the other gets [`OnVersion::Sent`]; nobody sends
    /// past a change made meanwhile ([`OnVersion::Changed`]).
    ///
    /// `key` is the send's `Idempotency-Key`, as for
    /// [`Client::send_mail_with_key`]; a key belongs to one draft. A
    /// refusal is an error as for a send.
    pub fn send_draft(
        &self,
        account: &str,
        id: &str,
        version: i64,
        message: &Draft,
        hold: &Hold,
        key: &str,
    ) -> Result<OnVersion<DraftSent>, Error> {
        check_key(key)?;
        let mut body = serde_json::to_value(hold)?;
        if let Some(m) = body.as_object_mut() {
            m.insert("version".into(), version.into());
            m.insert("message".into(), serde_json::to_value(message)?);
        }
        let raw = serde_json::to_vec(&body)?;
        let url = self.url(
            &format!("/api/accounts/{}/drafts/{}/send", seg(account), seg(id)),
            &[],
        );
        let headers = [("Idempotency-Key", key.to_string())];
        let resp = self.exchange(
            "POST",
            &url,
            Some(&raw),
            &headers,
            Some(SEND_TIMEOUT),
            Repeat::Safe,
        )?;
        let (outcome, replayed) = self.versioned::<DraftSent>(resp)?;
        Ok(match outcome {
            OnVersion::Done(mut sent) => {
                sent.replayed = replayed;
                OnVersion::Done(sent)
            }
            other => other,
        })
    }

    /// Saves `message` as a draft of `account` and sends it held as
    /// `hold` says: the way to send later, or with time to take it back,
    /// in one call. The answer's `outbox` names the held message.
    ///
    /// `key` is the send's `Idempotency-Key`, and names the draft as well:
    /// the same call with the same key, after an answer that was lost,
    /// finds the draft of the first call and its answer instead of
    /// holding the message twice.
    pub fn send_held(
        &self,
        account: &str,
        message: &Draft,
        hold: &Hold,
        key: &str,
    ) -> Result<DraftSent, Error> {
        check_key(key)?;
        let id = self.draft_id_for(key);
        let saved = match self.save_draft(account, &id, 0, &message.draft_body())? {
            OnVersion::Done(d) => d,
            // Saved by an earlier call with this key: sent on the version
            // it was saved as, which the service answers with what that
            // call got, or sends now if it never got that far.
            OnVersion::Changed(d) | OnVersion::Sent(d) => d,
        };
        let account = if saved.account.is_empty() {
            account.to_string()
        } else {
            saved.account.clone()
        };
        let id = if saved.id.is_empty() { id } else { saved.id.clone() };
        match self.send_draft(&account, &id, saved.version, message, hold, key)? {
            OnVersion::Done(sent) => Ok(sent),
            OnVersion::Changed(d) | OnVersion::Sent(d) => Err(Error::Api {
                status: 409,
                code: "sent".into(),
                message: format!(
                    "the draft of this key was sent or changed already{}; use a new key for a new message",
                    d.sent_by.map(|p| format!(" by {}", p.name)).unwrap_or_default()
                ),
                request_id: None,
                retry_after: None,
                replayed: false,
            }),
        }
    }

    /// The draft id [`send_held`](Client::send_held) uses for a key: a
    /// UUID made from the key and the token, so that it is the same for
    /// the same person and key and nobody else's.
    fn draft_id_for(&self, key: &str) -> String {
        let mut h = crate::webhook::Sha256::new();
        h.update(b"emx-sdk held draft\0");
        h.update(self.token().as_bytes());
        h.update(b"\0");
        h.update(key.as_bytes());
        uuid_from(h.finish())
    }

    // --- Held messages --------------------------------------------------------

    /// The account's held and failed messages, the next to go first, each
    /// with its draft in `message`.
    pub fn outbox(&self, account: &str) -> Result<Vec<Held>, Error> {
        #[derive(serde::Deserialize)]
        struct List {
            #[serde(default)]
            outbox: Vec<Held>,
        }
        let v: List = self.get(&format!("/api/accounts/{}/outbox", seg(account)), &[])?;
        Ok(v.outbox)
    }

    /// Takes a held message back before it goes (undo, or a message to
    /// be sent later), or puts away a failed one: its draft is a draft
    /// again, in a new version, and comes back with the files the message
    /// held. `not_found` once it is sent.
    pub fn cancel_held(&self, account: &str, id: &str) -> Result<Cancelled, Error> {
        self.send(
            "POST",
            &format!("/api/accounts/{}/outbox/{}/cancel", seg(account), seg(id)),
            &serde_json::json!({}),
            Repeat::Once,
        )
    }

    /// Moves a held message to another time, RFC 3339, in the future and
    /// within a year; it is held until then from now on. A failed message
    /// is not moved (`not_found`): its draft is sent anew.
    pub fn reschedule_held(&self, account: &str, id: &str, send_at: &str) -> Result<Held, Error> {
        #[derive(serde::Deserialize)]
        struct Answer {
            outbox: Held,
        }
        let v: Answer = self.send(
            "PATCH",
            &format!("/api/accounts/{}/outbox/{}", seg(account), seg(id)),
            &serde_json::json!({"sendAt": send_at}),
            Repeat::Safe,
        )?;
        Ok(v.outbox)
    }

    // --- Snippets -------------------------------------------------------------

    /// An account's snippets: the person's own (`me`), or a shared
    /// mailbox's for those who may write its drafts.
    pub fn snippets(&self, account: &str) -> Result<Snippets, Error> {
        self.get(&format!("/api/accounts/{}/snippets", seg(account)), &[])
    }

    /// Makes or replaces a snippet under `id`, which the caller chooses
    /// ([`new_draft_id`] makes one): a name of letters, digits, `-` and
    /// `_` up to 40, a text up to 16000 characters, at most 200 per
    /// account (`409 too_many`). A sealed mailbox's own are saved sealed
    /// by the web client (`409 sealed` here).
    pub fn save_snippet(&self, account: &str, id: &str, name: &str, text: &str) -> Result<Snippet, Error> {
        #[derive(serde::Deserialize)]
        struct Answer {
            snippet: Snippet,
        }
        let v: Answer = self.send(
            "PUT",
            &format!("/api/accounts/{}/snippets/{}", seg(account), seg(id)),
            &serde_json::json!({"body": {"name": name, "text": text}}),
            Repeat::Safe,
        )?;
        Ok(v.snippet)
    }

    pub fn delete_snippet(&self, account: &str, id: &str) -> Result<(), Error> {
        let _: serde_json::Value = self.send(
            "DELETE",
            &format!("/api/accounts/{}/snippets/{}", seg(account), seg(id)),
            &serde_json::Value::Null,
            Repeat::Safe,
        )?;
        Ok(())
    }

    // --- The wire -------------------------------------------------------------

    /// Decodes an answer that may be a draft's `409`: `conflict` or `sent`
    /// with the draft as it is now.
    fn versioned<T: serde::de::DeserializeOwned>(
        &self,
        resp: ureq::http::Response<ureq::Body>,
    ) -> Result<(OnVersion<T>, bool), Error> {
        if resp.status().as_u16() != 409 {
            let (v, replayed) = self.decode(resp)?;
            return Ok((OnVersion::Done(v), replayed));
        }
        let (err, body) = self.refusal_with(resp);
        let draft = body
            .and_then(|b| b.get("draft").cloned())
            .and_then(|d| serde_json::from_value::<SavedDraft>(d).ok());
        match (err.code(), draft) {
            (Some("conflict"), Some(d)) => Ok((OnVersion::Changed(d), false)),
            (Some("sent"), Some(d)) => Ok((OnVersion::Sent(d), false)),
            _ => Err(err),
        }
    }
}

#[derive(serde::Deserialize)]
struct One {
    draft: SavedDraft,
}

/// A new id for a draft or a snippet, which the caller chooses: a random
/// UUID (version 4), from the randomly seeded hasher of the standard
/// library with the time and a counter folded in.
pub fn new_draft_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::BuildHasher;
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut b = [0u8; 32];
    for (i, half) in b.chunks_mut(8).take(2).enumerate() {
        let v = RandomState::new().hash_one((i, n, nanos, std::process::id()));
        half.copy_from_slice(&v.to_le_bytes());
    }
    uuid_from(b)
}

/// A UUID of version 4 from the first 16 bytes of `b`.
fn uuid_from(b: [u8; 32]) -> String {
    let mut u = [0u8; 16];
    u.copy_from_slice(&b[..16]);
    u[6] = (u[6] & 0x0f) | 0x40;
    u[8] = (u[8] & 0x3f) | 0x80;
    let h: String = u.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}
