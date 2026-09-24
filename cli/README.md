# emx

EMX mail from the command line, for a person at a terminal and for a
script on a server.

```sh
cargo install --path .
emx login                       # paste a token; or set EMX_TOKEN
emx mailboxes
emx list --mailbox inbox --limit 20
emx read <id>
emx search offerte 2026
emx send --to anna@example.ch --subject "Re: Offerte" --text "Gerne." --attach Offerte.pdf
emx seen <ids...>; emx move <ids...> --to archive
emx watch --exec 'notify-send "New mail"'
emx --json list | jq '.messages[].subject'
```

`emx --help` lists everything. The token lives in
`~/.config/emx/config.json` (`%APPDATA%\emx\config.json` on Windows),
readable by you alone; `EMX_TOKEN` and `EMX_BASE_URL` in the environment
win over it, which is what a server uses.

`emx watch` follows the account and prints every change; with `--exec`
it runs a command for each batch, with the changes as JSON on its
standard input. It reconnects on its own and picks up where it was.

`emx api` reaches any call under `/api/`, for the administration calls
and whatever the service adds next.

Mozilla Public License 2.0.
