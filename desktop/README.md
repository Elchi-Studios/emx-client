# EMX for the desktop

The EMX app for macOS, Windows and Linux: a window on the same account
the web client shows, with the mail kept on the server. Tauri on the
outside, Svelte inside, `emx-sdk` in between.

## Install

Download the installer for your system from the
[latest release](https://github.com/Elchi-Studios/emx-client/releases/latest):

| System | File | Notes |
| --- | --- | --- |
| Windows | `EMX_<version>_x64-setup.exe` | Run it; it installs for your user. |
| macOS | `EMX_<version>_universal.dmg` | Open it, drag EMX to Applications. Intel and Apple silicon. |
| Linux | `EMX_<version>_amd64.AppImage` | `chmod +x` it and run it. |
| Linux (Debian, Ubuntu) | `EMX_<version>_amd64.deb` | `sudo apt install ./EMX_<version>_amd64.deb` |

Then sign in with a token from Settings, Developer API in the web client.
The app updates itself: it looks for a new release when it starts, and
from "Check for updates" in the tray menu, and asks before it installs
anything. A `.deb` install is replaced by a newer release the same way,
as long as the app may write to its own files; if not, install the new
`.deb` by hand.

### Warnings of an unsigned build

There are no code signing certificates yet, so the installers are not
signed, and your system says so. The warning means that the system cannot
tell who made the file; it does not mean the file is bad. Download only
from the releases page of this repository.

- **Windows** shows "Windows protected your PC" (SmartScreen). Choose
  "More info", then "Run anyway".
- **macOS** says the app "cannot be opened because the developer cannot be
  verified", or that it is damaged. Open System Settings, Privacy and
  Security, and choose "Open Anyway" under the message about EMX. If the
  system calls the app damaged, run
  `xattr -dr com.apple.quarantine /Applications/EMX.app` once.
- **Linux** shows nothing for the AppImage or the `.deb`.

Updates are checked separately: each one is signed with the app's updater
key, and the app refuses one that does not match, signed installer or not.
A build from source has no updater key and does not look for updates.

## Build from source

```sh
npm ci --ignore-scripts
npm run tauri dev      # runs the app against the hosted service
npm run tauri build    # a bundle for this system
```

Tauri's prerequisites per system are at
[tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/).
On Linux the tray needs `libayatana-appindicator3-dev` as well.

## Releases

For maintainers. A tag `desktop-v<version>`, with the version in
`src-tauri/tauri.conf.json`, runs `.github/workflows/release.yml`: it
builds the Windows NSIS installer, the macOS universal DMG and the Linux
AppImage and `.deb`, and publishes them as a release with the updater's
`latest.json`. Every pull request builds the same installers, unsigned,
on all three systems.

The workflow signs with what it finds and builds unsigned without it:

| Secret or variable | For |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (secrets), `TAURI_UPDATER_PUBKEY` (variable) | The updater. Make the pair with `npm run tauri signer generate`. Without them, no update files are made and the app does not offer updates. |
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` | Signing the macOS app (Developer ID certificate as base64). |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | Notarizing it as well. |
| `WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD` | Signing the Windows installer (a `.pfx` as base64). |

## How it is put together

- `src-tauri/` is the Rust side. It holds the token in the system's
  keychain (Credential Manager, Keychain, the Secret Service), talks to
  EMX through the SDK, and follows each account's event stream on a
  thread, telling the window when something changed. The window never
  sees the token. A token an earlier version left in `session.json` is
  moved into the keychain at the next start, and the file removed. Where
  there is no keychain (a Linux session without a Secret Service), the
  token stays in that file, readable by the person alone.
- Drafts are saved on the server two seconds after the last keystroke,
  the same drafts the web client keeps, so a message begun here can be
  finished there. Sending or discarding deletes the draft. If the draft
  changed elsewhere meanwhile, the app asks before it replaces it.
- A tray icon shows the unread count of the inboxes (in its tooltip and
  menu, and beside the icon on macOS), and each new message in the inbox
  is a system notification. They are ordinary notifications, so the
  system's Do Not Disturb, Focus or Focus Assist holds them back. Closing
  the window leaves the app running in the tray; "Quit EMX" ends it. A
  desktop without a tray gets a plain window that closes normally.
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
there. A saved draft cannot be reopened in the app yet, only finished in
the web client; a sealed mailbox's drafts are not saved here.

GNU Affero General Public License 3.0.
