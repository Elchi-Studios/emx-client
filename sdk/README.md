# emx-sdk

The [EMX](https://emxmail.ch) mail API as Rust types and calls. Blocking,
`Send + Sync`, three dependencies (`ureq`, `serde`, `serde_json`).

```rust
use emx_sdk::{Client, Draft, ME};

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
# Ok::<(), emx_sdk::Error>(())
```

Live updates:

```rust
for event in emx.events(ME)? {
    if let emx_sdk::Event::Change { .. } = event? {
        let changes = emx.changes(ME, modseq)?;
        modseq = changes.modseq;
    }
}
```

Webhooks, on the receiving side:

```rust
let event = emx_sdk::webhook::receive(secret, request.header("X-EMX-Signature"), body)?;
```

Every call is in the API reference at
[docs.elchi.dev/emx-api](https://docs.elchi.dev/emx-api). The service
adds fields over time and never renames or removes one; the types here
ignore what they do not know.

Mozilla Public License 2.0.
