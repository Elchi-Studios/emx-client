//! `emx`: EMX mail from the command line. Reads, sends, watches, and
//! everything else a token can do, for a person at a terminal and for a
//! script on a server.

mod args;
mod config;

/// `println!` for standard output, through [`emit`].
macro_rules! outln {
    () => {
        $crate::emit(format_args!("\n"))
    };
    ($($t:tt)*) => {
        $crate::emit(format_args!("{}\n", format_args!($($t)*)))
    };
}

/// `print!` for standard output, through [`emit`].
macro_rules! outp {
    ($($t:tt)*) => {
        $crate::emit(format_args!($($t)*))
    };
}

use args::Args;
use emx_sdk::{Client, Draft, DraftAttachment, Error, Event, Mailbox, Message, ME};
use std::io::{IsTerminal, Read, Write};
use std::process::ExitCode;

const USAGE: &str = "\
emx, mail from the command line

Usage: emx <command> [options]

Sign in
  login [--base-url URL]     store a token (read from stdin or asked for)
  logout                     forget it
  me                         who the token is and what it can reach

Read
  mailboxes                  folders with counts
  list [--mailbox M] [--limit N] [--cursor C]
                             messages, newest first (M is a name or an id, default Inbox)
  read <id> [--raw] [--html] a message; --raw prints it as received
  part <id> --part P [--out FILE]
                             an attachment, to a file or stdout
  thread <id>                a conversation, oldest first
  search <words...>          every word must occur, prefixes match

Change
  seen|unseen <ids...>       mark read or unread
  flag|unflag <ids...>
  move <ids...> --to M
  delete <ids...>            for good; move to Trash first if in doubt
  snooze <ids...> --until RFC3339

Send
  send --to A [--cc A] [--bcc A] --subject S [--from A]
       [--text T | --body-file F | stdin] [--html H]
       [--attach FILE]... [--in-reply-to MSGID]
                             --to, --cc and --bcc repeat, or take a list: a, b

The screener
  contacts [--state approved|blocked]
  approve <address> [--name N]
  block <address>
  forget <address>

Webhooks
  webhooks                   list them and the events there are
  webhook add URL --events a,b [--description D]
  webhook delete|test|enable <id>
  webhook deliveries <id>

Live
  watch [--exec CMD]         print changes as they happen; run CMD for each
                             (the change as JSON on its stdin)

Anything
  api GET|POST|PATCH|PUT|DELETE /api/... [--data JSON]

Options for every command
  --account A     a shared mailbox's id (default: your own)
  --json          machine-readable output
  --version, --help

The token comes from ~/.config/emx/config.json (emx login), or from
EMX_TOKEN and EMX_BASE_URL in the environment. Reference:
https://docs.elchi.dev/emx-api
";

fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    if args.switch("version") {
        outln!("emx {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let command = args.positional.first().map(|s| s.as_str()).unwrap_or("");
    if command.is_empty() || args.switch("help") || command == "help" {
        outp!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match run(command, &args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&e),
    }
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("emx: {msg}");
    ExitCode::FAILURE
}

fn run(command: &str, a: &Args) -> Result<(), String> {
    match command {
        "login" => return login(a),
        "logout" => {
            config::remove()?;
            outln!("Signed out. The token itself is revoked under Settings, Developer API.");
            return Ok(());
        }
        _ => {}
    }
    let cfg = config::load()?;
    if cfg.token.is_empty() {
        return Err("no token; run emx login, or set EMX_TOKEN".into());
    }
    let emx = Client::with_base_url(&cfg.base_url, &cfg.token)
        .map_err(|e| e.to_string())?
        .user_agent(&format!("emx-cli/{}", env!("CARGO_PKG_VERSION")));
    let account = a.value("account").unwrap_or(ME);
    let json = a.switch("json");
    let rest: Vec<&str> = a.positional.iter().skip(1).map(|s| s.as_str()).collect();
    let r = match command {
        "me" => me(&emx, json),
        "mailboxes" => mailboxes(&emx, account, json),
        "list" => list(&emx, account, a, json),
        "read" => read(&emx, account, &rest, a, json),
        "part" => part(&emx, account, &rest, a),
        "thread" => thread(&emx, account, &rest, json),
        "search" => search(&emx, account, &rest, json),
        "seen" => keywords(&emx, account, &rest, &["$seen"], &[], json),
        "unseen" => keywords(&emx, account, &rest, &[], &["$seen"], json),
        "flag" => keywords(&emx, account, &rest, &["$flagged"], &[], json),
        "unflag" => keywords(&emx, account, &rest, &[], &["$flagged"], json),
        "move" => mv(&emx, account, &rest, a, json),
        "delete" => delete(&emx, account, &rest, json),
        "snooze" => snooze(&emx, account, &rest, a, json),
        "send" => send(&emx, a, json),
        "contacts" => contacts(&emx, account, a, json),
        "approve" => contact(&emx, account, &rest, "approved", a),
        "block" => contact(&emx, account, &rest, "blocked", a),
        "forget" => forget(&emx, account, &rest),
        "webhooks" => webhooks(&emx, json),
        "webhook" => webhook(&emx, account, &rest, a, json),
        "watch" => watch(&emx, account, a, json),
        "api" => api(&emx, &rest, a),
        _ => return Err(format!("unknown command {command}; emx --help lists them")),
    };
    r.map_err(|e| describe(&e))
}

/// Why a command failed: EMX refused or could not be reached, or
/// something on this computer went wrong (a file, the input, the
/// arguments). They are kept apart so that a missing file is not
/// reported as a network problem.
enum Fail {
    Emx(Error),
    Local(String),
}

impl From<Error> for Fail {
    fn from(e: Error) -> Self {
        Fail::Emx(e)
    }
}

fn local(e: impl std::fmt::Display) -> Fail {
    Fail::Local(e.to_string())
}

/// A local failure that names the file it concerns.
fn file_err(path: &str, e: std::io::Error) -> Fail {
    Fail::Local(format!("{path}: {e}"))
}

fn describe(e: &Fail) -> String {
    match e {
        Fail::Local(m) => m.clone(),
        Fail::Emx(Error::Api { status: 401, .. }) => {
            "the token was refused; run emx login with a current one".into()
        }
        Fail::Emx(Error::Api { code, message, .. }) if code == "reload" => {
            format!("{message}; run the command again")
        }
        Fail::Emx(e) => e.to_string(),
    }
}

// --- Sign in -------------------------------------------------------------

fn login(a: &Args) -> Result<(), String> {
    let mut cfg = config::load().unwrap_or_default();
    if let Some(u) = a.value("base-url") {
        cfg.base_url = u.to_string();
    }
    if cfg.base_url.is_empty() {
        cfg.base_url = emx_sdk::DEFAULT_BASE_URL.to_string();
    }
    let stdin = std::io::stdin();
    let token = if stdin.is_terminal() {
        eprintln!("Make a token under Settings, Developer API, in the EMX web client.");
        eprint!("Token: ");
        let _ = std::io::stderr().flush();
        read_hidden()?
    } else {
        let mut s = String::new();
        stdin.lock().read_to_string(&mut s).map_err(|e| e.to_string())?;
        s
    };
    let token = token.trim().to_string();
    let emx = Client::with_base_url(&cfg.base_url, &token).map_err(|e| e.to_string())?;
    let me = emx.me().map_err(|e| match e {
        Error::Api { status: 401, .. } => "the service refused the token; copy it again".to_string(),
        e => e.to_string(),
    })?;
    cfg.token = token;
    let p = config::save(&cfg)?;
    outln!(
        "Signed in as {} ({}). The token is in {}.",
        me.name,
        me.tenant.name,
        p.display()
    );
    Ok(())
}

/// Reads a line without echo where the terminal allows it.
fn read_hidden() -> Result<String, String> {
    #[cfg(unix)]
    let quiet = std::process::Command::new("stty")
        .arg("-echo")
        .stdin(std::process::Stdio::inherit())
        .status()
        .is_ok();
    #[cfg(not(unix))]
    let quiet = false;
    let mut s = String::new();
    let r = std::io::stdin().read_line(&mut s);
    #[cfg(unix)]
    if quiet {
        let _ = std::process::Command::new("stty")
            .arg("echo")
            .stdin(std::process::Stdio::inherit())
            .status();
        eprintln!();
    }
    r.map_err(|e| e.to_string())?;
    Ok(s)
}

// --- Read ----------------------------------------------------------------

fn me(emx: &Client, json: bool) -> Result<(), Fail> {
    let me = emx.me()?;
    if json {
        return out(&me);
    }
    outln!(
        "{} <{}>",
        me.name,
        me.accounts
            .iter()
            .find(|a| a.rights.own)
            .map(|a| a.address.as_str())
            .unwrap_or("")
    );
    outln!("{} ({}), {}", me.tenant.name, me.tenant.plan, me.role);
    if me.accounts.len() > 1 {
        outln!("\nAccounts");
        for acc in &me.accounts {
            outln!(
                "  {}  {}  {}{}",
                acc.id,
                acc.address,
                acc.name,
                if acc.rights.own { " (you)" } else { "" }
            );
        }
    }
    outln!("\nSend from");
    for s in &me.send_from {
        outln!("  {}{}", s.address, if s.primary { " (primary)" } else { "" });
    }
    Ok(())
}

fn mailboxes(emx: &Client, account: &str, json: bool) -> Result<(), Fail> {
    let list = emx.mailboxes(account)?;
    if json {
        return out(&list);
    }
    let width = list.iter().map(|m| m.name.len()).max().unwrap_or(4).max(4);
    outln!("{:<width$}   TOTAL  UNSEEN  ID", "NAME");
    for m in &list {
        outln!("{:<width$}  {:>6}  {:>6}  {}", m.name, m.total, m.unseen, m.id);
    }
    Ok(())
}

/// A mailbox by name, role or id.
fn find_mailbox(emx: &Client, account: &str, want: &str) -> Result<Mailbox, Fail> {
    let list = emx.mailboxes(account)?;
    let lower = want.to_lowercase();
    list.iter()
        .find(|m| m.id == want)
        .or_else(|| list.iter().find(|m| m.role == lower))
        .or_else(|| list.iter().find(|m| m.name.to_lowercase() == lower))
        .cloned()
        .ok_or_else(|| Fail::Local(format!("no mailbox called {want}; emx mailboxes lists them")))
}

fn list(emx: &Client, account: &str, a: &Args, json: bool) -> Result<(), Fail> {
    let mailbox = find_mailbox(emx, account, a.value("mailbox").unwrap_or("inbox"))?;
    let limit = a.number("limit", 30).map_err(config_err)?;
    let page = emx.messages(account, &mailbox.id, limit, a.value("cursor").unwrap_or(""))?;
    if json {
        return out(&serde_json::json!({"messages": page.items, "cursor": page.cursor}));
    }
    print_messages(&page.items);
    if !page.cursor.is_empty() {
        outln!("\nmore: --cursor {}", page.cursor);
    }
    Ok(())
}

fn print_messages(list: &[Message]) {
    if list.is_empty() {
        outln!("nothing here");
        return;
    }
    for m in list {
        let mark = if m.is_seen() { " " } else { "*" };
        let flag = if m.is_flagged() { "!" } else { " " };
        let clip = if m.has_attachments { "+" } else { " " };
        let who = if m.from.name.is_empty() {
            m.from.address.clone()
        } else {
            m.from.name.clone()
        };
        outln!(
            "{mark}{flag}{clip} {}  {:<24}  {}",
            &m.received_at[..16].replace('T', " "),
            trim(&who, 24),
            trim(&m.subject, 60)
        );
        outln!("    {}", m.id);
    }
}

fn read(emx: &Client, account: &str, rest: &[&str], a: &Args, json: bool) -> Result<(), Fail> {
    let id = rest.first().ok_or_else(|| usage("read <id>"))?;
    if a.switch("raw") {
        let raw = emx.raw(account, id)?;
        emit_bytes(&raw);
        return Ok(());
    }
    let full = emx.message(account, id)?;
    if json {
        return out(&full);
    }
    let m = &full.message;
    outln!("From:    {}", m.from);
    outln!("To:      {}", m.to.join(", "));
    if !m.cc.is_empty() {
        outln!("Cc:      {}", m.cc.join(", "));
    }
    outln!("Date:    {}", m.received_at);
    outln!("Subject: {}", m.subject);
    if full.body.sealed {
        outln!("\n[sealed: the body is encrypted to the mailbox's key and opens in the web client]");
        return Ok(());
    }
    for at in &full.body.attachments {
        outln!(
            "Part:    {}  {}  {} ({} bytes)",
            at.part,
            at.filename,
            at.content_type,
            at.size
        );
    }
    outln!();
    if a.switch("html") && !full.body.html.is_empty() {
        outln!("{}", full.body.html);
    } else if !full.body.text.is_empty() {
        outln!("{}", full.body.text);
    } else if !full.body.html.is_empty() {
        outln!("{}", strip_tags(&full.body.html));
    }
    Ok(())
}

fn part(emx: &Client, account: &str, rest: &[&str], a: &Args) -> Result<(), Fail> {
    let id = rest
        .first()
        .ok_or_else(|| usage("part <id> --part P [--out FILE]"))?;
    let p = a
        .value("part")
        .ok_or_else(|| usage("part <id> --part P [--out FILE]"))?;
    let (data, kind) = emx.part(account, id, p)?;
    match a.value("out") {
        Some(path) => {
            std::fs::write(path, &data).map_err(|e| file_err(path, e))?;
            eprintln!("{} bytes of {kind} written to {path}", data.len());
        }
        None => emit_bytes(&data),
    }
    Ok(())
}

fn thread(emx: &Client, account: &str, rest: &[&str], json: bool) -> Result<(), Fail> {
    let id = rest.first().ok_or_else(|| usage("thread <thread id>"))?;
    let list = emx.thread(account, id)?;
    if json {
        return out(&list);
    }
    print_messages(&list);
    Ok(())
}

fn search(emx: &Client, account: &str, rest: &[&str], json: bool) -> Result<(), Fail> {
    if rest.is_empty() {
        return Err(usage("search <words...>"));
    }
    let list = emx.search(account, &rest.join(" "))?;
    if json {
        return out(&list);
    }
    print_messages(&list);
    Ok(())
}

// --- Change --------------------------------------------------------------

fn keywords(
    emx: &Client,
    account: &str,
    ids: &[&str],
    add: &[&str],
    remove: &[&str],
    json: bool,
) -> Result<(), Fail> {
    if ids.is_empty() {
        return Err(usage("<ids...>"));
    }
    let changed = emx.keywords(account, ids, add, remove)?;
    done(&changed, json)
}

fn mv(emx: &Client, account: &str, ids: &[&str], a: &Args, json: bool) -> Result<(), Fail> {
    let to = a.value("to").ok_or_else(|| usage("move <ids...> --to MAILBOX"))?;
    if ids.is_empty() || a.values("to").len() > 1 {
        return Err(usage("move <ids...> --to MAILBOX"));
    }
    let target = find_mailbox(emx, account, to)?;
    let changed = emx.r#move(account, ids, &target.id)?;
    done(&changed, json)
}

fn delete(emx: &Client, account: &str, ids: &[&str], json: bool) -> Result<(), Fail> {
    if ids.is_empty() {
        return Err(usage("delete <ids...>"));
    }
    let changed = emx.delete(account, ids)?;
    done(&changed, json)
}

fn snooze(emx: &Client, account: &str, ids: &[&str], a: &Args, json: bool) -> Result<(), Fail> {
    let until = a
        .value("until")
        .ok_or_else(|| usage("snooze <ids...> --until 2026-09-28T07:00:00Z"))?;
    if ids.is_empty() {
        return Err(usage("snooze <ids...> --until RFC3339"));
    }
    let changed = emx.snooze(account, ids, until)?;
    done(&changed, json)
}

fn done(changed: &[String], json: bool) -> Result<(), Fail> {
    if json {
        return out(&serde_json::json!({"changed": changed}));
    }
    outln!("{} changed", changed.len());
    Ok(())
}

// --- Send ----------------------------------------------------------------

fn send(emx: &Client, a: &Args, json: bool) -> Result<(), Fail> {
    // Recipients collect over repeated flags, as attachments do.
    let to = a.addresses("to");
    if to.is_empty() {
        return Err(usage("send --to A --subject S [--text T]"));
    }
    let from = match a.value("from") {
        Some(f) => f.to_string(),
        None => {
            let me = emx.me()?;
            me.send_from
                .iter()
                .find(|s| s.primary)
                .or(me.send_from.first())
                .map(|s| s.address.clone())
                .ok_or_else(|| usage("no address to send from; --from A"))?
        }
    };
    let text = match (a.value("text"), a.value("body-file")) {
        (Some(t), _) => t.to_string(),
        (None, Some(f)) => std::fs::read_to_string(f).map_err(|e| file_err(f, e))?,
        (None, None) if a.value("html").is_none() => {
            let mut s = String::new();
            if std::io::stdin().is_terminal() {
                eprintln!("Type the message, then Ctrl-D:");
            }
            std::io::stdin()
                .read_to_string(&mut s)
                .map_err(|e| Fail::Local(format!("reading the message from standard input: {e}")))?;
            s
        }
        _ => String::new(),
    };
    let mut attachments = Vec::new();
    for path in a.values("attach") {
        let data = std::fs::read(path).map_err(|e| file_err(path, e))?;
        let filename = std::path::Path::new(path)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        attachments.push(DraftAttachment {
            content_type: content_type(&filename).to_string(),
            filename,
            data: base64(&data),
            content_id: String::new(),
        });
    }
    let draft = Draft {
        from,
        to,
        cc: a.addresses("cc"),
        bcc: a.addresses("bcc"),
        reply_to: a.value("reply-to").unwrap_or("").into(),
        subject: a.value("subject").unwrap_or("").into(),
        text,
        html: a.value("html").unwrap_or("").into(),
        in_reply_to: a.value("in-reply-to").unwrap_or("").into(),
        references: a
            .value("in-reply-to")
            .map(|r| vec![r.to_string()])
            .unwrap_or_default(),
        attachments,
    };
    let sent = emx.send_mail(&draft)?;
    if json {
        return out(&sent);
    }
    outln!(
        "sent to {} recipient{}",
        sent.recipients,
        if sent.recipients == 1 { "" } else { "s" }
    );
    Ok(())
}

// --- The screener --------------------------------------------------------

fn contacts(emx: &Client, account: &str, a: &Args, json: bool) -> Result<(), Fail> {
    let list = emx.contacts(account, a.value("state").unwrap_or(""))?;
    if json {
        return out(&list);
    }
    for c in &list {
        outln!("{:<9} {}  {}", c.state, c.address, c.name);
    }
    Ok(())
}

fn contact(emx: &Client, account: &str, rest: &[&str], state: &str, a: &Args) -> Result<(), Fail> {
    let address = rest.first().ok_or_else(|| usage("<address>"))?;
    emx.set_contact(account, address, state, a.value("name").unwrap_or(""))?;
    outln!("{address} {state}");
    Ok(())
}

fn forget(emx: &Client, account: &str, rest: &[&str]) -> Result<(), Fail> {
    let address = rest.first().ok_or_else(|| usage("forget <address>"))?;
    emx.remove_contact(account, address)?;
    outln!("{address} forgotten");
    Ok(())
}

// --- Webhooks ------------------------------------------------------------

fn webhooks(emx: &Client, json: bool) -> Result<(), Fail> {
    let (hooks, events) = emx.webhooks()?;
    if json {
        return out(&serde_json::json!({"webhooks": hooks, "events": events}));
    }
    if hooks.is_empty() {
        outln!("no webhooks; events there are: {}", events.join(", "));
        return Ok(());
    }
    for h in &hooks {
        let state = if h.disabled_at.is_some() {
            format!("off: {}", h.disabled_reason)
        } else {
            "on".into()
        };
        outln!(
            "{}  {}  [{}]  {}  {}",
            h.id,
            h.url,
            h.events.join(","),
            state,
            h.description
        );
    }
    Ok(())
}

fn webhook(emx: &Client, account: &str, rest: &[&str], a: &Args, json: bool) -> Result<(), Fail> {
    let what = rest.first().copied().unwrap_or("");
    let id = rest.get(1).copied();
    match what {
        "add" => {
            let url = id.ok_or_else(|| usage("webhook add URL --events a,b"))?;
            let events: Vec<&str> = a
                .value("events")
                .unwrap_or("")
                .split(',')
                .filter(|e| !e.is_empty())
                .collect();
            let made = emx.create_webhook(account, url, &events, a.value("description").unwrap_or(""))?;
            if json {
                return out(&made);
            }
            outln!("{}\nsecret (shown once): {}", made.webhook.id, made.secret);
        }
        "delete" => {
            emx.delete_webhook(id.ok_or_else(|| usage("webhook delete <id>"))?)?;
            outln!("deleted");
        }
        "test" => {
            emx.test_webhook(id.ok_or_else(|| usage("webhook test <id>"))?)?;
            outln!("a ping is on its way");
        }
        "enable" => {
            emx.enable_webhook(id.ok_or_else(|| usage("webhook enable <id>"))?)?;
            outln!("on again");
        }
        "deliveries" => {
            let list = emx.webhook_deliveries(id.ok_or_else(|| usage("webhook deliveries <id>"))?, 50)?;
            if json {
                return out(&list);
            }
            for d in &list {
                let state = if d.delivered_at.is_some() {
                    "delivered".to_string()
                } else if d.failed_at.is_some() {
                    format!("failed: {}", d.last_error)
                } else {
                    format!("pending, next {}", d.next_at.clone().unwrap_or_default())
                };
                outln!(
                    "{}  {}  {}  attempts {}  {}",
                    d.created_at,
                    d.event,
                    d.last_status,
                    d.attempts,
                    state
                );
            }
        }
        _ => return Err(usage("webhook add|delete|test|enable|deliveries")),
    }
    Ok(())
}

// --- Live ----------------------------------------------------------------

/// Follows the account: for every change, asks what changed and prints
/// it, or hands it to a command. A batch with nothing in it prints
/// nothing and runs nothing: the service announces its modseq on every
/// connection, and that alone is not a change. When the stream ends or
/// breaks, a new one is opened without a word and picks up from the last
/// modseq, so nothing in between is missed; only a service that stays
/// away is reported.
fn watch(emx: &Client, account: &str, a: &Args, json: bool) -> Result<(), Fail> {
    let mut modseq = current_modseq(emx, account)?;
    let exec = a.value("exec");
    if !json {
        eprintln!("watching {account} from modseq {modseq}; Ctrl-C ends it");
    }
    let mut backoff = 1u64;
    loop {
        let opened = std::time::Instant::now();
        match emx.events(account) {
            Ok(stream) => {
                for event in stream {
                    match event {
                        Ok(Event::Change { modseq: now }) if now > modseq => {}
                        Ok(_) => continue,
                        // Broken or silent: a new stream is opened below.
                        Err(_) => break,
                    }
                    match catch_up(emx, account, &mut modseq, exec, json) {
                        Ok(()) => {}
                        Err(Fail::Emx(e)) if e.is_retryable() => break,
                        Err(e) => return Err(e),
                    }
                }
            }
            Err(e) if e.is_retryable() => {
                eprintln!("emx: {e}; trying again in {backoff}s");
                std::thread::sleep(std::time::Duration::from_secs(backoff));
                backoff = (backoff * 2).min(60);
                continue;
            }
            Err(e) => return Err(e.into()),
        }
        // A stream that keeps ending at once must not turn this into a
        // busy loop; one that lasted resets the pause.
        if opened.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_secs(backoff));
            backoff = (backoff * 2).min(60);
        } else {
            backoff = 1;
        }
    }
}

