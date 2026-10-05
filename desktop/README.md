# EMX for the desktop

The EMX app for macOS, Windows and Linux: a window on the same account
the web client shows, with the mail kept on the server. Tauri on the
outside, Svelte inside, `emx-sdk` in between.

```sh
npm ci --ignore-scripts
npm run tauri dev      # runs the app against the hosted service
npm run tauri build    # a bundle for this system
```

Tauri's prerequisites per system are at
[tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/).
There is no signed installer or download yet; until there is, the app
is built from source as above.

## How it is put together

- `src-tauri/` is the Rust side. It holds the token in the app's data
  folder, readable by the person alone, talks to EMX through the SDK, and
  follows each account's event stream on a thread, telling the window
  when something changed. The window never sees the token.
- `src/` is the window. It loads a folder once and then applies changes
  from the feed, so a message that arrives, is read on the phone or
  filed elsewhere shows up here without a reload.
- Message HTML comes sanitised from the service and is shown in a frame
  with no scripts. Remote images stay out until asked for; inline images
  are fetched through the Rust side.

Keys: `j` `k` move, `e` archive, `#` or `Delete` trash, `u` read or
unread, `s` flag, `r` reply, `a` reply all, `c` new message, `/` search,
`Esc` close. They do nothing while the composer is open. In Trash,
deleting for good asks first, and closing a composer with something
written in it asks before the text is thrown away.

## What is not here yet

Sealed mailboxes open in the web client only, because the key lives
there. Drafts are not saved between sessions. Notifications from the
system tray and a keychain for the token are next.

GNU Affero General Public License 3.0.
