# Changelog

## Unreleased

- The SDK reaches the drafts saved on the server, a shared mailbox's
  among them, with their versions (`drafts`, `draft`, `save_draft`,
  `delete_draft`, `send_draft`), where a change on an older version or
  of a draft sent meanwhile answers `OnVersion::Changed` or
  `OnVersion::Sent` with the draft as it is; holds a message on the
  server for undo send or until a time (`Hold`, `send_held`), lists the
  held and failed ones (`outbox`), moves and takes them back
  (`reschedule_held`, `cancel_held`, which gives the draft and its
  files back); and reads and keeps snippets (`snippets`,
  `save_snippet`, `delete_snippet`).
- Webhooks know `complaint.received`, the event for an abuse report
  (RFC 5965) about a message the account sent: the SDK reads it, with
  `message.received`, `delivery.failed` and `ping`, as
  `WebhookEvent::payload`, and names the events in
  `webhook::EVENTS`. `emx webhook add` asks for `--events`, lists the
  three in its help, and takes `--events all` for every event the
  service offers; without `--events` the service refused the hook. The
  documentation says that `message.received` no longer fires for mail
  filed in Trash, nor `delivery.failed` for forwards and out-of-office
  replies.
- `emx send` reports a send refused with `503 try_later` after its
  attempts as not sent, with status 4, instead of one that may have gone
  out; a limit says when the message can go ("can go in 29 minutes"),
  `idempotency_mismatch` that a new message needs a new key, and an
  answer EMX kept under the key that nothing went out again. `emx read
  <id> --raw --out PATH` and `emx part --out PATH` save into a folder
  under the name EMX gives the file, its subject for a message, beside
  a file of that name rather than over it. `emx me` lists the limits a
  send is held to.
- The SDK makes a call refused with `503 try_later` again after the
  pause in `Retry-After`, up to 30 seconds (`Client::max_wait` sets
  another), where a repeat is harmless: reads, changes that leave the
  same state however often they are made, and a send with its
  `Idempotency-Key`. The service now answers a fault while it checks
  the token this way on every route, `GET /api/me`, the raw source,
  deleting and sending included; it is never a sign-out, and
  `Error::is_try_later` and `Error::changed_nothing` say so. A call that
  makes something new is made again only after the per-minute rate
  limit. A replayed answer under an `Idempotency-Key` sets
  `Sent::replayed` or `Error::is_replayed`; `405 method_not_allowed`
  names the methods the path takes. `GET /api/me` gives
  `limits.maxRecipients`, `limits.hourlyRecipients`, `mailHost` and
  `listMail` at each address of `sendFrom`, and a send takes
  `forwardParts` and the list mail `headers`. `raw_download` and
  `part_download` answer the bytes with the name the service gives
  them, `filename*` in UTF-8 before the ASCII `filename`, go on with a
  `Range` of the same version when the transfer breaks, and check a
  raw source against the SHA-256 in its `ETag`; `safe_file_name`, which
  the desktop app used on its own, makes a name fit to save under.
- After Archive, Trash or Delete on the open message, the desktop app
  opens the next message in the list, or the one above at the end of
  it, as the web client does. It closed the message, and `j` then went
  on from the top of the list.
- The desktop composer sends to addresses separated by `;`, as Outlook
  writes them, or by spaces, and to names with `[`, `:` or `;` in them,
  which it now quotes; such a send failed with "an address is
  malformed". A name that is only the address again is left out. A
  reply to a forward (`WG:`, `Fwd:`) is `Re: WG: ...`; `AW:` and
  `Antw:` count as replies already.
- The README and CONTRIBUTING say that a plain `cargo build` or
  `cargo test` covers the SDK and the command line only, and what the
  desktop app needs besides (Rust 1.88, GTK and WebKit on Linux).
  `source-map-js`, used by the desktop app's build only, is at 1.2.2,
  past a denial-of-service fix; the remaining `npm audit` findings are
  low and in the build tools, not in the app.
- The desktop window opens where it was and as large as it was when it
  last closed, maximised if it was, instead of at 1200 by 800 each
  time. A place on a screen that is no longer there is not used.