/// The highest modseq of an account's folders: where following starts.
fn current_modseq(emx: &Client, account: &str) -> Result<i64, Fail> {
    Ok(emx
        .mailboxes(account)?
        .iter()
        .map(|m| m.modseq)
        .max()
        .unwrap_or(0))
}

/// Asks for everything after `modseq`, page by page, and prints or hands
/// on each batch that has something in it.
fn catch_up(
    emx: &Client,
    account: &str,
    modseq: &mut i64,
    exec: Option<&str>,
    json: bool,
) -> Result<(), Fail> {
    loop {
        let changes = match emx.changes(account, *modseq) {
            Ok(c) => c,
            Err(e) if e.code() == Some("reload") => {
                // Too far behind for the service to say what changed:
                // carry on from now.
                *modseq = current_modseq(emx, account)?;
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };
        *modseq = changes.modseq;
        if !changes.updated.is_empty() || !changes.destroyed.is_empty() {
            if json {
                outln!("{}", serde_json::to_string(&changes).map_err(local)?);
            } else {
                for m in &changes.updated {
                    outln!("{} {}  {}  {}", m.modseq, m.id, m.from, trim(&m.subject, 60));
                }
                for id in &changes.destroyed {
                    outln!("{} {}  gone", changes.modseq, id);
                }
            }
            if let Some(cmd) = exec {
                run_exec(cmd, &changes);
            }
        }
        if !changes.has_more {
            return Ok(());
        }
    }
}

fn run_exec(cmd: &str, changes: &emx_sdk::Changes) {
    let (shell, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    let child = std::process::Command::new(shell)
        .arg(flag)
        .arg(cmd)
        .stdin(std::process::Stdio::piped())
        .spawn();
    match child {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(serde_json::to_string(changes).unwrap_or_default().as_bytes());
            }
            let _ = child.wait();
        }
        Err(e) => eprintln!("emx: --exec: {e}"),
    }
}

