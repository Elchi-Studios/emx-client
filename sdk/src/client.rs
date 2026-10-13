// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::error::Error;
use crate::events::Events;
use crate::types::*;
use serde::de::DeserializeOwned;
use std::time::Duration;

/// The longest pause the client waits out by itself before making a
/// refused call again, unless [`Client::max_wait`] sets another. The
/// service asks for 5 seconds after a fault while it checks the token
/// and 30 after another passing fault. A longer pause is the caller's to
/// decide on: the error carries it as `retry_after`.
const MAX_WAIT: Duration = Duration::from_secs(30);

/// How long a send may wait for its answer once the message is uploaded.
/// The service gives the submission two minutes; a client that gave up
/// earlier would leave the person not knowing whether the message went.
pub(crate) const SEND_TIMEOUT: Duration = Duration::from_secs(150);

/// How long any other call may wait for its answer to begin.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a connection may take, TLS included.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a body may stand still, up or down, before the call fails.
/// There is no limit for the whole transfer: a large attachment on a slow
/// line takes as long as it takes, as long as it moves.
const BODY_IDLE: Duration = Duration::from_secs(60);

/// The outer bound for a body, far above any transfer that moves.
const BODY_BOUND: Duration = Duration::from_secs(24 * 60 * 60);

/// The API, reached with one token.
///
/// A token acts as the person who made it, within its scopes. Every mail
/// call names an account: `"me"` for the person's own, or the id of a
/// shared mailbox from [`Me::accounts`].
#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
    /// The agent for event streams, whose reads give up after `idle`.
    events_agent: ureq::Agent,
    /// How long an event stream may stay silent; see [`Events`].
    idle: Duration,
    base: String,
    token: String,
    user_agent: String,
    /// The certificates trusted for TLS: the bundled roots, or those
    /// given to [`Client::trust_only`].
    tls: ureq::tls::TlsConfig,
    /// The longest pause waited out before a call is made again.
    max_wait: Duration,
}

/// One page of a list, with the cursor for the next one.
#[derive(Debug, Clone)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Empty at the end.
    pub cursor: String,
}

impl Client {
    /// A client for the hosted service.
    pub fn new(token: &str) -> Result<Client, Error> {
        Client::with_base_url(crate::DEFAULT_BASE_URL, token)
    }

    /// A client for a service at another address, for a self-hosted or a
    /// test instance. `base_url` is scheme and host, without a path.
    pub fn with_base_url(base_url: &str, token: &str) -> Result<Client, Error> {
        let token = token.trim();
        if token.is_empty() {
            return Err(Error::Config(
                "a token is needed; make one under Settings, Developer API".into(),
            ));
        }
        if token.contains(char::is_whitespace) {
            return Err(Error::Config(
                "the token contains whitespace; copy it again".into(),
            ));
        }
        let base = base_url.trim().trim_end_matches('/').to_string();
        if !base.starts_with("https://") && !is_loopback_http(&base) {
            return Err(Error::Config(format!("the base URL must be https: {base}")));
        }
        let tls = ureq::tls::TlsConfig::default();
        Ok(Client {
            agent: agent(BODY_IDLE, &tls),
            events_agent: crate::events::agent(crate::events::IDLE, &tls),
            idle: crate::events::IDLE,
            base,
            token: token.to_string(),
            user_agent: format!("emx-sdk/{}", env!("CARGO_PKG_VERSION")),
            tls,
            max_wait: MAX_WAIT,
        })
    }

