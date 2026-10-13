// SPDX-License-Identifier: MPL-2.0
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The API's records, as the service sends them. Fields are added by the
//! service over time and never renamed or removed, so every struct here
//! ignores what it does not know and treats missing fields as empty.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A person's identity and what they can reach.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub given_name: String,
    #[serde(default)]
    pub family_name: String,
    /// `owner`, `admin` or `member`.
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub operator: bool,
    pub tenant: Tenant,
    /// The person's own account and the shared mailboxes they may open.
    #[serde(default)]
    pub accounts: Vec<Account>,
    /// The addresses the person may send from.
    #[serde(default)]
    pub send_from: Vec<SendFrom>,
    #[serde(default)]
    pub orgs: Vec<Org>,
    #[serde(default)]
    pub limits: Limits,
    /// The person's preferences. Anything but an object, which a client
    /// could have stored by mistake, reads as none, so that it cannot
    /// lock the person out of every program that asks who they are.
    #[serde(default, deserialize_with = "object_or_empty")]
    pub prefs: BTreeMap<String, serde_json::Value>,
    /// The server a mail app is pointed at, for IMAP on 993 and SMTP on
    /// 465, both TLS from the first byte.
    #[serde(default)]
    pub mail_host: String,
}

/// An object as a map, and anything else as an empty one.
fn object_or_empty<'de, D>(d: D) -> Result<BTreeMap<String, serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Object(m) => m.into_iter().collect(),
        _ => BTreeMap::new(),
    })
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tenant {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub plan: String,
    #[serde(default)]
    pub billing_state: String,
    /// Whether the organisation has a logo of its own.
    #[serde(default)]
    pub has_logo: bool,
    /// Set when the organisation cancelled: it is read-only, and every
    /// change is refused with `read_only`, until `purge_after`, when it is
    /// deleted. RFC 3339.
    #[serde(default)]
    pub cancelled_at: Option<String>,
    #[serde(default)]
    pub purge_after: Option<String>,
    /// Set while the subscription has ended: mail is received but nothing
    /// is sent, and the organisation is cancelled at `lapses_at` unless
    /// it is renewed. RFC 3339.
    #[serde(default)]
    pub subscription_ended_at: Option<String>,
    #[serde(default)]
    pub lapses_at: Option<String>,
}

/// A mailbox the person can open: their own, or a shared one.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    /// `user` or `shared`.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub rights: Rights,
    #[serde(default)]
    pub quota_bytes: i64,
    #[serde(default)]
    pub used_bytes: i64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rights {
    #[serde(default)]
    pub own: bool,
    #[serde(default)]
    pub read: bool,
    #[serde(default)]
    pub write: bool,
    #[serde(default)]
    pub delete: bool,
    #[serde(default)]
    pub send_as: bool,
    #[serde(default)]
    pub send_on_behalf: bool,
    #[serde(default)]
    pub manage: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendFrom {
    pub address: String,
    #[serde(default)]
    pub name: String,
    /// `own`, `as` or `on_behalf`.
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub primary: bool,
    /// The numbers list mail from this address is held to, when its
    /// account is a list sender; absent otherwise.
    #[serde(default)]
    pub list_mail: Option<ListLimits>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Org {
    pub tenant_id: String,
    #[serde(default)]
    pub tenant_name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub current: bool,
    /// The person's id in that organisation.
    #[serde(default)]
    pub principal_id: String,
    #[serde(default)]
    pub state: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    /// The largest message, attachments and text as mail encodes them.
    #[serde(default)]
    pub max_message_bytes: i64,
    /// The most recipients of one message, To, Cc and Bcc together; more
    /// is refused with `too_many_recipients`. 0 when the service did not
    /// say.
    #[serde(default)]
    pub max_recipients: i64,
    #[serde(default)]
    pub daily_recipients: i64,
    /// Recipients outside the organisation per hour; more is refused with
    /// `hourly_limit` until enough of the hour's have left it.
    #[serde(default)]
    pub hourly_recipients: i64,
    /// 0 means unlimited.
    #[serde(default)]
    pub monthly_recipients: i64,
    #[serde(default)]
    pub monthly_used: i64,
    /// A list sender's numbers for its list mail; absent for everybody
    /// else.
    #[serde(default)]
    pub list_mail: Option<ListLimits>,
}

/// The sending limits for list mail.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListLimits {
    #[serde(default)]
    pub hourly_recipients: i64,
    #[serde(default)]
    pub daily_recipients: i64,
}

/// A folder.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Mailbox {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub name: String,
    /// `inbox`, `drafts`, `sent`, `archive`, `junk`, `trash`, or empty
    /// for a folder the person made.
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub unseen: i64,
    #[serde(default)]
    pub bytes: i64,
    #[serde(default)]
    pub modseq: i64,
    #[serde(default)]
    pub uid_validity: u32,
}

/// A name and an address.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Address {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub address: String,
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.name.is_empty() {
            write!(f, "{}", self.address)
        } else {
            write!(f, "{} <{}>", self.name, self.address)
        }
    }
}