// --- Anything ------------------------------------------------------------

fn api(emx: &Client, rest: &[&str], a: &Args) -> Result<(), Fail> {
    let (method, path) = match rest {
        [m, p, ..] => (m.to_uppercase(), *p),
        _ => return Err(usage("api GET|POST|PATCH|PUT|DELETE /api/... [--data JSON]")),
    };
    if !path.starts_with("/api/") {
        return Err(usage("the path starts with /api/"));
    }
    let v = if method == "GET" {
        emx.get_json(path, &[])?
    } else {
        let body: serde_json::Value = match a.value("data") {
            Some(d) => {
                serde_json::from_str(d).map_err(|e| Fail::Local(format!("--data is not JSON: {e}")))?
            }
            None => serde_json::Value::Null,
        };
        emx.call_json(&method, path, &body)?
    };
    outln!("{}", serde_json::to_string_pretty(&v).map_err(local)?);
    Ok(())
}

// --- Helpers -------------------------------------------------------------

fn out<T: serde::Serialize>(v: &T) -> Result<(), Fail> {
    outln!("{}", serde_json::to_string_pretty(v).map_err(local)?);
    Ok(())
}

/// Writes to standard output. When the reader has gone away, as with
/// `emx --json list | head -1`, the program ends quietly and successfully,
/// the way other command-line tools do, where `println!` would panic.
fn emit(args: std::fmt::Arguments<'_>) {
    if let Err(e) = std::io::stdout().lock().write_fmt(args) {
        output_failed(e);
    }
}