    /// The client, trusting only the certificate authorities in `pem`
    /// (one or more `CERTIFICATE` blocks) instead of the bundled Mozilla
    /// roots: for a network whose proxy inspects TLS with a company
    /// authority, or a self-hosted service with a certificate from a
    /// private one. For both kinds of server, put both kinds of authority
    /// in the file, for instance the system's bundle and the company's.
    pub fn trust_only(mut self, pem: &[u8]) -> Result<Client, Error> {
        let mut certs = Vec::new();
        for item in ureq::tls::parse_pem(pem) {
            match item {
                Ok(ureq::tls::PemItem::Certificate(c)) => certs.push(c),
                Ok(_) => {}
                Err(e) => return Err(Error::Config(format!("the certificate file cannot be read: {e}"))),
            }
        }
        if certs.is_empty() {
            return Err(Error::Config(
                "the certificate file holds no certificate (PEM, BEGIN CERTIFICATE)".into(),
            ));
        }
        self.tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::new_with_certs(&certs))
            .build();
        self.agent = agent(BODY_IDLE, &self.tls);
        self.events_agent = crate::events::agent(self.idle, &self.tls);
        Ok(self)
    }

    /// Names the program in requests, as `name/version`, in front of the
    /// crate's own name. The service logs it with errors, which helps
    /// when something needs looking into.
    pub fn user_agent(mut self, name: &str) -> Client {
        self.user_agent = format!("{} emx-sdk/{}", name.trim(), env!("CARGO_PKG_VERSION"));
        self
    }

    /// The longest pause the client waits out by itself, at most twice
    /// per call, before making a refused call again: 30 seconds unless
    /// set here. A refusal that asks for a longer pause is returned at
    /// once, with the pause as [`Error::retry_after`]. Zero makes every
    /// call once and leaves every pause to the caller.
    pub fn max_wait(mut self, wait: Duration) -> Client {
        self.max_wait = wait;
        self
    }

    /// The client with a shorter silence for event streams, for tests
    /// that cannot wait a minute.
    #[cfg(test)]
    pub(crate) fn with_idle(mut self, idle: Duration) -> Client {
        self.events_agent = crate::events::agent(idle, &self.tls);
        self.idle = idle;
        self
    }

    /// The client with a shorter standstill for bodies, for tests.
    #[cfg(test)]
    pub(crate) fn with_body_idle(mut self, idle: Duration) -> Client {
        self.agent = agent(idle, &self.tls);
        self
    }

    /// The base URL in use.
    pub fn base_url(&self) -> &str {
        &self.base
    }

    // --- Me -------------------------------------------------------------

    /// Who the token is, and what it can reach.
    pub fn me(&self) -> Result<Me, Error> {
        self.get("/api/me", &[])
    }

    /// Merges preferences; a `null` removes a key.
    pub fn set_prefs(&self, prefs: &serde_json::Value) -> Result<serde_json::Value, Error> {
        let v: serde_json::Value = self.send("PATCH", "/api/me/prefs", prefs, Repeat::Safe)?;
        Ok(v.get("prefs").cloned().unwrap_or(serde_json::Value::Null))
    }

    // --- Mailboxes --------------------------------------------------------

    /// The folders of an account.
    pub fn mailboxes(&self, account: &str) -> Result<Vec<Mailbox>, Error> {
        let v: Listed<Mailbox> = self.get(&format!("/api/accounts/{}/mailboxes", seg(account)), &[])?;
        Ok(v.mailboxes)
    }

    /// Makes a folder, under `parent` when given.
    pub fn create_mailbox(&self, account: &str, name: &str, parent: Option<&str>) -> Result<Mailbox, Error> {
        let body = serde_json::json!({"name": name, "parentId": parent});
        self.send(
            "POST",
            &format!("/api/accounts/{}/mailboxes", seg(account)),
            &body,
            Repeat::Once,
        )
    }

    // --- Messages ---------------------------------------------------------

    /// One page of a folder, newest first. `limit` is at most 200; pass
    /// the page's cursor for the next one.
    pub fn messages(
        &self,
        account: &str,
        mailbox: &str,
        limit: u32,
        cursor: &str,
    ) -> Result<Page<Message>, Error> {
        let limit = limit.clamp(1, 200).to_string();
        let mut q = vec![("mailbox", mailbox), ("limit", &limit)];
        if !cursor.is_empty() {
            q.push(("cursor", cursor));
        }
        let v: MessagePage = self.get(&format!("/api/accounts/{}/messages", seg(account)), &q)?;
        Ok(Page {
            items: v.messages,
            cursor: v.cursor,
        })
    }

    /// A message with its body.
    pub fn message(&self, account: &str, id: &str) -> Result<FullMessage, Error> {
        self.get(
            &format!("/api/accounts/{}/messages/{}", seg(account), seg(id)),
            &[],
        )
    }

    /// The message as received, RFC 822 bytes; in a sealed mailbox, the
    /// age ciphertext. See [`raw_download`](Client::raw_download) for its
    /// file name.
    pub fn raw(&self, account: &str, id: &str) -> Result<Vec<u8>, Error> {
        self.bytes(&format!(
            "/api/accounts/{}/messages/{}/raw",
            seg(account),
            seg(id)
        ))
    }

    /// The message as received, with the name to save it under: its
    /// subject and `.eml` (`message.eml` without one), or for a sealed
    /// message `sealed-....eml.age`, as the service names it. The bytes
    /// are checked against the SHA-256 the service gives as their
    /// `ETag`, and a transfer that breaks goes on where it broke.
    /// [`Download::file_name`] makes the name safe to save under.
    pub fn raw_download(&self, account: &str, id: &str) -> Result<Download, Error> {
        let mut d = self.download(&format!(
            "/api/accounts/{}/messages/{}/raw",
            seg(account),
            seg(id)
        ))?;
        if d.filename.is_empty() {
            d.filename = "message.eml".into();
        }
        Ok(d)
    }

    /// One part of a message, as bytes with its content type.
    pub fn part(&self, account: &str, id: &str, part: &str) -> Result<(Vec<u8>, String), Error> {
        let d = self.part_download(account, id, part)?;
        Ok((d.data, d.content_type))
    }

    /// One part of a message, with the name the service gives it.
    pub fn part_download(&self, account: &str, id: &str, part: &str) -> Result<Download, Error> {
        self.download(&format!(
            "/api/accounts/{}/messages/{}/parts/{}",
            seg(account),
            seg(id),
            seg(part)
        ))
    }

    /// A conversation, oldest first.
    pub fn thread(&self, account: &str, thread_id: &str) -> Result<Vec<Message>, Error> {
        let v: Listed<Message> = self.get(
            &format!("/api/accounts/{}/threads/{}", seg(account), seg(thread_id)),
            &[],
        )?;
        Ok(v.messages)
    }

    /// Searches subject, people and text; every word must occur, prefixes
    /// match. The first page, up to 50 hits, newest first; see
    /// [`search_page`](Client::search_page) for the rest.
    pub fn search(&self, account: &str, query: &str) -> Result<Vec<Message>, Error> {
        Ok(self.search_page(account, query, "", 50, "")?.items)
    }

    /// One page of a search, newest first: in every folder, or in the
    /// folder with the id `mailbox` when it is not empty. `limit` is at
    /// most 200; pass the page's cursor for the next one.
    pub fn search_page(
        &self,
        account: &str,
        query: &str,
        mailbox: &str,
        limit: u32,
        cursor: &str,
    ) -> Result<Page<Message>, Error> {
        let limit = limit.clamp(1, 200).to_string();
        let mut q = vec![("q", query), ("limit", limit.as_str())];
        if !mailbox.is_empty() {
            q.push(("mailbox", mailbox));
        }
        if !cursor.is_empty() {
            q.push(("cursor", cursor));
        }
        let v: MessagePage = self.get(&format!("/api/accounts/{}/search", seg(account)), &q)?;
        Ok(Page {
            items: v.messages,
            cursor: v.cursor,
        })
    }

    /// Everything that changed after `since`. Keep the returned modseq and
    /// ask again. An error with code `reload` means the state is too old:
    /// load the lists again.
    pub fn changes(&self, account: &str, since: i64) -> Result<Changes, Error> {
        let since = since.to_string();
        self.get(
            &format!("/api/accounts/{}/changes", seg(account)),
            &[("since", &since)],
        )
    }

    /// Adds and removes keywords on messages. Answers with the ids that
    /// changed.
    pub fn keywords(
        &self,
        account: &str,
        ids: &[&str],
        add: &[&str],
        remove: &[&str],
    ) -> Result<Vec<String>, Error> {
        let body = serde_json::json!({"ids": ids, "add": add, "remove": remove});
        let v: Changed = self.send(
            "POST",
            &format!("/api/accounts/{}/messages/keywords", seg(account)),
            &body,
            Repeat::Safe,
        )?;
        Ok(v.changed)
    }

    /// Marks messages seen or unseen.
    pub fn mark_seen(&self, account: &str, ids: &[&str], seen: bool) -> Result<Vec<String>, Error> {
        if seen {
            self.keywords(account, ids, &["$seen"], &[])
        } else {
            self.keywords(account, ids, &[], &["$seen"])
        }
    }

    /// Moves messages to a folder.
    pub fn r#move(&self, account: &str, ids: &[&str], to: &str) -> Result<Vec<String>, Error> {
        let body = serde_json::json!({"ids": ids, "to": to});
        let v: Changed = self.send(
            "POST",
            &format!("/api/accounts/{}/messages/move", seg(account)),
            &body,
            Repeat::Safe,
        )?;
        Ok(v.changed)
    }

    /// Removes messages for good. The web client moves to Trash first;
    /// this is the second step.
    pub fn delete(&self, account: &str, ids: &[&str]) -> Result<Vec<String>, Error> {
        let body = serde_json::json!({"ids": ids});
        let v: Changed = self.send(
            "POST",
            &format!("/api/accounts/{}/messages/delete", seg(account)),
            &body,
            Repeat::Safe,
        )?;
        Ok(v.changed)
    }

    /// Puts messages away until `until` (RFC 3339); they return to the
    /// inbox then, on top and unread.
    pub fn snooze(&self, account: &str, ids: &[&str], until: &str) -> Result<Vec<String>, Error> {
        let body = serde_json::json!({"ids": ids, "until": until});
        let v: Changed = self.send(
            "POST",
            &format!("/api/accounts/{}/messages/snooze", seg(account)),
            &body,
            Repeat::Safe,
        )?;
        Ok(v.changed)
    }

    /// A stream of change events for an account. See [`Events`].
    pub fn events(&self, account: &str) -> Result<Events, Error> {
        let url = format!("{}/api/accounts/{}/events", self.base, seg(account));
        // The agent's limit for a whole call would cut the stream off after
        // a minute. A stream gets limits for connecting and for the answer
        // to begin instead. Silence is noticed by `Events` from the missing
        // keepalives, and the events agent gives up a read after the same
        // time, so the connection of a stream given up is closed and its
        // reader ends. The body's limit, above the hour the service keeps
        // a stream, only bounds a stream that keeps talking.
        let resp = self
            .events_agent
            .get(&url)
            .config()
            .timeout_global(None)
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_recv_body(Some(Duration::from_secs(65 * 60)))
            .build()
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Accept", "text/event-stream")
            .header("User-Agent", &self.user_agent)
            .call()
            .map_err(|e| Error::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(self.refusal(resp));
        }
        Ok(Events::with_idle(
            Box::new(resp.into_body().into_reader()),
            self.idle,
        ))
    }

    // --- The screener ----------------------------------------------------

    /// The screener's decisions; `state` is `approved`, `blocked` or empty
    /// for both.
    pub fn contacts(&self, account: &str, state: &str) -> Result<Vec<Contact>, Error> {
        let q: Vec<(&str, &str)> = if state.is_empty() {
            vec![]
        } else {
            vec![("state", state)]
        };
        let v: Listed<Contact> = self.get(&format!("/api/accounts/{}/contacts", seg(account)), &q)?;
        Ok(v.contacts)
    }

    /// Records a decision. Approving moves the sender's waiting messages
    /// to the inbox; blocking moves them to Trash.
    pub fn set_contact(&self, account: &str, address: &str, state: &str, name: &str) -> Result<(), Error> {
        let body = serde_json::json!({"address": address, "state": state, "name": name});
        let _: serde_json::Value = self.send(
            "POST",
            &format!("/api/accounts/{}/contacts", seg(account)),
            &body,
            Repeat::Safe,
        )?;
        Ok(())
    }

    /// Forgets a decision.
    pub fn remove_contact(&self, account: &str, address: &str) -> Result<(), Error> {
        let _: serde_json::Value = self.send(
            "DELETE",
            &format!("/api/accounts/{}/contacts/{}", seg(account), seg(address)),
            &serde_json::Value::Null,
            Repeat::Safe,
        )?;
        Ok(())
    }

    // --- Sending ----------------------------------------------------------

    /// Sends a message. It is signed, filed in Sent and delivered, the same
    /// path a mail app takes. `from` must be one of [`Me::send_from`].
    ///
    /// The call carries a fresh `Idempotency-Key`, the same on each of its
    /// own attempts, so a send that is tried again after a refusal can
    /// never go out twice.
    pub fn send_mail(&self, draft: &Draft) -> Result<Sent, Error> {
        self.send_mail_with_key(draft, &idempotency_key())
    }

    /// [`send_mail`](Client::send_mail) with a key chosen by the caller.
    /// The service answers the same key from the same person within a day
    /// with the first answer instead of sending again, so a program that
    /// repeats a send itself, after a lost connection or a restart, passes
    /// the key it used the first time; [`idempotency_key`](crate::idempotency_key)
    /// makes one. A key is 1 to 200 visible ASCII characters; any other
    /// is refused before anything is sent, as an empty one would send
    /// without the protection and a longer one is refused by the service.
    ///
    /// A send refused for a passing fault (`503 try_later`: the service
    /// could not check the token, or its virus scanner is away) is made
    /// again with the same key after the pause the service asks for, up
    /// to [`Client::max_wait`]; the service keeps no answer for it, so
    /// the same request runs again and the message goes out once.
    /// `400 too_many_recipients`, `422 refused_content` and the other
    /// refusals below 500 are final for this message: the same request
    /// gets the same answer, replayed under the key
    /// ([`Error::is_replayed`]). The hourly, daily and monthly limits
    /// (`429`) are returned with the exact wait in
    /// [`Error::retry_after`]. A different message with a key used
    /// already is refused with `422 idempotency_mismatch` and nothing is
    /// sent: every message needs a key of its own.
    pub fn send_mail_with_key(&self, draft: &Draft, key: &str) -> Result<Sent, Error> {
        let (mut sent, replayed): (Sent, bool) = self.send_keyed("/api/send", draft, key)?;
        sent.replayed = replayed;
        Ok(sent)
    }

    // --- Webhooks ---------------------------------------------------------

    /// The person's webhooks on every account, and the events there are.
    pub fn webhooks(&self) -> Result<(Vec<Webhook>, Vec<String>), Error> {
        let v: WebhookList = self.get("/api/me/webhooks", &[])?;
        Ok((v.webhooks, v.events))
    }

    /// Adds a webhook on an account. The secret in the answer is shown
    /// once.
    pub fn create_webhook(
        &self,
        account: &str,
        url: &str,
        events: &[&str],
        description: &str,
    ) -> Result<NewWebhook, Error> {
        let body = serde_json::json!({"url": url, "events": events, "description": description});
        self.send(
            "POST",
            &format!("/api/accounts/{}/webhooks", seg(account)),
            &body,
            Repeat::Once,
        )
    }

    pub fn delete_webhook(&self, id: &str) -> Result<(), Error> {
        let _: serde_json::Value = self.send(
            "DELETE",
            &format!("/api/webhooks/{}", seg(id)),
            &serde_json::Value::Null,
            Repeat::Safe,
        )?;
        Ok(())
    }

    /// Queues a `ping` event.
    pub fn test_webhook(&self, id: &str) -> Result<(), Error> {
        let _: serde_json::Value = self.send(
            "POST",
            &format!("/api/webhooks/{}/test", seg(id)),
            &serde_json::Value::Null,
            Repeat::Once,
        )?;
        Ok(())
    }

    /// Turns a webhook on again after the service turned it off.
    pub fn enable_webhook(&self, id: &str) -> Result<(), Error> {
        let _: serde_json::Value = self.send(
            "POST",
            &format!("/api/webhooks/{}/enable", seg(id)),
            &serde_json::Value::Null,
            Repeat::Safe,
        )?;
        Ok(())
    }

    /// Recent deliveries, newest first.
    pub fn webhook_deliveries(&self, id: &str, limit: u32) -> Result<Vec<WebhookDelivery>, Error> {
        let limit = limit.clamp(1, 200).to_string();
        let v: Listed<WebhookDelivery> = self.get(
            &format!("/api/webhooks/{}/deliveries", seg(id)),
            &[("limit", &limit)],
        )?;
        Ok(v.deliveries)
    }

    // --- Administration (scope admin) --------------------------------------

    /// Any GET under the API, decoded as JSON. For the administration
    /// calls, which are many and change with the product; the reference
    /// lists them. `path` starts with `/api/`.
    pub fn get_json(&self, path: &str, query: &[(&str, &str)]) -> Result<serde_json::Value, Error> {
        self.get(path, query)
    }

    /// Any other call under the API with a JSON body, decoded as JSON.
    /// `PUT` and `DELETE` are made again after a `503 try_later`, as HTTP
    /// has them repeatable; `POST` and `PATCH` only after the rate limit.
    pub fn call_json(
        &self,
        method: &str,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, Error> {
        let repeat = match method {
            "PUT" | "DELETE" => Repeat::Safe,
            _ => Repeat::Once,
        };
        self.send(method, path, body, repeat)
    }

    // --- The wire ----------------------------------------------------------

    pub(crate) fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T, Error> {
        let resp = self.exchange("GET", &self.url(path, query), None, &[], None, Repeat::Safe)?;
        Ok(self.decode(resp)?.0)
    }

    /// A call with a JSON body. `repeat` says whether a `503 try_later`
    /// may be answered by making the call again: see [`Repeat`].
    pub(crate) fn send<B: serde::Serialize, T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: &B,
        repeat: Repeat,
    ) -> Result<T, Error> {
        let raw = serde_json::to_vec(body)?;
        // A deletion without a body sends none.
        let body = (method != "DELETE" || raw != b"null").then_some(raw.as_slice());
        let resp = self.exchange(method, &self.url(path, &[]), body, &[], None, repeat)?;
        Ok(self.decode(resp)?.0)
    }

    /// Sends a message with its `Idempotency-Key`, with the longer limit
    /// a send has for its answer, and answers whether the service gave
    /// the answer kept under the key (`Idempotent-Replayed`).
    fn send_keyed<B: serde::Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
        key: &str,
    ) -> Result<(T, bool), Error> {
        check_key(key)?;
        let raw = serde_json::to_vec(body)?;
        let headers = [("Idempotency-Key", key.to_string())];
        let resp = self.exchange(
            "POST",
            &self.url(path, &[]),
            Some(&raw),
            &headers,
            Some(SEND_TIMEOUT),
            Repeat::Safe,
        )?;
        self.decode(resp)
    }

    /// Makes one request and answers its response, whatever its status,
    /// after making it again where a refusal passes: up to twice more,
    /// after the pause the service asks for (`Retry-After`), when that
    /// pause is at most [`Client::max_wait`].
    ///
    /// The per-minute rate limit is answered before the call does
    /// anything, so every call is made again after it. A `503 try_later`
    /// (or a 503 from a proxy in front of the service) is made again
    /// only for a call that may safely be repeated, `repeat` being
    /// [`Repeat::Safe`]: one that reads, one that leaves the same state
    /// however often it is made, and a send with its `Idempotency-Key`,
    /// the same on every attempt, which the service sends once at most.
    /// Any other refusal, a daily sending limit for one, would answer the
    /// same again and is returned at once, and so is a longer pause: the
    /// error carries it as `retry_after` and the caller decides whether
    /// to wait, instead of hanging without a word.
    pub(crate) fn exchange(
        &self,
        method: &str,
        url: &str,
        body: Option<&[u8]>,
        headers: &[(&str, String)],
        timeout: Option<Duration>,
        repeat: Repeat,
    ) -> Result<ureq::http::Response<ureq::Body>, Error> {
        let mut tries = 0;
        loop {
            tries += 1;
            let resp = self
                .request(method, url, body, headers, timeout)?
                .map_err(|e| Error::Transport(e.to_string()))?;
            let status = resp.status().as_u16();
            if status != 429 && status != 503 {
                return Ok(resp);
            }
            let err = self.refusal(resp);
            let again = match &err {
                Error::Api {
                    status: 429, code, ..
                } => code == "rate_limited",
                Error::Api {
                    status: 503, code, ..
                } => repeat == Repeat::Safe && (code == "try_later" || code == "http_503"),
                _ => false,
            };
            let wait = Duration::from_secs(err.retry_after().unwrap_or(2).max(1));
            if tries >= 3 || !again || wait > self.max_wait {
                return Err(err);
            }
            std::thread::sleep(wait);
        }
    }

    /// One request, with the token and the headers every call carries.
    /// The outer error is a method the client does not make.
    fn request(
        &self,
        method: &str,
        url: &str,
        body: Option<&[u8]>,
        headers: &[(&str, String)],
        timeout: Option<Duration>,
    ) -> Result<Result<ureq::http::Response<ureq::Body>, ureq::Error>, Error> {
        let auth = format!("Bearer {}", self.token);
        macro_rules! dress {
            ($req:expr) => {{
                let mut req = $req;
                if let Some(t) = timeout {
                    req = req.config().timeout_recv_response(Some(t)).build();
                }
                req = req
                    .header("Authorization", &auth)
                    .header("User-Agent", &self.user_agent);
                for (name, value) in headers {
                    req = req.header(*name, value);
                }
                req
            }};
        }
        Ok(match (method, body) {
            ("GET", _) => dress!(self.agent.get(url))
                .header("Accept", "application/json")
                .call(),
            ("DELETE", None) => dress!(self.agent.delete(url))
                .header("Accept", "application/json")
                .call(),
            (m, body) => {
                let req = match m {
                    "POST" => self.agent.post(url),
                    "PATCH" => self.agent.patch(url),
                    "PUT" => self.agent.put(url),
                    "DELETE" => self.agent.delete(url).force_send_body(),
                    other => {
                        return Err(Error::Config(format!(
                            "{other} is not a method of the API; use GET, POST, PATCH, PUT or DELETE"
                        )))
                    }
                };
                dress!(req)
                    .header("Accept", "application/json")
                    .header("Content-Type", "application/json")
                    .send(body.unwrap_or(b"null"))
            }
        })
    }

    fn bytes(&self, path: &str) -> Result<Vec<u8>, Error> {
        Ok(self.download(path)?.data)
    }

    /// A download: the bytes, with the name and type the service gives
    /// them. A transfer that breaks after its first bytes goes on from
    /// where it broke, as long as the service names the version of what
    /// it sends (`ETag`) and still has that version (`If-Range`); a
    /// download whose `ETag` is the SHA-256 of its bytes, as a message's
    /// raw source is, is checked against it.
    fn download(&self, path: &str) -> Result<Download, Error> {
        let url = self.url(path, &[]);
        let resp = self.exchange("GET", &url, None, &[], None, Repeat::Safe)?;
        if resp.status().as_u16() >= 300 {
            return Err(self.refusal(resp));
        }
        let header = |resp: &ureq::http::Response<ureq::Body>, name: &str| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let mut out = Download {
            data: Vec::new(),
            filename: disposition_filename(&header(&resp, "content-disposition")).unwrap_or_default(),
            content_type: header(&resp, "content-type"),
            etag: header(&resp, "etag"),
            last_modified: header(&resp, "last-modified"),
        };
        let mut resp = resp;
        let mut resumes = 0;
        loop {
            let mut reader = resp
                .into_body()
                .into_with_config()
                .limit(DOWNLOAD_LIMIT.saturating_sub(out.data.len() as u64))
                .reader();
            let read = std::io::copy(&mut reader, &mut out.data);
            if read.is_err() && out.data.len() as u64 >= DOWNLOAD_LIMIT {
                return Err(Error::Transport(format!(
                    "the download is larger than {} MB, the most taken",
                    DOWNLOAD_LIMIT >> 20
                )));
            }
            match read {
                Ok(_) => break,
                // Broken on the way: asked again for the rest, of the same
                // version only. A weak ETag cannot ask for a range.
                Err(e) if resumes < 3 && !out.data.is_empty() && out.etag.starts_with('"') => {
                    resumes += 1;
                    let from = out.data.len();
                    let headers = [
                        ("Range", format!("bytes={from}-")),
                        ("If-Range", out.etag.clone()),
                    ];
                    let next = self
                        .exchange("GET", &url, None, &headers, None, Repeat::Safe)
                        .map_err(|_| Error::Transport(e.to_string()))?;
                    match next.status().as_u16() {
                        206 if header(&next, "content-range").starts_with(&format!("bytes {from}-")) => {}
                        // The whole of it again: the version changed, or
                        // the service does not take ranges here.
                        200 => {
                            out.data.clear();
                            out.etag = header(&next, "etag");
                        }
                        s if s >= 300 => return Err(self.refusal(next)),
                        _ => return Err(Error::Transport(e.to_string())),
                    }
                    resp = next;
                }
                Err(e) => return Err(Error::Transport(e.to_string())),
            }
        }
        if let Some(want) = sha256_etag(&out.etag) {
            if crate::webhook::sha256(&out.data) != want {
                return Err(Error::Transport(format!(
                    "the download arrived damaged: {} bytes that are not the ones EMX sent",
                    out.data.len()
                )));
            }
        }
        Ok(out)
    }

    /// Decodes a JSON answer, or turns a refusal into its error. Answers
    /// whether the service replayed an answer kept under an
    /// `Idempotency-Key`.
    pub(crate) fn decode<T: DeserializeOwned>(
        &self,
        resp: ureq::http::Response<ureq::Body>,
    ) -> Result<(T, bool), Error> {
        if resp.status().as_u16() >= 300 {
            return Err(self.refusal(resp));
        }
        let replayed = replayed(&resp);
        let text = resp
            .into_body()
            .with_config()
            .limit(64 << 20)
            .read_to_string()
            .map_err(|e| Error::Transport(e.to_string()))?;
        let v = serde_json::from_str(&text).map_err(|e| Error::Decode(format!("{e}: {}", excerpt(&text))))?;
        Ok((v, replayed))
    }

    /// Turns a refused answer into the error the API described.
    fn refusal(&self, resp: ureq::http::Response<ureq::Body>) -> Error {
        self.refusal_with(resp).0
    }

    /// [`refusal`](Client::refusal), and the answer's JSON body when it
    /// had one, for a refusal that carries more than the error, as a
    /// draft's `409` carries the draft.
    pub(crate) fn refusal_with(
        &self,
        resp: ureq::http::Response<ureq::Body>,
    ) -> (Error, Option<serde_json::Value>) {
        let status = resp.status().as_u16();
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        let replayed = replayed(&resp);
        // A redirect is not followed, so that the token never goes to
        // another address; the error names where it pointed, which is
        // usually the base URL to use instead.
        if (300..400).contains(&status) {
            let to = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .map(|v| excerpt(v.trim()))
                .unwrap_or_default();
            let message = if to.is_empty() {
                format!(
                    "{} answers with a redirect; is it the address of the EMX service?",
                    self.base
                )
            } else {
                format!(
                    "{} sends requests on to {to}; use that address (scheme and host) as the base URL",
                    self.base
                )
            };
            let e = Error::Api {
                status,
                code: "redirect".into(),
                message,
                request_id: None,
                retry_after: None,
                replayed: false,
            };
            return (e, None);
        }
        let allow = resp
            .headers()
            .get("allow")
            .and_then(|v| v.to_str().ok())
            .map(|v| excerpt(v.trim()))
            .unwrap_or_default();
        let text = resp
            .into_body()
            .with_config()
            .limit(4 << 20)
            .read_to_string()
            .unwrap_or_default();
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Refusal {
            code: String,
            message: String,
            #[serde(default)]
            request_id: Option<String>,
        }
        let body: Option<serde_json::Value> = serde_json::from_str(&text).ok();
        let refusal = body
            .as_ref()
            .and_then(|b| b.get("error"))
            .and_then(|e| serde_json::from_value::<Refusal>(e.clone()).ok());
        let e = match refusal {
            Some(r) => Error::Api {
                status,
                code: r.code,
                message: r.message,
                request_id: r.request_id,
                retry_after,
                replayed,
            },
            None => Error::Api {
                status,
                code: match status {
                    401 => "unauthorized".into(),
                    403 => "not_permitted".into(),
                    404 => "not_found".into(),
                    405 => "method_not_allowed".into(),
                    429 => "rate_limited".into(),
                    _ => format!("http_{status}"),
                },
                message: if status == 405 && !allow.is_empty() {
                    format!("the method is not allowed here; use {allow}")
                } else if text.trim().is_empty() {
                    format!("HTTP {status}")
                } else {
                    excerpt(&text)
                },
                request_id: None,
                retry_after,
                replayed,
            },
        };
        (e, body)
    }

    pub(crate) fn url(&self, path: &str, query: &[(&str, &str)]) -> String {
        let mut url = format!("{}{}", self.base, path);
        let mut first = true;
        for (k, v) in query {
            url.push(if first { '?' } else { '&' });
            first = false;
            url.push_str(&encode(k));
            url.push('=');
            url.push_str(&encode(v));
        }
        url
    }
}

