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

## 0.1.0

The first release: the SDK with every call a token can make, the command
line, and the desktop app with reading, search, replies, attachments and
live updates.
