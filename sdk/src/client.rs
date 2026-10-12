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

/// The longest pause the client waits out by itself before trying a
/// refused call again. A longer one is the caller's to decide on: the
/// error carries it as `retry_after`.
const MAX_WAIT: Duration = Duration::from_secs(5);

/// How long a send may wait for its answer once the message is uploaded.
/// The service gives the submission two minutes; a client that gave up
/// earlier would leave the person not knowing whether the message went.
const SEND_TIMEOUT: Duration = Duration::from_secs(150);

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
        let v: serde_json::Value = self.send("PATCH", "/api/me/prefs", prefs)?;
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

    /// The message as received, RFC 822 bytes.
    pub fn raw(&self, account: &str, id: &str) -> Result<Vec<u8>, Error> {
        self.bytes(&format!(
            "/api/accounts/{}/messages/{}/raw",
            seg(account),
            seg(id)
        ))
    }

    /// One part of a message, as bytes with its content type.
    pub fn part(&self, account: &str, id: &str, part: &str) -> Result<(Vec<u8>, String), Error> {
        self.bytes_typed(&format!(
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
        let _: serde_json::Value =
            self.send("POST", &format!("/api/accounts/{}/contacts", seg(account)), &body)?;
        Ok(())
    }

    /// Forgets a decision.
    pub fn remove_contact(&self, account: &str, address: &str) -> Result<(), Error> {
        let _: serde_json::Value = self.send(
            "DELETE",
            &format!("/api/accounts/{}/contacts/{}", seg(account), seg(address)),
            &serde_json::Value::Null,
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
    pub fn send_mail_with_key(&self, draft: &Draft, key: &str) -> Result<Sent, Error> {
        if key.is_empty() || key.len() > 200 || !key.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(Error::Config(
                "an idempotency key is 1 to 200 visible ASCII characters, without spaces".into(),
            ));
        }
        self.send_with("POST", "/api/send", draft, Some(key), Some(SEND_TIMEOUT))
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
        self.send("POST", &format!("/api/accounts/{}/webhooks", seg(account)), &body)
    }

    pub fn delete_webhook(&self, id: &str) -> Result<(), Error> {
        let _: serde_json::Value = self.send(
            "DELETE",
            &format!("/api/webhooks/{}", seg(id)),
            &serde_json::Value::Null,
        )?;
        Ok(())
    }

    /// Queues a `ping` event.
    pub fn test_webhook(&self, id: &str) -> Result<(), Error> {
        let _: serde_json::Value = self.send(
            "POST",
            &format!("/api/webhooks/{}/test", seg(id)),
            &serde_json::Value::Null,
        )?;
        Ok(())
    }

    /// Turns a webhook on again after the service turned it off.
    pub fn enable_webhook(&self, id: &str) -> Result<(), Error> {
        let _: serde_json::Value = self.send(
            "POST",
            &format!("/api/webhooks/{}/enable", seg(id)),
            &serde_json::Value::Null,
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
    pub fn call_json(
        &self,
        method: &str,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, Error> {
        self.send(method, path, body)
    }

    // --- The wire ----------------------------------------------------------

    fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, &str)]) -> Result<T, Error> {
        let url = self.url(path, query);
        let resp = self.with_retry(|| {
            self.agent
                .get(&url)
                .header("Authorization", &format!("Bearer {}", self.token))
                .header("Accept", "application/json")
                .header("User-Agent", &self.user_agent)
                .call()
        })?;
        self.decode(resp)
    }

    fn send<B: serde::Serialize, T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: &B,
    ) -> Result<T, Error> {
        self.send_with(method, path, body, None, None)
    }

    /// A call with a JSON body, with an `Idempotency-Key` and a limit of
    /// its own when given.
    fn send_with<B: serde::Serialize, T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: &B,
        key: Option<&str>,
        timeout: Option<Duration>,
    ) -> Result<T, Error> {
        let url = self.url(path, &[]);
        let raw = serde_json::to_string(body)?;
        let resp = self.with_retry(|| {
            let mut req = match method {
                "POST" => self.agent.post(&url),
                "PATCH" => self.agent.patch(&url),
                "PUT" => self.agent.put(&url),
                "DELETE" => self.agent.delete(&url).force_send_body(),
                _ => self.agent.post(&url),
            };
            if let Some(t) = timeout {
                req = req.config().timeout_recv_response(Some(t)).build();
            }
            if let Some(k) = key {
                req = req.header("Idempotency-Key", k);
            }
            req.header("Authorization", &format!("Bearer {}", self.token))
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .header("User-Agent", &self.user_agent)
                .send(raw.as_bytes())
        })?;
        self.decode(resp)
    }

    fn bytes(&self, path: &str) -> Result<Vec<u8>, Error> {
        Ok(self.bytes_typed(path)?.0)
    }

    fn bytes_typed(&self, path: &str) -> Result<(Vec<u8>, String), Error> {
        let url = self.url(path, &[]);
        let resp = self.with_retry(|| {
            self.agent
                .get(&url)
                .header("Authorization", &format!("Bearer {}", self.token))
                .header("User-Agent", &self.user_agent)
                .call()
        })?;
        if resp.status().as_u16() >= 300 {
            return Err(self.refusal(resp));
        }
        let kind = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let data = resp
            .into_body()
            .with_config()
            .limit(256 << 20)
            .read_to_vec()
            .map_err(|e| Error::Transport(e.to_string()))?;
        Ok((data, kind))
    }

    /// Runs a request, and up to twice more after a pause when the refusal
    /// is one that passes: the per-minute rate limit, or a service that is
    /// briefly unavailable (see [`Error::is_retryable`]). Such a request
    /// was refused before it did anything, and a send carries an
    /// `Idempotency-Key` besides, so repeating it cannot do a thing twice.
    /// Any other refusal, a daily sending limit for one, would answer the
    /// same again and is returned at once, and so is a pause longer than
    /// [`MAX_WAIT`]: the error carries it as `retry_after` and the caller
    /// decides whether to wait, instead of hanging without a word.
    fn with_retry<F>(&self, mut f: F) -> Result<ureq::http::Response<ureq::Body>, Error>
    where
        F: FnMut() -> Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    {
        let mut tries = 0;
        loop {
            tries += 1;
            let resp = f().map_err(|e| Error::Transport(e.to_string()))?;
            let status = resp.status().as_u16();
            if status != 429 && status != 503 {
                return Ok(resp);
            }
            let err = self.refusal(resp);
            let wait = match &err {
                Error::Api { retry_after, .. } => Duration::from_secs(retry_after.unwrap_or(2).max(1)),
                _ => MAX_WAIT,
            };
            if tries >= 3 || !err.is_retryable() || wait > MAX_WAIT {
                return Err(err);
            }
            std::thread::sleep(wait);
        }
    }

    fn decode<T: DeserializeOwned>(&self, resp: ureq::http::Response<ureq::Body>) -> Result<T, Error> {
        if resp.status().as_u16() >= 300 {
            return Err(self.refusal(resp));
        }
        let text = resp
            .into_body()
            .with_config()
            .limit(64 << 20)
            .read_to_string()
            .map_err(|e| Error::Transport(e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| Error::Decode(format!("{e}: {}", excerpt(&text))))
    }

    /// Turns a refused answer into the error the API described.
    fn refusal(&self, resp: ureq::http::Response<ureq::Body>) -> Error {
        let status = resp.status().as_u16();
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
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
            return Error::Api {
                status,
                code: "redirect".into(),
                message,
                request_id: None,
                retry_after: None,
            };
        }
        let text = resp
            .into_body()
            .with_config()
            .limit(1 << 20)
            .read_to_string()
            .unwrap_or_default();
        #[derive(serde::Deserialize)]
        struct Wrapped {
            error: Refusal,
        }
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Refusal {
            code: String,
            message: String,
            #[serde(default)]
            request_id: Option<String>,
        }
        match serde_json::from_str::<Wrapped>(&text) {
            Ok(w) => Error::Api {
                status,
                code: w.error.code,
                message: w.error.message,
                request_id: w.error.request_id,
                retry_after,
            },
            Err(_) => Error::Api {
                status,
                code: match status {
                    401 => "unauthorized".into(),
                    403 => "not_permitted".into(),
                    404 => "not_found".into(),
                    429 => "rate_limited".into(),
                    _ => format!("http_{status}"),
                },
                message: if text.trim().is_empty() {
                    format!("HTTP {status}")
                } else {
                    excerpt(&text)
                },
                request_id: None,
                retry_after,
            },
        }
    }

    fn url(&self, path: &str, query: &[(&str, &str)]) -> String {
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