- A base URL that redirects, such as the public site
  `https://emxmail.ch`, is refused with the address it points to and a
  hint to use that one, under the code `redirect`; the error read only
  "HTTP 301". The redirect is still not followed, so the token never
  goes to another address.
- `emx` ends with a status a script can act on: 3 without a token or
  with one EMX refused, 4 when the network or EMX is away for a moment,
  5 when a send may or may not have gone out, besides 2 for a wrong
  command line and 1 for any other failure; every failure used to end
  with 1. `approve`, `block`, `forget` and `webhook delete`, `test` and
  `enable` answer JSON with `--json`; they printed prose.
- The desktop composer checks attachments against the largest message
  EMX takes, as mail encodes them, before reading a file; a message
  over the limit was uploaded whole and only then refused. Saving an
  attachment shortens a name too long for the file system, such as 90
  Chinese characters, in front of its extension, and gives a name
  Windows keeps for a device (`CON`, `NUL.pdf`) a leading `_`; both
  failed to save.
- The desktop app's search box empties when another folder or account
  opens, and a search still waiting to start is dropped; the box kept
  its words over the whole folder, and one more letter searched again.
  A folder and a new search open at the top of the list instead of
  where the previous list was scrolled, which could also load a second
  page at once.
- A message at the bottom of the desktop app's window stays for its
  full time; the timer of an earlier one cleared it early, so "Saved
  to ..." could vanish at once after a quick Archive.
- `EMX_CA_FILE` names a PEM file of certificate authorities that `emx`
  and the desktop app trust instead of the bundled Mozilla roots, for a
  network whose proxy inspects TLS and for a self-hosted server with a
  certificate from a private authority; both failed with a certificate
  error. The SDK has `Client::trust_only(pem)`.
- A reply in the desktop app to a message that has only HTML, as most
  newsletters, many invoices and much Outlook mail, quotes its text;
  the quote used to be empty.
- `emx login` gives the terminal back as it was when Ctrl-C ends it
  while it asks for the token; the terminal stayed without echo. On
  Windows the token is no longer shown as it is typed. A base URL from
  `EMX_BASE_URL` is used to check the token but is no longer written
  to the file, where it outlasted the environment it was set for;
  `--base-url` keeps one.
- The desktop app stops following an account within a second of
  signing out; a watcher could go on for up to a minute and tell the
  window about the previous account. It starts following only once the
  window listens, so the first change after signing in is not missed.
  The SDK has `Events::next_within`, which waits for the next event at
  most as long as asked and keeps the stream open.
- The command line comes with the notices of the crates it is built
  from, in `cli/THIRD-PARTY-NOTICES.txt`, and
  `scripts/third-party-notices.py` writes them for the command line or
  the desktop app.
- Every source file names its licence with an SPDX line: MPL-2.0, with
  the notice of the licence's Exhibit A, for the SDK and the command
  line; AGPL-3.0-only for the desktop app.
- The SDK reads more of what the service sends: whether an
  organisation was cancelled or its subscription ended, and until when
  (`Tenant`), the list mail limits (`Limits::list_mail`), an
  organisation's `principal_id` and `state`, a sealed message's
  `envelope`, who sent a message on someone's behalf (`Body::sender`),
  and a sealed contact's `label`. All are optional.
- `emx api PATCH` and `PUT` without `--data` are refused; they sent
  `null`, and `PATCH /api/me/prefs` with it can store preferences that
  are not an object. The SDK reads such preferences as none, so that
  `me()`, and with it signing in from the command line and the desktop
  app, keeps working.
- `webhook::verify` refuses a timestamp far outside the range as stale;
  `t=-9223372036854775808` made a receiver built with overflow checks
  panic.
- The desktop app shows HTML mail on a white page in dark mode too, as
  the web client does. Mail is written for a white page, and its dark
  text was unreadable on the dark panel.
- `emx` drops control characters from mail it shows on a terminal, so
  a message can no longer set the window's title, clear the screen or
  write the clipboard with escape sequences. `read --raw` on a terminal
  is cleaned the same way, and `part` without `--out` shows only a text
  part on a terminal; to a file or a pipe, both write the bytes as
  received.
- `emx read` of an HTML-only message decodes every character reference
  once: `&#34;` reads as `"`, and `&amp;lt;` as `&lt;`, no longer `<`.
  A large style block no longer takes a minute.
