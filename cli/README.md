# emx

EMX mail from the command line, for a person at a terminal and for a
script on a server.

```sh
cargo install --path .
emx login                       # paste a token; or set EMX_TOKEN
emx mailboxes
emx list --mailbox inbox --limit 20
emx read <id>
emx read <id> --raw --out ~/Mail  # saved as "<subject>.eml", the name EMX gives it
emx search offerte 2026
emx send --to anna@example.ch --subject "Re: Offerte" --text "Gerne." --attach Offerte.pdf
emx seen <ids...>; emx move <ids...> --to archive
emx watch --exec 'notify-send "EMX" "Something changed"'
emx --json list | jq '.messages[].subject'
```

`emx --help` lists everything. The token lives in
`~/.config/emx/config.json` (`%APPDATA%\emx\config.json` on Windows),
readable by you alone; `EMX_TOKEN` and `EMX_BASE_URL` in the environment
win over it, which is what a server uses.

`emx` ends with status 0 when it did what was asked, 1 when EMX refused
or a file here could not be read, 2 for a command line it does not
understand, 3 without a token or with one EMX refused, 4 when the
network or EMX is away for a moment (the same command may work soon),
and 5 when a send may or may not have gone out. A send EMX refused for
a passing fault on its side (`try_later`) is tried again with the same
idempotency key and, when that does not help, ends with 4: nothing went
out. A limit says when the message can go. With `--json`, every
command answers JSON.

`emx` trusts the Mozilla root certificates it is built with. Behind a
proxy that inspects TLS with a company authority, or for a self-hosted
server with a certificate from a private one, set `EMX_CA_FILE` to a
PEM file of the authorities to trust instead; for both kinds of server,
put both in the file, for instance the system's bundle
(`/etc/ssl/certs/ca-certificates.crt` on Debian) and the company's.

`emx watch` follows the account and prints every change; with `--exec`
it runs a command for each batch, with the changes as JSON on its
standard input. A change is anything that happened to a message, from
any device, this one included: new mail, but also mail read, flagged,
moved or deleted. It reconnects on its own, without a word, and picks
up where it was; a batch with nothing in it is never printed or passed
on.

`emx api` reaches any call under `/api/`, for the administration calls
and whatever the service adds next.

Mozilla Public License 2.0. The crates `emx` is built from are under
their own licences, MIT, Apache 2.0, ISC and others; their notices are
in THIRD-PARTY-NOTICES.txt, which goes with every copy of the program.
`scripts/third-party-notices.py emx-cli` writes it again after a
dependency changes.