/// [`emit`] for bytes: a raw message or an attachment.
fn emit_bytes(data: &[u8]) {
    let mut out = std::io::stdout().lock();
    if let Err(e) = out.write_all(data).and_then(|_| out.flush()) {
        output_failed(e);
    }
}

fn output_failed(e: std::io::Error) -> ! {
    if e.kind() == std::io::ErrorKind::BrokenPipe {
        std::process::exit(0);
    }
    eprintln!("emx: writing the output: {e}");
    std::process::exit(1);
}

fn usage(s: &str) -> Fail {
    Fail::Local(format!("usage: emx {s}"))
}

fn config_err(s: String) -> Fail {
    Fail::Local(s)
}

fn trim(s: &str, n: usize) -> String {
    let s = s.replace(['\n', '\r'], " ");
    if s.chars().count() <= n {
        s
    } else {
        let cut: String = s.chars().take(n - 1).collect();
        format!("{cut}\u{2026}")
    }
}

/// A plain reading of HTML for a terminal: tags removed, block ends as
/// line breaks, style and script dropped, the common entities decoded.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut chars = html.chars().peekable();
    let mut tag = String::new();
    while let Some(c) = chars.next() {
        if c != '<' {
            out.push(c);
            continue;
        }
        tag.clear();
        for t in chars.by_ref() {
            if t == '>' {
                break;
            }
            tag.push(t);
        }
        let closing = tag.starts_with('/');
        let name = tag
            .trim_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_lowercase();
        if !closing && (name == "style" || name == "script") {
            // Skip to the closing tag.
            let end = format!("</{name}");
            let mut window = String::new();
            for t in chars.by_ref() {
                window.push(t);
                if window.to_lowercase().ends_with(&end) {
                    for t in chars.by_ref() {
                        if t == '>' {
                            break;
                        }
                    }
                    break;
                }
            }
            continue;
        }
        if matches!(
            name.as_str(),
            "p" | "div" | "br" | "tr" | "li" | "h1" | "h2" | "h3" | "h4" | "blockquote" | "table"
        ) && !out.ends_with('\n')
            && !out.is_empty()
        {
            out.push('\n');
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let mut lines: Vec<&str> = out.lines().map(|l| l.trim()).collect();
    lines.dedup_by(|a, b| a.is_empty() && b.is_empty());
    lines.join("\n").trim().to_string()
}

/// The media type of an attachment, from its file name's extension. The
/// service keeps what it is given, and an attachment without a type
/// arrives as application/octet-stream, which a mail app will not open
/// or preview. The table covers what people attach to business mail.
fn content_type(filename: &str) -> &'static str {
    let ext = match filename.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_ascii_lowercase(),
        _ => return "application/octet-stream",
    };
    match ext.as_str() {
        "pdf" => "application/pdf",
        "txt" | "text" | "log" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "ics" => "text/calendar",
        "vcf" => "text/vcard",
        "eml" => "message/rfc822",
        "json" => "application/json",
        "xml" => "application/xml",
        "rtf" => "application/rtf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "heic" => "image/heic",
        "tif" | "tiff" => "image/tiff",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "7z" => "application/x-7z-compressed",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "odp" => "application/vnd.oasis.opendocument.presentation",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    fn args(list: &[&str]) -> Args {
        Args::parse(list.iter().map(|s| s.to_string())).unwrap()
    }

    /// A client for a port nothing listens on: a test that reaches the
    /// network fails loudly instead of passing by accident.
    fn offline() -> Client {
        Client::with_base_url("http://127.0.0.1:9", "emx_test").unwrap()
    }

    fn local_message(r: Result<(), Fail>) -> String {
        match r {
            Err(Fail::Local(m)) => m,
            Err(Fail::Emx(e)) => panic!("expected a local failure, got {e}"),
            Ok(()) => panic!("expected a failure"),
        }
    }

    #[test]
    fn a_missing_file_is_a_local_failure_with_its_name() {
        let a = args(&[
            "send",
            "--from",
            "me@example.ch",
            "--to",
            "anna@example.ch",
            "--text",
            "Hi",
            "--attach",
            "/nonexistent/Offerte.pdf",
        ]);
        let m = local_message(send(&offline(), &a, false));
        assert!(m.starts_with("/nonexistent/Offerte.pdf: "), "{m}");
        let a = args(&[
            "send",
            "--from",
            "me@example.ch",
            "--to",
            "a@example.ch",
            "--body-file",
            "/nonexistent/body.txt",
        ]);
        let m = local_message(send(&offline(), &a, false));
        assert!(m.starts_with("/nonexistent/body.txt: "), "{m}");
    }

    #[test]
    fn bad_json_in_data_is_a_local_failure() {
        let a = args(&["api", "POST", "/api/me/prefs", "--data", "{screener: true}"]);
        let m = local_message(api(&offline(), &["POST", "/api/me/prefs"], &a));
        assert!(m.starts_with("--data is not JSON: "), "{m}");
    }

    #[test]
    fn attachments_get_a_type_from_their_extension() {
        assert_eq!(content_type("Offerte.pdf"), "application/pdf");
        assert_eq!(content_type("Logo.PNG"), "image/png");
        assert_eq!(content_type("Belege 2026.tar.gz"), "application/gzip");
        assert_eq!(
            content_type("Budget.xlsx"),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        );
        assert_eq!(content_type("README"), "application/octet-stream");
        assert_eq!(content_type(".profile"), "application/octet-stream");
        assert_eq!(content_type("data.unknownext"), "application/octet-stream");
    }

    #[test]
    fn html_reads_as_text() {
        let html = "<style>p{}</style><p>Hello &amp; welcome</p><p>Second<br>line</p><script>x</script>";
        assert_eq!(strip_tags(html), "Hello & welcome\nSecond\nline");
        let html =
            "<p>Danke für die Offerte, wir sind <b>dabei</b>. <img data-src=\"x\"/></p><p>Gruss<br/>Anna</p>";
        assert_eq!(
            strip_tags(html),
            "Danke für die Offerte, wir sind dabei.\nGruss\nAnna"
        );
    }

    #[test]
    fn trims_by_characters() {
        assert_eq!(trim("Grüezi mitenand", 8), "Grüezi \u{2026}");
        assert_eq!(trim("short\nline", 20), "short line");
    }
}