/// A message as it appears in a list: the envelope, without the body.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub mailbox_id: String,
    #[serde(default)]
    pub thread_id: String,
    /// RFC 3339, UTC.
    pub received_at: String,
    #[serde(default)]
    pub sent_at: Option<String>,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub from: Address,
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    #[serde(default)]
    pub snippet: String,
    #[serde(default)]
    pub has_attachments: bool,
    /// True in a sealed mailbox: the body is ciphertext to the owner's key.
    #[serde(default)]
    pub sealed: bool,
    /// In a sealed mailbox: the subject, people, date and snippet,
    /// encrypted with age to the mailbox's key, base64. The fields above
    /// are empty then.
    #[serde(default)]
    pub envelope: String,
    /// JMAP keywords: `$seen`, `$flagged`, `$answered`, `$draft`,
    /// `$forwarded`, and the person's own without `$`.
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub size: i64,
    /// `pass`, `fail`, `none` or empty.
    #[serde(default)]
    pub dmarc: String,
    #[serde(default)]
    pub spam_score: f32,
    #[serde(default)]
    pub modseq: i64,
}

impl Message {
    pub fn is_seen(&self) -> bool {
        self.keywords.iter().any(|k| k == "$seen")
    }
    pub fn is_flagged(&self) -> bool {
        self.keywords.iter().any(|k| k == "$flagged")
    }
}

/// A message with its body.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FullMessage {
    pub message: Message,
    pub body: Body,
}

/// The body of a message, ready to show: the HTML is sanitised, remote
/// images are held back in `data-src`, inline images point at the part
/// endpoint. In a sealed mailbox only `sealed` and `ciphertext` are set.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Body {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub html: String,
    #[serde(default)]
    pub remote_images: i64,
    /// Who sent the message for the From address, when that is someone
    /// else: "sent by X on behalf of Y".
    #[serde(default)]
    pub sender: Option<Address>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub reply_to: Vec<Address>,
    #[serde(default)]
    pub message_id: String,
    #[serde(default)]
    pub in_reply_to: String,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub list_id: String,
    #[serde(default)]
    pub sealed: bool,
    /// The message as received, encrypted with age to the mailbox's key,
    /// base64. Only the owner's key opens it.
    #[serde(default)]
    pub ciphertext: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// The path to pass to `part`.
    pub part: String,
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub inline: bool,
    #[serde(default)]
    pub content_id: String,
}

/// What changed after a modseq.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Changes {
    #[serde(default)]
    pub updated: Vec<Message>,
    #[serde(default)]
    pub destroyed: Vec<String>,
    /// Keep this and ask again with it.
    pub modseq: i64,
    #[serde(default)]
    pub has_more: bool,
}

/// A decision of the screener.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub address: String,
    #[serde(default)]
    pub name: String,
    /// `approved` or `blocked`.
    pub state: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub created_at: String,
    /// In a sealed mailbox: the name, encrypted with age to the mailbox's
    /// key, base64; `address` is then a token.
    #[serde(default)]
    pub label: String,
}

/// A message to send.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Draft {
    /// An address from `Me::send_from`.
    pub from: String,
    /// Comma separated; names in angle brackets are allowed.
    pub to: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cc: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub bcc: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reply_to: String,
    #[serde(default)]
    pub subject: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// With a text alternative made by the service when `text` is empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub html: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub in_reply_to: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<DraftAttachment>,
    /// Files of messages the person can read, named instead of uploaded,
    /// as a forward attaches them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub forward_parts: Vec<PartRef>,
    /// The fields of list mail: `List-Id`, `List-Unsubscribe`,
    /// `List-Unsubscribe-Post` and `Precedence`, written as given and
    /// covered by the signature. Any other is refused.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

/// An attachment of a message the person can read, to attach without
/// uploading it again.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PartRef {
    /// `me` or an account id from [`Me::accounts`].
    pub account: String,
    pub message: String,
    /// The `part` of one of the message's [`Attachment`]s.
    pub part: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DraftAttachment {
    pub filename: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub content_type: String,
    /// Base64.
    pub data: String,
    /// Makes an inline image the HTML refers to as `cid:`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub content_id: String,
}

/// The answer to a send.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Sent {
    pub sent: bool,
    #[serde(default)]
    pub recipients: i64,
    /// True when the service answered with what it kept under the send's
    /// `Idempotency-Key`: the message went out with an earlier request,
    /// and not again.
    #[serde(default)]
    pub replayed: bool,
}

/// A download: a message as received or one of its parts, with the name
/// and type the service gives it.
#[derive(Debug, Clone, Default)]
pub struct Download {
    pub data: Vec<u8>,
    /// As the service names it in `Content-Disposition`, `filename*` in
    /// UTF-8 before the ASCII `filename`; empty when it names none. Not
    /// safe to save under as it is: see [`Download::file_name`].
    pub filename: String,
    pub content_type: String,
    /// The version of what was sent, quoted; a message's raw source has
    /// the SHA-256 of its bytes.
    pub etag: String,
    /// When it arrived, as HTTP writes a date.
    pub last_modified: String,
}

