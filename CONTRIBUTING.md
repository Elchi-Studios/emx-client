# Contributing

Thank you for looking. A few things keep this repository easy to work in.

## Before a pull request

- `cargo fmt --all --check` and
  `cargo clippy --workspace --exclude emx-desktop --all-targets -- -D warnings` pass.
- `cargo test --workspace --exclude emx-desktop` passes. A change to the
  SDK's calls comes with a test in `sdk/tests/client.rs` against the
  scripted server there.
- For a change to the desktop app: `npm run check` and `npm run build`
  in `desktop/`, then `cargo clippy -p emx-desktop -- -D warnings`, which
  needs Tauri's prerequisites for your system. The app was run once with
  `npm run tauri dev`.

These are the steps CI runs.
- Commits are small and say what changed and why, in plain English.
  Squash the fixups.

## What fits

- Anything the API offers and a client does not yet do. The reference is
  at https://docs.elchi.dev/emx-api. A call the service does not have
  cannot be added here; open an issue and it can be discussed for the
  service.
- Fixes, of course, with the case that showed them.
- Ports of the SDK to other languages are welcome as their own
  repositories; link them in an issue and they are listed in the README.

## What does not

- New dependencies without a reason that a few hundred lines could not
  cover. The SDK has three; the aim is to keep it there.
- Anything that sends a person's mail or token anywhere but to their own
  EMX server.

## Licenses

The SDK and the command line are under the Mozilla Public License 2.0,
the desktop app under the GNU Affero General Public License 3.0. A
contribution is under the license of the part it lands in.
