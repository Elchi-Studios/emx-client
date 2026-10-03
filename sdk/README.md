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
use emx_sdk::webhook;

fn on_webhook(secret: &str, signature: &str, body: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let event = webhook::receive(secret, signature, body)?;
    println!("{} {}", event.kind, event.id);
    Ok(())
}
```

Every call is in the API reference at
[docs.elchi.dev/emx-api](https://docs.elchi.dev/emx-api). The service
adds fields over time and never renames or removes one; the types here
ignore what they do not know.

Mozilla Public License 2.0.
