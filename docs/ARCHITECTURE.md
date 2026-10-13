# Architecture

Three parts, one direction of dependency: the app uses the SDK, the
command line uses the SDK, the SDK uses the API.

```
desktop/src (Svelte)  ──invoke──▶  desktop/src-tauri (Rust)  ──▶  emx-sdk  ──HTTPS──▶  EMX
cli                                                             ──▶  emx-sdk  ──HTTPS──▶  EMX
```

## emx-sdk

- `client.rs` is the calls. One `Client` holds the base URL, the token and
  a `ureq` agent; it is `Clone`, `Send` and `Sync`. Each call builds one
  request, sends it, and either decodes the answer or turns a refusal
  into `Error::Api` with the API's own code and message. A refusal that
  passes is made again up to twice more after the pause the service
  asks for in `Retry-After`, when that pause is at most 30 seconds
  (`Client::max_wait`); a longer one is returned with the error for the
  caller to decide. The per-minute rate limit refuses before anything
  is done, so every call is made again after it. A `503 try_later` (a
  fault on the service's side, such as its database not answering while
  it checks the token) is made again only where a repeat is harmless:
  reads, changes that leave the same state however often they are made,
  and a send, which carries an `Idempotency-Key`, the same on every
  attempt, so that the service sends it once at most. A 503 is never a
  sign-out; only a 401 is. Lasting refusals such as the daily sending
  limit or `too_many_recipients` are returned at once.
- Downloads (a message's raw source, a part) carry the name the service
  gives them, `filename*` in UTF-8 as RFC 8187 encodes it before the
  ASCII `filename`; `safe_file_name` makes it fit to save under. A
  transfer that breaks goes on with a `Range` of the same version
  (`If-Range`), and a raw source is checked against the SHA-256 the
  service gives as its `ETag`.
- `types.rs` is the records, with serde. Every optional field defaults,
  and unknown fields are ignored, because the service adds fields and
  never removes one.
- `events.rs` reads the server-sent event stream as an iterator, on a
  thread of its own so that a stream silent for a minute (two lost
  keepalives) ends with an error instead of hanging. The service ends a
  stream after an hour; the caller opens another and asks for changes
  since its last modseq, so nothing between is missed.
- `webhook.rs` checks `X-EMX-Signature` with HMAC-SHA256 written in the
  crate, so a program that only receives webhooks needs nothing else.

## emx (the command line)

`main.rs` dispatches on the first word. `args.rs` is the argument parser,
sixty lines because a command line needs flags with values, switches and
what is left. `config.rs` is the token file, `0600` on Unix, under the
system's config folder, overridden by `EMX_TOKEN` and `EMX_BASE_URL`.

`watch` is the command a server runs: it follows the event stream,
asks for changes, prints or hands them to a command when there are any,
and reconnects quietly, with a growing pause when the service is away.

## The desktop app

- The Rust side (`src-tauri/src/lib.rs`) holds the token in the app's data
  folder and the `Client` in managed state. Each command the window can
  call is a function with `#[tauri::command]`; the window gets records,
  never the token. On sign-in one thread per account follows the event
  stream and emits `emx://change` with the modseq.
- The window (`src/lib/state.svelte.ts`) is one state object. A folder is
  loaded once; a change notice makes it ask for changes since the last
  modseq it handled and fold them in. Notices during a fetch are
  coalesced. `reload` from the service means the state is too old and
  the lists are loaded again.
- Message HTML is sanitised by the service (no scripts, no remote loads,
  remote images in `data-src`) and shown in a frame with `sandbox`.
  Inline images point at the part endpoint; the window fetches them
  through the Rust side and puts them in as data URIs. Remote images
  load only when asked.

## Testing

The SDK's tests run against a small HTTP server inside the test that
answers scripted responses and records the requests, so a test asserts
both what was sent and how the answer was read. Every call the command
line and the desktop app make is covered, with answers and error codes
as the service sends them; `create_mailbox` and `set_prefs` are not
yet. The webhook module is tested against the RFC 4231 and FIPS 180-4
vectors. The command line's own logic (arguments, base64, HTML to
text, media types) has unit tests, and `cli/tests` runs the binary
against a scripted server for what only shows as a whole: `watch` and
the output pipe. Nothing here runs against a live service; the rest
is exercised by hand against one.
