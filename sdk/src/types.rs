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
    #[serde(default)]
    pub prefs: BTreeMap<String, serde_json::Value>,
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
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    #[serde(default)]
    pub max_message_bytes: i64,
    #[serde(default)]
    pub daily_recipients: i64,
    /// 0 means unlimited.
    #[serde(default)]
    pub monthly_recipients: i64,
    #[serde(default)]
    pub monthly_used: i64,
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

/// What a webhook receives. `data` depends on `kind`: for
/// `message.received` it carries `message` with the envelope, for
/// `delivery.failed` the recipient, status and reason.
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EventAccount {
    #[serde(default)]
    pub id: String,
}
