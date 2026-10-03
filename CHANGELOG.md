# Changelog

## Unreleased

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
- The README examples compile, and are compiled with the doctests from
  now on. The SDK's README says how to depend on it.
- The oldest Rust that builds the SDK and the command line is 1.85, as
  their dependencies require, not 1.80; CI checks it. The desktop app
  needs 1.88.

## 0.1.0

The first release: the SDK with the calls a person's token makes for
mail, the command line, and the desktop app with reading, search,
replies, attachments and live updates.