/// Whether a failed call may be made again after a `503 try_later`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Repeat {
    /// Made again: a read, a change that leaves the same state however
    /// often it is made (flags, a move, a deletion, a save of a whole
    /// record under its own id), or a send with its `Idempotency-Key`.
    Safe,
    /// Made again only after the per-minute rate limit, which refuses
    /// before anything is done: a call that makes something new, or
    /// that does something each time it is made, such as a test ping.
    Once,
}

/// The largest download taken, a message or an attachment.
const DOWNLOAD_LIMIT: u64 = 256 << 20;

/// Whether the service answered with what it kept under an
/// `Idempotency-Key`.
fn replayed(resp: &ureq::http::Response<ureq::Body>) -> bool {
    resp.headers()
        .get("idempotent-replayed")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("true"))
}

/// The SHA-256 an `ETag` names, when it is one: a strong tag of 64 hex
/// digits, as the service gives a message's raw source.
fn sha256_etag(etag: &str) -> Option<[u8; 32]> {
    let hex = etag.strip_prefix('"')?.strip_suffix('"')?;
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// A key the service takes: 1 to 200 visible ASCII characters. Any other
/// is refused before anything is sent, as an empty one would send without
/// the protection and a longer one is refused by the service.
pub(crate) fn check_key(key: &str) -> Result<(), Error> {
    if key.is_empty() || key.len() > 200 || !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Error::Config(
            "an idempotency key is 1 to 200 visible ASCII characters, without spaces".into(),
        ));
    }
    Ok(())
}