- `emx watch` waits and tries again when the per-minute rate limit is
  reached, as long as the service asks with `Retry-After`, instead of
  ending; it ended when another program with the same token used up the
  limit.
- The desktop app writes its kept sign-in whole, through a file beside
  it, so a crash or a full disk can no longer leave it empty and sign
  the person out; and it makes the file readable by the person alone
  each time, also when an earlier one had wider permissions.
- `emx` runs with `EMX_TOKEN` set and no home folder, as under a
  service manager or in a container; it used to refuse with "no home
  directory to keep the token in". `emx login` writes its file whole,
  through a file beside it, so a full disk or a crash cannot leave it
  empty, and makes its folder readable by the person alone.
- A large attachment no longer fails on a slow line: downloads and the
  upload of a send are limited by how long they stand still, a minute,
  instead of by how long they take as a whole (60 seconds for a
  download, two and a half minutes for a send). Connecting and waiting
  for the answer keep their limits.
- A send refused with `429 hourly_limit` (more recipients outside the
  organisation than the hour allows) is no longer tried three times:
  of the 429 refusals only the per-minute rate limit is tried again.
  The desktop app says which limit was reached, instead of "too many
  requests in the last minute".
- Search goes past the first 50 hits. The SDK has
  `Client::search_page(account, query, mailbox, limit, cursor)`, which
  can keep to one folder; `emx search` takes `--mailbox`, `--limit` and
  `--cursor` and says where the next page starts, as `emx list` does;
  and the desktop app loads more hits as the list is scrolled.
- The desktop app no longer rebuilds an open HTML message when it is
  flagged or marked read: remote images let in stay, and the frame does
  not flicker or fetch its inline images again.
- Reply all in the desktop app leaves out exactly the person's own
  addresses, every one they send from, compared whole and in any case.
  It used to drop `lukas@x.ch` for the person `s@x.ch`, and to keep
  their aliases. Each address is in a reply once, and a reply to one's
  own message goes to its recipients.
- When EMX refuses the token (expired or revoked), the desktop app
  shows the sign-in form with the reason, instead of failing at every
  click; the live updates say so too, rather than stopping without a
  word. Listeners from an earlier sign-in are removed.
- The desktop app deletes a message for good only when the message
  itself is in Trash or Junk. A search from Trash also finds messages in
  other folders, and Delete on one of those deleted it for good; it now
  moves it to Trash, and the button says so. The search box says that
  it searches all folders.
- Links in an HTML message open in the system's browser, and mail
  addresses in the mail program, from the desktop app; a click on one
  used to do nothing. Only `http`, `https` and `mailto` links leave the
  app. The window gets no new permission: the app hands the link on
  itself.
- The desktop app's message list no longer stops updating after a
  message below the loaded part changed on another device: a change is
  added only where the loaded part reaches, and a further page never
  brings a message twice. A change notice that comes while the app is
  fetching changes is no longer dropped; the app fetches once more. The
  unread count moves only when EMX says the message changed. An answer
  that comes back late (a folder clicked before another, an earlier
  search, a page of the previous folder, a message passed with `j` or
  `k`) no longer replaces a newer one, and the open message follows
  what other devices do to it, its flag among them.
- `emx` refuses an option a command does not take, and arguments it does
  not take, with exit status 2 and nothing sent. `emx delete <id>
  --dry-run` used to delete the message for good, `send --subjct S`
  sent without a subject, and `send --attachment F` without the file.
  There is no `--dry-run`. `-h` shows the help.
- `emx send` keeps every `--to`, `--cc` and `--bcc` when one is given
  more than once; it used to keep the last one only. Any other flag given
  twice is now refused instead of the last value winning.
- `emx read <id> --html` works as the help says; it used to answer
  "--html needs a value", because only `send --html` takes one.
- `emx send --attach` gives each file a media type from its extension,
  so a PDF arrives as application/pdf instead of
  application/octet-stream.
- `emx` reports a file it cannot read or write with the file's name,
  and bad JSON in `emx api --data` as such; both used to read as
  "could not reach EMX" or "unexpected answer from EMX". An unknown
  mailbox name no longer pretends to be an HTTP 404 from the service.
