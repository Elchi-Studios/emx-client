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

## 0.1.0

The first release: the SDK with every call a token can make, the command
line, and the desktop app with reading, search, replies, attachments and
live updates.