/// The file name a `Content-Disposition` gives: `filename*` as RFC 8187
/// encodes it, in UTF-8 or ISO-8859-1, before the plain `filename`,
/// which older programs read. None when it names none.
pub fn disposition_filename(header: &str) -> Option<String> {
    let mut plain = None;
    let mut extended = None;
    for (name, value) in disposition_params(header) {
        match name.to_ascii_lowercase().as_str() {
            "filename*" if extended.is_none() => extended = decode_ext_value(&value),
            "filename" if plain.is_none() => plain = Some(value),
            _ => {}
        }
    }
    extended.or(plain).filter(|n| !n.trim().is_empty())
}

/// The parameters after the disposition type, each name with its value,
/// quoted strings unquoted and their escapes undone.
fn disposition_params(header: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = header.chars().peekable();
    // The type itself, up to the first semicolon.
    for c in chars.by_ref() {
        if c == ';' {
            break;
        }
    }
    loop {
        let mut name = String::new();
        for c in chars.by_ref() {
            if c == '=' || c == ';' {
                break;
            }
            name.push(c);
        }
        let name = name.trim().to_string();
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let mut value = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        if let Some(n) = chars.next() {
                            value.push(n);
                        }
                    }
                    '"' => break,
                    c => value.push(c),
                }
            }
            for c in chars.by_ref() {
                if c == ';' {
                    break;
                }
            }
        } else {
            for c in chars.by_ref() {
                if c == ';' {
                    break;
                }
                value.push(c);
            }
            value = value.trim().to_string();
        }
        if !name.is_empty() {
            out.push((name, value));
        }
        if chars.peek().is_none() {
            return out;
        }
    }
}

