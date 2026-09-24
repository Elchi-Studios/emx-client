# EMX client

EMX is hosted business mail by [Elchi Studios](https://elchi.dev), Zug.
This repository holds the client side: a Rust SDK for the EMX API, a
command line, and a desktop app for macOS, Windows and Linux.

| Part | What it is | License |
| --- | --- | --- |
| [`sdk/`](sdk/) | `emx-sdk`, the API as Rust types and calls | MPL-2.0 |
| [`cli/`](cli/) | `emx`, mail from a terminal or a script | MPL-2.0 |
| [`desktop/`](desktop/) | the app, Tauri and Svelte | AGPL-3.0-only |

The API is the same one the EMX web client uses, documented at
[docs.elchi.dev/emx-api](https://docs.elchi.dev/emx-api). Anyone can
build their own client against it with a token from the web client, and
this repository is the reference for how.

## Quick start

```sh
cargo install --path cli
emx login          # paste a token from Settings, Developer API
emx list
emx read <id>
emx send --to anna@example.ch --subject "Offerte" --text "Gerne."
emx watch --exec 'notify-send "New mail"'
```

The SDK, from Rust:

```rust
let emx = emx_sdk::Client::new("emx_...")?;
for m in emx.messages("me", &inbox.id, 20, "")?.items {
    println!("{}  {}", m.from, m.subject);
}
```

The desktop app: `cd desktop && npm ci --ignore-scripts && npm run tauri build`.
The prerequisites for Tauri on each system are at
[tauri.app](https://tauri.app/start/prerequisites/).

## Design

- The service does the work. Search, threading, sanitising HTML, signing
  and delivering mail happen on the server; a client shows what it is
  told and asks for changes since the last modseq it handled. That is why
  the SDK is small and a client stays right when the service improves.
- Few dependencies. The SDK needs an HTTP client with TLS and JSON, and
  nothing else; the command line adds nothing to that. HMAC and SHA-256
  for checking webhooks are written in the crate.
- One token, kept by the person. A token is made in the web client with
  the scopes it needs and is stored in a file only that person can read.
  The desktop app keeps it on the Rust side; the window never sees it.
- Nothing hidden. What the client sends and receives is in the API
  reference, and every call here has a test against a server that
  answers the documented way.

## Building and testing

```sh
cargo test --workspace --exclude emx-desktop
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

The desktop crate builds where Tauri's system libraries are installed;
`cargo check -p emx-desktop` needs them too.

## Contributing

Issues and pull requests are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md).
Security matters go to the address in [SECURITY.md](SECURITY.md), not to
the tracker.