- `emx` ends quietly when the program reading its output stops early,
  as in `emx --json list | head -1`; it used to panic.
- The event stream no longer ends after a minute: the SDK's limit for a
  whole call cut it off. A stream now ends when the service ends it, or
  with an error when it has been silent for a minute, past two
  keepalives. `emx watch` reopens it without a word, and neither it nor
  `--exec` reacts to a batch with nothing in it, so the README's
  notification example no longer fires every minute. The desktop app
  tells the window about a new stream only when something changed while
  it was down.
- Every send carries an `Idempotency-Key`, the same on each attempt, and
  `Client::send_mail_with_key` lets a program choose it. Only refusals
  that pass (the per-minute rate limit, a service briefly unavailable)
  are tried again, and only after a pause of a few seconds; the daily
  and monthly sending limits are returned at once, and so is a longer
  pause, with `retry_after`, where the client used to wait up to a
  minute without a word. A send may take as long as the service allows
  for it, two minutes.
- A plain-HTTP base URL is accepted only when its host is exactly
  `localhost`, `127.0.0.1` or `[::1]`. A host such as
  `localhost.example.ch` used to pass the check, and the token went to
  it in clear text.
- The SDK's tests cover every call the command line and the desktop app
  make, and the scripted server answers a refused token with
  `bad_token`, the code the service sends.
- `emx`: times show their zone (`2026-09-24 06:12 UTC`); `--account`
  takes an address as well as an id; `--limit` outside 1 to 200 is
  refused instead of quietly becoming 1 or 200; message text is printed
  with plain line ends; and `send` without `--from` picks the person's
  own address when a shared mailbox lists a primary address too.
- The desktop app no longer shows the sign-in form when it starts
  without a network: it says it is offline, keeps the sign-in, and
  connects on its own when EMX can be reached again. Errors from a lost
  connection read in plain words instead of "os error 111".
- The desktop app asks before deleting a message for good in Trash and
  before discarding a composer with text in it (Esc, a click beside it
  or Discard). Backspace no longer trashes the open message, and the
  list's keys do nothing while the composer is open or a select has
  the focus.
- The desktop app shows its licence notice and where its source is, on
  the sign-in card and in the empty reading pane, as the AGPL asks of an
  interactive program.
- The README examples compile, and are compiled with the doctests from
  now on. The SDK's README says how to depend on it.
- The oldest Rust that builds the SDK and the command line is 1.85, as
  their dependencies require, not 1.80; CI checks it. The desktop app
  needs 1.88.
- `emx watch --exec` on Windows hands the command line to cmd as it was
  typed. It used to be quoted for a program, with a backslash before
  each inner quote, so a command with a quoted path did not run.
- An event stream that is given up after a minute of silence now
  closes its connection. The thread reading it used to wait on the dead
  connection, and keep its socket, for over an hour, once for every
  reconnect. ureq is held to 3.4, since this uses its transport
  traits, which are outside its semver promise.
- `emx send --idempotency-key K` sends with a key of your choosing;
  without it, each run makes a new one. When the answer is lost, the
  error names the key, and the same command with it sends the message
  once at most. `emx_sdk::idempotency_key()` makes a key, and
  `send_mail_with_key` refuses an empty key, one over 200 characters
  and one with characters a header cannot carry, before sending.
- The desktop window no longer freezes while it waits for EMX. Every
  call ran on the main thread, so a send (up to two and a half minutes)
  or the offline screen's repeated attempts to connect held the window
  still. A sign-in that is still under way when the person signs out
  now comes to nothing instead of signing them in again.
- The desktop composer sends with one idempotency key for its message,
  however often Send is pressed, so pressing it again after a timeout
  no longer risks a second copy.
- Signing out on the desktop app's offline screen while it is trying to
  connect now stays signed out: an attempt that ended afterwards used
  to start the app, or bring the offline screen back. When EMX refuses
  for a moment because of too many requests, the screen says so instead
  of "No connection", and so it does when EMX is not answering.
- `emx` shows a time that carries no zone without one; it used to end
  in a piece of its seconds, as in `2026-09-24 06:12 :12:40`.

## 0.1.0

The first release: the SDK with the calls a person's token makes for
mail, the command line, and the desktop app with reading, search,
replies, attachments and live updates.
