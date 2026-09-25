<div align="center">

```
███████╗███╗   ███╗██╗  ██╗
██╔════╝████╗ ████║╚██╗██╔╝
█████╗  ██╔████╔██║ ╚███╔╝
██╔══╝  ██║╚██╔╝██║ ██╔██╗
███████╗██║ ╚═╝ ██║██╔╝ ██╗
╚══════╝╚═╝     ╚═╝╚═╝  ╚═╝
```

**Business mail, from the terminal and the desktop**

[![CI](https://github.com/Elchi-Studios/emx-client/actions/workflows/ci.yml/badge.svg)](https://github.com/Elchi-Studios/emx-client/actions/workflows/ci.yml)
![Status](https://img.shields.io/badge/status-alpha-orange)
[![Rust](https://img.shields.io/badge/Rust-1.80+-B7410E?logo=rust)](https://www.rust-lang.org)
[![License: MPL-2.0](https://img.shields.io/badge/License-MPL--2.0-blue.svg)](LICENSE)
[![License: AGPL-3.0](https://img.shields.io/badge/Desktop-AGPL--3.0-blue.svg)](desktop/LICENSE)
[![Made by Elchi Studios](https://img.shields.io/badge/made%20by-Elchi%20Studios-8A2BE2)](https://github.com/Elchi-Studios)

</div>

---

## Overview

[EMX](https://emxmail.ch) is hosted business mail by Elchi Studios in Zug.
The API the EMX web client uses is the developer API: everything the
client can do, a program can do with a token. There is no second, smaller
API to keep in step, and no reason a mail client has to be ours.

This repository is the client side, in three parts:

| Part | What it is | License |
| --- | --- | --- |
| [`sdk/`](sdk/) | `emx-sdk`, the API as Rust types and calls | MPL-2.0 |
| [`cli/`](cli/) | `emx`, mail from a terminal or a script | MPL-2.0 |
| [`desktop/`](desktop/) | the app for macOS, Windows and Linux, Tauri and Svelte | AGPL-3.0 |

Anyone can build their own client against the same API with a token from
the web client. This repository is the reference for how, and the API
itself is documented at [docs.elchi.dev/emx-api](https://docs.elchi.dev/emx-api).

## The trick

The service does the work. Search, threading, sanitising HTML, signing
and delivering mail all happen on the server. A client shows what it is
told and asks for what changed since the last modseq it handled:

```
GET /api/accounts/me/changes?since=812
{"updated": [...], "destroyed": [...], "modseq": 830, "hasMore": false}
```

That is why the SDK is a few files, a client stays right when the service
improves, and a message read on the phone shows as read on the desktop a
moment later without anyone reloading anything.

## Quick look

The command line:

```sh
cargo install --path cli
emx login                      # paste a token from Settings, Developer API
emx list
emx read <id>
emx send --to anna@example.ch --subject "Offerte" --text "Gerne." --attach Offerte.pdf
emx watch --exec 'notify-send "New mail"'
emx --json list | jq '.messages[].subject'
```

The SDK, from Rust:

```rust
use emx_sdk::{Client, ME};

let emx = Client::new("emx_...")?;
let inbox = emx.mailboxes(ME)?.into_iter().find(|m| m.role == "inbox").unwrap();
for m in emx.messages(ME, &inbox.id, 20, "")?.items {
    println!("{}  {}", m.from, m.subject);
}
```

The desktop app: `cd desktop && npm ci --ignore-scripts && npm run tauri build`.
Tauri's prerequisites for each system are at
[tauri.app](https://tauri.app/start/prerequisites/).

## Design

- **Few dependencies.** The SDK needs an HTTP client with TLS and JSON,
  and nothing else. The command line adds nothing to that. HMAC and
  SHA-256 for checking webhook signatures are written in the crate and
  tested against the RFC vectors.
- **One token, kept by the person.** A token is made in the web client
  with the scopes it needs and stored in a file only that person can
  read. In the desktop app it stays on the Rust side; the window never
  sees it.
- **Nothing hidden.** Every call is in the API reference, and every call
  here has a test against a server that answers the documented way.
- **The service is unversioned and only adds.** Fields are added, never
  renamed or removed; the types ignore what they do not know.

More in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), the plan in
[docs/ROADMAP.md](docs/ROADMAP.md), the choices in
[docs/DECISIONS.md](docs/DECISIONS.md).

## Non-goals

- Sealed mailboxes in the desktop app or the command line. The key lives
  in the web client, on purpose.
- IMAP. EMX speaks IMAP for the mail apps people already have; this
  repository is for the API.
- A second API surface. Anything the service does not offer cannot be
  added here; open an issue and it can be discussed for the service.

Keeping this list is half the battle.

## Status

Alpha. The SDK covers every call a token can make, the command line does
what the README shows, and the desktop app reads, searches, replies and
follows the account live. Drafts between sessions, a keychain for the
token and system notifications are next.

```sh
cargo test --workspace --exclude emx-desktop
cargo clippy --workspace --all-targets -- -D warnings
```

Issues and pull requests are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md).
Security matters go to the address in [SECURITY.md](SECURITY.md).

## License

The SDK and the command line are under the [Mozilla Public License 2.0](LICENSE).
The desktop app is under the [GNU Affero General Public License 3.0](desktop/LICENSE).