/// An RFC 8187 `ext-value`: `charset'language'percent-encoded`.
fn decode_ext_value(v: &str) -> Option<String> {
    let mut parts = v.splitn(3, '\'');
    let charset = parts.next()?.trim().to_ascii_lowercase();
    let _language = parts.next()?;
    let encoded = parts.next()?;
    let mut bytes = Vec::with_capacity(encoded.len());
    let raw = encoded.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' {
            let hex = encoded.get(i + 1..i + 3)?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            bytes.push(raw[i]);
            i += 1;
        }
    }
    match charset.as_str() {
        "utf-8" => String::from_utf8(bytes).ok(),
        "iso-8859-1" => Some(bytes.iter().map(|&b| b as char).collect()),
        _ => None,
    }
}

/// The longest file name [`safe_file_name`] keeps, in bytes: file systems
/// allow 255, and room is left for the ` (2)` of a name that is taken.
pub const FILE_NAME_MAX: usize = 200;

/// A name to save a file under, made from one the service or a message
/// gave: nothing that could leave the folder or that a file system
/// refuses. No separators, control or direction characters, or
/// characters Windows refuses; no name Windows keeps for a device (`CON`,
/// `NUL.pdf`); no leading or trailing dots; at most [`FILE_NAME_MAX`]
/// bytes, shortened in front of the extension. `fallback` when nothing is
/// left.
pub fn safe_file_name(name: &str, fallback: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !is_direction_mark(*c))
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut cleaned = cleaned.trim().trim_matches('.').trim().to_string();
    if cleaned.is_empty() {
        return fallback.to_string();
    }
    let stem = cleaned
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ((stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.chars().count() == 4
        && stem
            .chars()
            .nth(3)
            .is_some_and(|c| c.is_ascii_digit() || "\u{b9}\u{b2}\u{b3}".contains(c)));
    if reserved {
        cleaned.insert(0, '_');
    }
    shorten(&cleaned, FILE_NAME_MAX)
}

