# Changelog

## Unreleased

- `emx send` keeps every `--to`, `--cc` and `--bcc` when one is given
  more than once; it used to keep the last one only. Any other flag given
  twice is now refused instead of the last value winning.

## 0.1.0

The first release: the SDK with every call a token can make, the command
line, and the desktop app with reading, search, replies, attachments and
live updates.