impl Download {
    /// The name to save it under: [`filename`](Download::filename) made
    /// safe for every file system by [`crate::safe_file_name`], or
    /// `fallback`.
    pub fn file_name(&self, fallback: &str) -> String {
        crate::safe_file_name(&self.filename, fallback)
    }
}

/// A URL of the person's that hears about an account's events.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Webhook {
    pub id: String,
    #[serde(default)]
    pub account_id: String,
    pub url: String,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub disabled_at: Option<String>,
    #[serde(default)]
    pub disabled_reason: String,
    #[serde(default)]
    pub failures: i64,
    #[serde(default)]
    pub last_status: i64,
    #[serde(default)]
    pub last_delivery_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookDelivery {
    pub id: String,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub attempts: i64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub next_at: Option<String>,
    #[serde(default)]
    pub delivered_at: Option<String>,
    #[serde(default)]
    pub failed_at: Option<String>,
    #[serde(default)]
    pub last_status: i64,
    #[serde(default)]
    pub last_error: String,
}

/// A webhook and, once, its signing secret.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NewWebhook {
    pub webhook: Webhook,
    /// `whsec_...`, shown once.
    pub secret: String,
}

/// What a webhook receives. `data` depends on `kind`;
/// [`WebhookEvent::payload`] reads it as the record of its kind.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookEvent {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub account: EventAccount,
    #[serde(default)]
    pub data: serde_json::Value,
}

impl WebhookEvent {
    /// `data` as the record of the event's kind. An event this crate does
    /// not know yet, or data that is not what its kind describes, is
    /// [`Payload::Other`]: a receiver answers it with a 2xx all the same,
    /// or the service tries it again.
    pub fn payload(&self) -> Payload {
        fn read<T: serde::de::DeserializeOwned>(v: &serde_json::Value) -> Option<T> {
            serde_json::from_value(v.clone()).ok()
        }
        let typed = match self.kind.as_str() {
            crate::webhook::MESSAGE_RECEIVED => read(&self.data).map(Payload::MessageReceived),
            crate::webhook::DELIVERY_FAILED => read(&self.data).map(Payload::DeliveryFailed),
            crate::webhook::COMPLAINT_RECEIVED => read(&self.data).map(Payload::ComplaintReceived),
            crate::webhook::PING => read(&self.data).map(Payload::Ping),
            _ => None,
        };
        typed.unwrap_or(Payload::Other)
    }
}

/// The data of a webhook event, by its kind.
#[derive(Debug, Clone)]
pub enum Payload {
    /// `message.received`: a message was filed in the account, in the
    /// inbox, Junk, the screener's folder, Archive or a folder a rule
    /// chose. Not a Sent copy, a draft, a mail app's upload, an import,
    /// nor mail filed in Trash.
    MessageReceived(MessageReceived),
    /// `delivery.failed`: a message the account sent could not be
    /// delivered to one recipient. Not a forward or an out-of-office
    /// reply the service sent for the account.
    DeliveryFailed(DeliveryFailed),
    /// `complaint.received`: an abuse report (RFC 5965) arrived in the
    /// account, one event for each recipient it names.
    ComplaintReceived(ComplaintReceived),
    /// `ping`: Test was pressed.
    Ping(Ping),
    /// An event of a kind this crate does not know, or data it cannot
    /// read; `WebhookEvent::data` has it as it came.
    Other,
}

/// The data of `message.received`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageReceived {
    #[serde(default)]
    pub message: ReceivedMessage,
}

/// The envelope of a message a webhook tells of, never its text. A sealed
/// message's `subject`, `from`, `to`, `cc` and `message_id` are empty.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReceivedMessage {
    pub id: String,
    pub thread_id: String,
    pub mailbox_id: String,
    pub subject: String,
    pub from: Address,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    /// RFC 3339.
    pub received_at: String,
    pub size: i64,
    pub has_attachments: bool,
    pub message_id: String,
    pub sealed: bool,
}

/// The data of `delivery.failed`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeliveryFailed {
    pub queue_message_id: String,
    pub recipient: String,
    /// The enhanced status code, such as `5.1.1`.
    pub status: String,
    pub reason: String,
    pub remote_server: String,
}

/// The data of `complaint.received`: a recipient of a message the account
/// sent complained about it to their mailbox provider, whose report
/// arrived in the account.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ComplaintReceived {
    /// Who complained: the report's `Original-Rcpt-To`, or the To of the
    /// message it encloses; empty when the provider redacted both.
    pub recipient: String,
    /// `abuse`, `fraud`, `virus` or `other`.
    pub feedback_type: String,
    /// The Message-ID of the message complained about, when the report
    /// gives it.
    pub message_id: String,
    /// The envelope sender of that message, when the report gives it.
    pub mail_from: String,
    /// The From address of the report, whose domain vouched for it.
    pub reported_by: String,
    /// The report as filed in the account, to read with the API.
    pub report: ComplaintReport,
}

/// Where a complaint's report was filed.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ComplaintReport {
    /// A message id, for [`crate::Client::message`].
    pub id: String,
    pub mailbox_id: String,
}

/// The data of `ping`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Ping {
    pub message: String,
    pub sent_at: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EventAccount {
    #[serde(default)]
    pub id: String,
}