/// The characters that turn the direction of text around, which can make
/// `fdp.exe` read as `exe.pdf`.
fn is_direction_mark(c: char) -> bool {
    matches!(c, '\u{200e}' | '\u{200f}' | '\u{061c}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// `name` in at most `max` bytes, cut on a character boundary in front
/// of a short extension, which stays.
fn shorten(name: &str, max: usize) -> String {
    if name.len() <= max {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && e.len() <= 16 && !e.contains(' ') => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    let mut cut = max.saturating_sub(ext.len());
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{ext}", stem[..cut].trim_end())
}

#[derive(serde::Deserialize)]
struct Listed<T> {
    #[serde(default = "Vec::new")]
    mailboxes: Vec<T>,
    #[serde(default = "Vec::new")]
    messages: Vec<T>,
    #[serde(default = "Vec::new")]
    contacts: Vec<T>,
    #[serde(default = "Vec::new")]
    deliveries: Vec<T>,
}

#[derive(serde::Deserialize)]
struct MessagePage {
    #[serde(default)]
    messages: Vec<Message>,
    #[serde(default)]
    cursor: String,
}

#[derive(serde::Deserialize)]
struct Changed {
    #[serde(default)]
    changed: Vec<String>,
}

#[derive(serde::Deserialize)]
struct WebhookList {
    #[serde(default)]
    webhooks: Vec<Webhook>,
    #[serde(default)]
    events: Vec<String>,
}

/// The agent for calls other than event streams: limits for connecting
/// and for the answer to begin, and `body_idle` for a body standing
/// still, in place of a limit for the whole call.
fn agent(body_idle: Duration, tls: &ureq::tls::TlsConfig) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .tls_config(tls.clone())
        .http_status_as_error(false)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_send_request(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(ANSWER_TIMEOUT))
        // Named, so that a read or write of a body is known as one and
        // gets `body_idle`; the bound itself is only there to be named.
        .timeout_send_body(Some(BODY_BOUND))
        .timeout_recv_body(Some(BODY_BOUND))
        .max_redirects(0)
        .build();
    crate::events::body_idle_agent(config, body_idle)
}

/// A new key for [`Client::send_mail_with_key`], which no other send
/// will use: 128 bits from the randomly seeded hasher of the standard
/// library, with the time and a counter folded in so that two keys made
/// in one instant still differ. A program that may repeat a send makes
/// one key per message and keeps it until the message is sent.
pub fn idempotency_key() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::BuildHasher;
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut parts = [0u64; 2];
    for (i, part) in parts.iter_mut().enumerate() {
        *part = RandomState::new().hash_one((i, n, nanos, std::process::id()));
    }
    format!("emx-{:016x}{:016x}", parts[0], parts[1])
}

/// True for a plain-HTTP URL whose host is this computer: `localhost`,
/// `127.0.0.1` or `[::1]`, with a port or without. The whole host is
/// compared, so `http://localhost.example.ch` is not one of them: the
/// token would cross the network in clear text.
fn is_loopback_http(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    // The host, and what follows it: nothing, or a colon and the port.
    let split = if authority.starts_with('[') {
        authority.find(']').map(|i| i + 1)
    } else {
        Some(authority.find(':').unwrap_or(authority.len()))
    };
    let Some((host, port)) = split.map(|i| authority.split_at(i)) else {
        return false;
    };
    let port_ok = match port.strip_prefix(':') {
        Some(p) => !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()),
        None => port.is_empty(),
    };
    port_ok && (host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "[::1]")
}

