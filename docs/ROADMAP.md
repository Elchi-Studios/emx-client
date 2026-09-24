# Roadmap

Rough order. Nothing here is a promise with a date.

## Desktop

- Drafts kept between sessions, through the API's drafts.
- The token in the system keychain instead of a file.
- System notifications for new mail, from the change feed.
- A tray icon with the unread count.
- Several accounts side by side, not one at a time.
- The screener: approve and block from the app.
- German and French, following the web client.

## Command line

- `emx export` once the export endpoint takes tokens.
- Shell completions.
- An `admin` group for the administration calls, once they settle.

## SDK

- An async client behind a feature flag, for programs that already run
  on tokio.
- Ports: the same shape in TypeScript and Go, as their own repositories.

## Not planned

Sealed mailboxes outside the web client, and anything the API does not
offer. See the non-goals in the README.
