# emx-sdk

The [EMX](https://emxmail.ch) mail API as Rust types and calls. Blocking,
`Send + Sync`, three dependencies (`ureq`, `serde`, `serde_json`).

It is not on crates.io yet; take it from the repository:

```toml
[dependencies]
emx-sdk = { git = "https://github.com/Elchi-Studios/emx-client" }
```

```rust,no_run
use emx_sdk::{Client, Draft, ME};

fn main() -> Result<(), emx_sdk::Error> {
    let emx = Client::new("emx_...")?;
    let me = emx.me()?;
    let inbox = emx.mailboxes(ME)?.into_iter().find(|m| m.role == "inbox").unwrap();
    for m in emx.messages(ME, &inbox.id, 20, "")?.items {
        println!("{}  {}", m.from, m.subject);
    }
    emx.send_mail(&Draft {
        from: me.send_from[0].address.clone(),
        to: "anna@example.ch".into(),
        subject: "Offerte".into(),
        text: "Gerne.".into(),
        ..Default::default()
    })?;
    Ok(())
}
```

Send later, or with time to take it back: the message is saved as a
draft and held on the server, which sends it when its time comes
whether anybody is signed in or not.

```rust,no_run
use emx_sdk::{idempotency_key, Client, Draft, Hold, ME};

fn send_tomorrow(emx: &Client, draft: &Draft) -> Result<(), emx_sdk::Error> {
    let hold = Hold { send_at: Some("2026-10-14T07:00:00Z".into()), ..Default::default() };
    let sent = emx.send_held(ME, draft, &hold, &idempotency_key())?;
    let held = sent.outbox.expect("held");
    // Changed my mind:
    emx.cancel_held(&held.account, &held.id)?;
    Ok(())
}
```

Drafts of a shared mailbox are written by everyone who may send from
it, with versions: `save_draft`, `delete_draft` and `send_draft` name
the version a change was made on, and answer `OnVersion::Changed` with
the newer draft when a colleague saved meanwhile, or `OnVersion::Sent`
when somebody sent it, instead of overwriting their work.

Calendars: `calendars`, `occurrences` for a range (recurring events
expanded by the service), `event`, `create_event`, `update_event` and
`delete_event` for one occurrence, the following ones or all, and
`invitation` and `answer_invitation` for a message that carries one.

Live updates: the stream says that something changed, and `changes`
says what. A new stream starts by announcing the current modseq, so
one that is not above the last handled needs no call.

```rust,no_run
use emx_sdk::{Client, Event, ME};

fn follow(emx: &Client) -> Result<(), emx_sdk::Error> {
    let mut modseq = emx.mailboxes(ME)?.iter().map(|m| m.modseq).max().unwrap_or(0);
    loop {
        for event in emx.events(ME)? {
            match event {
                Ok(Event::Change { modseq: now }) if now > modseq => {
                    let changes = emx.changes(ME, modseq)?;
                    modseq = changes.modseq;
                    // changes.updated, changes.destroyed
                }
                Ok(_) => {}
                // The connection broke or went silent: open a new stream.
                Err(_) => break,
            }
        }
    }
}
```

Webhooks, on the receiving side, with the secret from `create_webhook`,
the request's `X-EMX-Signature` header and its body as received:

```rust,no_run
use emx_sdk::{webhook, Payload};

fn on_webhook(secret: &str, signature: &str, body: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let event = webhook::receive(secret, signature, body)?;
    match event.payload() {
        Payload::MessageReceived(m) => println!("new: {}", m.message.subject),
        Payload::DeliveryFailed(d) => println!("{} bounced: {}", d.recipient, d.reason),
        // A reader marked a newsletter as spam: take them off the list.
        Payload::ComplaintReceived(c) => println!("complaint from {}", c.recipient),
        Payload::Ping(_) | Payload::Other => {}
    }
    Ok(())
}
```

`X-EMX-Delivery` is the same on every retry of an event, so a receiver
can skip what it handled already. `message.received` does not fire for
mail filed in Trash, and `delivery.failed` only for mail the account
sent itself, not for its forwards or out-of-office replies.

Every call is in the API reference at
[docs.elchi.dev/emx-api](https://docs.elchi.dev/emx-api). The service
adds fields over time and never renames or removes one; the types here
ignore what they do not know.

Mozilla Public License 2.0.