/// A path segment, percent-encoded.
pub(crate) fn seg(s: &str) -> String {
    encode(s)
}

/// RFC 3986 percent-encoding of everything but the unreserved characters.
pub(crate) fn encode(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => {
                out.push('%');
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 15) as usize] as char);
            }
        }
    }
    out
}

fn excerpt(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() > 200 {
        let cut: String = s.chars().take(200).collect();
        format!("{cut}...")
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Event;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// A certificate authority made for the tests, and nothing else.
    const TEST_CA: &str = "-----BEGIN CERTIFICATE-----\nMIIBgTCCASegAwIBAgIULVIfdk0OEI414Fv207B7NqYaVeEwCgYIKoZIzj0EAwIw\nFjEUMBIGA1UEAwwLRU1YIFRlc3QgQ0EwHhcNMjYxMDEyMDgxNDI4WhcNMzYxMDA5\nMDgxNDI4WjAWMRQwEgYDVQQDDAtFTVggVGVzdCBDQTBZMBMGByqGSM49AgEGCCqG\nSM49AwEHA0IABMQxApKR6BMyzrmJpzjCFgK3LCuZMaQs5csNtnRWvv3pVE0MUG9n\nGe0qFY4ZWqK/qudTY7pmeoDjXto5OVkICTujUzBRMB0GA1UdDgQWBBTJgvbUII3J\n1F1sN6YXHG6pQ/Bz0zAfBgNVHSMEGDAWgBTJgvbUII3J1F1sN6YXHG6pQ/Bz0zAP\nBgNVHRMBAf8EBTADAQH/MAoGCCqGSM49BAMCA0gAMEUCIHem6PpvSYs0oiiHhBbb\nNvLomSPGnYv/7xMNW9eMveNjAiEAlWnH/COIXNA8uLl61XwH9wBrR0xaJILOzAEp\nh44NOJQ=\n-----END CERTIFICATE-----\n";

    #[test]
    fn trust_only_takes_certificates_and_refuses_anything_else() {
        let c = || Client::with_base_url("https://mail.example.ch", "emx_test").unwrap();
        assert!(c().trust_only(TEST_CA.as_bytes()).is_ok());
        let two = format!("{TEST_CA}{TEST_CA}");
        assert!(c().trust_only(two.as_bytes()).is_ok());
        for bad in [
            "",
            "not a certificate",
            "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n",
        ] {
            let e = c().trust_only(bad.as_bytes()).err().expect(bad);
            assert!(matches!(e, Error::Config(_)), "{e}");
        }
    }

    /// A server that answers with a body of `chunks` pieces, `pause`
    /// apart.
    fn slow_body(chunks: usize, pause: Duration) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 && line != "\r\n" {
                line.clear();
            }
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/pdf\r\nContent-Length: {chunks}\r\n\r\n"
            );
            for _ in 0..chunks {
                std::thread::sleep(pause);
                if stream.write_all(b"x").and_then(|_| stream.flush()).is_err() {
                    return;
                }
            }
        });
        base
    }

    /// A download that keeps moving is not cut off, however long it takes
    /// as a whole; one that stands still is.
    #[test]
    fn a_body_is_limited_by_standstill_not_by_its_length() {
        let idle = Duration::from_millis(400);
        let base = slow_body(8, Duration::from_millis(150));
        let emx = Client::with_base_url(&base, "emx_test")
            .unwrap()
            .with_body_idle(idle);
        let started = std::time::Instant::now();
        let (data, _) = emx.part("me", "x1", "2").unwrap();
        assert_eq!(data.len(), 8);
        assert!(started.elapsed() > idle * 2, "took {:?}", started.elapsed());

        let base = slow_body(2, Duration::from_secs(3));
        let emx = Client::with_base_url(&base, "emx_test")
            .unwrap()
            .with_body_idle(idle);
        let started = std::time::Instant::now();
        assert!(matches!(emx.part("me", "x1", "2"), Err(Error::Transport(_))));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "took {:?}",
            started.elapsed()
        );
    }

    /// A stream that announces its modseq and then says nothing, the way
    /// one does after the network went away without a word. Once the
    /// stream is given up, its connection must close: a reader left
    /// waiting on it would keep the socket and a thread for an hour,
    /// after every reconnect.
    #[test]
    fn a_silent_stream_closes_its_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (tx, closed) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 && line != "\r\n" {
                line.clear();
            }
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n\
                      event: change\ndata: {\"modseq\": 5}\n\n",
                )
                .unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let gone = match stream.read(&mut [0u8; 1]) {
                Ok(n) => n == 0,
                Err(e) => !matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ),
            };
            let _ = tx.send(gone);
        });
        let emx = Client::with_base_url(&base, "emx_test")
            .unwrap()
            .with_idle(Duration::from_millis(300));
        let mut events = emx.events("me").unwrap();
        assert_eq!(events.next().unwrap().unwrap(), Event::Change { modseq: 5 });
        assert!(events.next().unwrap().is_err(), "silence ends the stream");
        drop(events);
        assert!(
            closed.recv_timeout(Duration::from_secs(20)).unwrap(),
            "the connection of the stream given up stayed open"
        );
    }
}
