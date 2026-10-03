use crate::error::Error;
use crate::events::Events;
use crate::types::*;
use serde::de::DeserializeOwned;
use std::time::Duration;

/// The longest pause the client waits out by itself before trying a
/// refused call again. A longer one is the caller's to decide on: the
/// error carries it as `retry_after`.
const MAX_WAIT: Duration = Duration::from_secs(5);

/// How long a send may take. The service gives the submission two
/// minutes; a client that gave up earlier would leave the person not
/// knowing whether the message went.
const SEND_TIMEOUT: Duration = Duration::from_secs(150);

/// The API, reached with one token.
///
/// A token acts as the person who made it, within its scopes. Every mail
/// call names an account: `"me"` for the person's own, or the id of a
/// shared mailbox from [`Me::accounts`].
#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
    base: String,
    token: String,
    user_agent: String,
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
        if !base.starts_with("https://")
            && !base.starts_with("http://localhost")
            && !base.starts_with("http://127.0.0.1")
        {
            return Err(Error::Config(format!("the base URL must be https: {base}")));
        }
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(60)))
            .max_redirects(0)
            .build();
        Ok(Client {
            agent: config.into(),
            base,
            token: token.to_string(),
            user_agent: format!("emx-sdk/{}", env!("CARGO_PKG_VERSION")),
        })
    }

    /// Names the program in requests, as `name/version`, in front of the
    /// crate's own name. The service logs it with errors, which helps
    /// when something needs looking into.
    pub fn user_agent(mut self, name: &str) -> Client {
        self.user_agent = format!("{} emx-sdk/{}", name.trim(), env!("CARGO_PKG_VERSION"));
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
    /// match.
    pub fn search(&self, account: &str, query: &str) -> Result<Vec<Message>, Error> {
        let v: Listed<Message> =
            self.get(&format!("/api/accounts/{}/search", seg(account)), &[("q", query)])?;
        Ok(v.messages)
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
        // to begin instead; silence is noticed by `Events` from the missing
        // keepalives, and the body's limit, above the hour the service
        // keeps a stream, only bounds a connection that died unnoticed.
        let resp = self
            .agent
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
        Ok(Events::new(Box::new(resp.into_body().into_reader())))
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
    /// the key it used the first time. At most 200 characters.
    pub fn send_mail_with_key(&self, draft: &Draft, key: &str) -> Result<Sent, Error> {
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
                req = req.config().timeout_global(Some(t)).build();
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

/// A key no other send of this person will use: 128 bits from the
/// randomly seeded hasher of the standard library, with the time and a
/// counter folded in so that two keys made in one instant still differ.
fn idempotency_key() -> String {
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
