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

use args::{Args, Spec};
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
  search <words...> [--mailbox M] [--limit N] [--cursor C]
                             every word must occur, prefixes match; all folders
                             unless M is given

Change
  seen|unseen <ids...>       mark read or unread
  flag|unflag <ids...>
  move <ids...> --to M
  delete <ids...>            for good; move to Trash first if in doubt
  snooze <ids...> --until RFC3339

Send
  send --to A [--cc A] [--bcc A] --subject S [--from A]
       [--text T | --body-file F | stdin] [--html H]
       [--attach FILE]... [--in-reply-to MSGID] [--idempotency-key K]
                             --to, --cc and --bcc repeat, or take a list: a, b;
                             a send again with the same K within a day goes
                             out once at most (default: a new K each time)

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
  --account A     a shared mailbox, by id or address (default: your own)
  --json          machine-readable output
  --version, --help

The token comes from ~/.config/emx/config.json (emx login), or from
EMX_TOKEN and EMX_BASE_URL in the environment. Reference:
https://docs.elchi.dev/emx-api
";

fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => return fail_usage(&e),
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
    let Some(spec) = spec(command) else {
        return fail_usage(&format!("unknown command {command}; emx --help lists them"));
    };
    if let Err(e) = args.check(&spec) {
        return fail_usage(&e);
    }
    match run(command, &args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Fail::Usage(e)) => fail_usage(&e),
        Err(e) => fail(&describe(&e)),
    }
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("emx: {msg}");
    ExitCode::FAILURE
}

/// A command line `emx` does not understand ends with status 2, as with
/// other command-line tools, so that a script can tell it from a failure.
fn fail_usage(msg: &str) -> ExitCode {
    eprintln!("emx: {msg}");
    ExitCode::from(2)
}

/// The options and arguments each command takes; see [`Args::check`].
fn spec(command: &str) -> Option<Spec> {
    const NONE: &[&str] = &[];
    let s = |values, switches, min, max, usage| Spec {
        values,
        switches,
        min,
        max,
        usage,
    };
    Some(match command {
        "login" => s(&["base-url"], NONE, 0, Some(0), "login [--base-url URL]"),
        "logout" => s(NONE, NONE, 0, Some(0), "logout"),
        "me" => s(NONE, NONE, 0, Some(0), "me"),
        "mailboxes" => s(NONE, NONE, 0, Some(0), "mailboxes"),
        "list" => s(
            &["mailbox", "limit", "cursor"],
            NONE,
            0,
            Some(0),
            "list [--mailbox M] [--limit N] [--cursor C]",
        ),
        "read" => s(NONE, &["raw", "html"], 1, Some(1), "read <id> [--raw] [--html]"),
        "part" => s(
            &["part", "out"],
            NONE,
            1,
            Some(1),
            "part <id> --part P [--out FILE]",
        ),
        "thread" => s(NONE, NONE, 1, Some(1), "thread <id>"),
        "search" => s(
            &["mailbox", "limit", "cursor"],
            NONE,
            1,
            None,
            "search <words...> [--mailbox M] [--limit N] [--cursor C]",
        ),
        "seen" | "unseen" | "flag" | "unflag" => s(NONE, NONE, 1, None, "seen|unseen|flag|unflag <ids...>"),
        "move" => s(&["to"], NONE, 1, None, "move <ids...> --to M"),
        "delete" => s(NONE, NONE, 1, None, "delete <ids...>"),
        "snooze" => s(&["until"], NONE, 1, None, "snooze <ids...> --until RFC3339"),
        "send" => s(
            &[
                "from",
                "to",
                "cc",
                "bcc",
                "subject",
                "text",
                "html",
                "body-file",
                "attach",
                "reply-to",
                "in-reply-to",
                "idempotency-key",
            ],
            NONE,
            0,
            Some(0),
            "send --to A --subject S [--text T]",
        ),
        "contacts" => s(
            &["state"],
            NONE,
            0,
            Some(0),
            "contacts [--state approved|blocked]",
        ),
        "approve" => s(&["name"], NONE, 1, Some(1), "approve <address> [--name N]"),
        "block" => s(NONE, NONE, 1, Some(1), "block <address>"),
        "forget" => s(NONE, NONE, 1, Some(1), "forget <address>"),
        "webhooks" => s(NONE, NONE, 0, Some(0), "webhooks"),
        "webhook" => s(
            &["events", "description"],
            NONE,
            1,
            Some(2),
            "webhook add|delete|test|enable|deliveries ...",
        ),
        "watch" => s(&["exec"], NONE, 0, Some(0), "watch [--exec CMD]"),
        "api" => s(
            &["data"],
            NONE,
            2,
            Some(2),
            "api GET|POST|PATCH|PUT|DELETE /api/... [--data JSON]",
        ),
        _ => return None,
    })
}

fn run(command: &str, a: &Args) -> Result<(), Fail> {
    match command {
        "login" => return login(a).map_err(Fail::Local),
        "logout" => {
            config::remove().map_err(Fail::Local)?;
            outln!("Signed out. The token itself is revoked under Settings, Developer API.");
            return Ok(());
        }
        _ => {}
    }
    let cfg = config::load().map_err(Fail::Local)?;
    if cfg.token.is_empty() {
        return Err(Fail::Local("no token; run emx login, or set EMX_TOKEN".into()));
    }
    let emx = Client::with_base_url(&cfg.base_url, &cfg.token)
        .map_err(local)?
        .user_agent(&format!("emx-cli/{}", env!("CARGO_PKG_VERSION")));
    let account = match a.value("account") {
        // An address is looked up among the accounts the token reaches.
        Some(want) if want.contains('@') => {
            let me = emx.me()?;
            account_id(&me, want).map_err(Fail::Local)?
        }
        Some(want) => want.to_string(),
        None => ME.to_string(),
    };
    let account = account.as_str();
    let json = a.switch("json");
    let rest: Vec<&str> = a.positional.iter().skip(1).map(|s| s.as_str()).collect();
    match command {
        "me" => me(&emx, json),
        "mailboxes" => mailboxes(&emx, account, json),
        "list" => list(&emx, account, a, json),
        "read" => read(&emx, account, &rest, a, json),
        "part" => part(&emx, account, &rest, a),
        "thread" => thread(&emx, account, &rest, json),
        "search" => search(&emx, account, &rest, a, json),
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
        _ => Err(Fail::Usage(format!(
            "unknown command {command}; emx --help lists them"
        ))),
    }
}

/// Why a command failed: EMX refused or could not be reached, or
/// something on this computer went wrong (a file, the input, the
/// arguments). They are kept apart so that a missing file is not
/// reported as a network problem.
enum Fail {
    Emx(Error),
    Local(String),
    /// The command line is wrong: exit status 2.
    Usage(String),
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

/// The id of the account with the address `want`, from what `me` lists.
fn account_id(me: &emx_sdk::Me, want: &str) -> Result<String, String> {
    me.accounts
        .iter()
        .find(|acc| acc.address.eq_ignore_ascii_case(want.trim()))
        .map(|acc| acc.id.clone())
        .ok_or_else(|| format!("no account {want} for this token; emx me lists them"))
}

/// The address to send from when none is given: the person's own
/// address if it is a primary one, else the first primary, else the
/// first. With a shared mailbox there are several primaries, and the
/// order the service lists them in must not decide.
fn default_from(me: &emx_sdk::Me) -> Option<String> {
    let own = me
        .accounts
        .iter()
        .find(|acc| acc.rights.own)
        .map(|acc| acc.address.as_str());
    me.send_from
        .iter()
        .find(|s| s.primary && Some(s.address.as_str()) == own)
        .or_else(|| me.send_from.iter().find(|s| s.primary))
        .or(me.send_from.first())
        .map(|s| s.address.clone())
}

fn describe(e: &Fail) -> String {
    match e {
        Fail::Local(m) | Fail::Usage(m) => m.clone(),
        Fail::Emx(Error::Api { status: 401, .. }) => {
            "the token was refused; run emx login with a current one".into()
        }
        Fail::Emx(Error::Api { code, message, .. }) if code == "reload" => {
            format!("{message}; run the command again")
        }
        Fail::Emx(
            e @ Error::Api {
                retry_after: Some(s), ..
            },
        ) if e.is_retryable() => format!("{e}; try again in {s} seconds"),
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
    let limit = page_limit(a)?;
    let mailbox = find_mailbox(emx, account, a.value("mailbox").unwrap_or("inbox"))?;
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
            when(&m.received_at),
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
        // As received for a file or a pipe; on a terminal, without what
        // the terminal would take for a command.
        if std::io::stdout().is_terminal() {
            outp!("{}", clean(&String::from_utf8_lossy(&raw)));
        } else {
            emit_bytes(&raw);
        }
        return Ok(());
    }
    let full = emx.message(account, id)?;
    if json {
        return out(&full);
    }
    let m = &full.message;
    outln!("From:    {}", clean(&m.from.to_string()));
    outln!("To:      {}", clean(&m.to.join(", ")));
    if !m.cc.is_empty() {
        outln!("Cc:      {}", clean(&m.cc.join(", ")));
    }
    outln!("Date:    {}", when(&m.received_at));
    outln!("Subject: {}", clean(&m.subject));
    if full.body.sealed {
        outln!("\n[sealed: the body is encrypted to the mailbox's key and opens in the web client]");
        return Ok(());
    }
    for at in &full.body.attachments {
        outln!(
            "Part:    {}  {}  {} ({} bytes)",
            clean(&at.part),
            clean(&at.filename),
            clean(&at.content_type),
            at.size
        );
    }
    outln!();
    if a.switch("html") && !full.body.html.is_empty() {
        outln!("{}", clean(&full.body.html));
    } else if !full.body.text.is_empty() {
        outln!("{}", clean(&full.body.text.replace("\r\n", "\n")));
    } else if !full.body.html.is_empty() {
        outln!("{}", clean(&strip_tags(&full.body.html)));
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
        // A terminal shows text only; a PDF or an image would fill it
        // with noise, and with sequences the terminal takes as commands.
        None if std::io::stdout().is_terminal() => {
            if !kind.starts_with("text/") {
                return Err(Fail::Usage(format!(
                    "the part is {kind}; save it with --out FILE, or send the output to a file or program"
                )));
            }
            outp!("{}", clean(&String::from_utf8_lossy(&data)));
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

fn search(emx: &Client, account: &str, rest: &[&str], a: &Args, json: bool) -> Result<(), Fail> {
    if rest.is_empty() {
        return Err(usage("search <words...>"));
    }
    let limit = page_limit(a)?;
    let mailbox = match a.value("mailbox") {
        Some(m) => find_mailbox(emx, account, m)?.id,
        None => String::new(),
    };
    let page = emx.search_page(
        account,
        &rest.join(" "),
        &mailbox,
        limit,
        a.value("cursor").unwrap_or(""),
    )?;
    if json {
        return out(&serde_json::json!({"messages": page.items, "cursor": page.cursor}));
    }
    print_messages(&page.items);
    if !page.cursor.is_empty() {
        outln!("\nmore: --cursor {}", page.cursor);
    }
    Ok(())
}

/// `--limit` for a page: 30 when not given, at most 200.
fn page_limit(a: &Args) -> Result<u32, Fail> {
    let limit = a.number("limit", 30).map_err(config_err)?;
    if !(1..=200).contains(&limit) {
        return Err(Fail::Usage("--limit is a number from 1 to 200".into()));
    }
    Ok(limit)
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
        None => default_from(&emx.me()?)
            .ok_or_else(|| Fail::Local("no address to send from; pass --from A".into()))?,
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
    // One key for this message. When the answer is lost, the message may
    // have gone out or not; the error names the key, and the same command
    // with it sends the message if it did not go, and not a second time
    // if it did.
    let key = match a.value("idempotency-key") {
        Some(k) => k.to_string(),
        None => emx_sdk::idempotency_key(),
    };
    let sent = match emx.send_mail_with_key(&draft, &key) {
        Ok(sent) => sent,
        // A refusal with a status below 500, the rate limit among them,
        // came before anything was sent; after any other failure nobody
        // knows.
        Err(e @ (Error::Transport(_) | Error::Api { status: 500.., .. })) => {
            return Err(Fail::Local(format!(
                "{}\nThe message may have been sent. To send it again without a second copy, \
                 repeat the command with --idempotency-key {key}",
                describe(&Fail::Emx(e))
            )))
        }
        Err(e) => return Err(e.into()),
    };
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
                        Err(Fail::Emx(e)) if e.is_retryable() => {
                            // Waited out below, before the stream is opened
                            // again; the pause starts no shorter than asked.
                            backoff = backoff.max(retry_wait(&e, 1));
                            break;
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            // The network, the service, or the per-minute rate limit, which
            // another program with the same token may have used up: wait,
            // at least as long as the service asks, and try again. A
            // daemon must not end over a refusal that passes.
            Err(e) if e.is_retryable() => {
                let wait = retry_wait(&e, backoff);
                eprintln!("emx: {e}; trying again in {wait}s");
                std::thread::sleep(std::time::Duration::from_secs(wait));
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

/// How long to wait before trying again: the pause so far, or longer when
/// the service asks for longer with `Retry-After`, at most an hour.
fn retry_wait(e: &Error, backoff: u64) -> u64 {
    match e {
        Error::Api {
            retry_after: Some(s), ..
        } => backoff.max(*s).min(3600),
        _ => backoff,
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
                    outln!(
                        "{} {}  {}  {}",
                        m.modseq,
                        m.id,
                        trim(&m.from.to_string(), 60),
                        trim(&m.subject, 60)
                    );
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
    let child = shell(cmd).stdin(std::process::Stdio::piped()).spawn();
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

/// The system's shell, to run a command line as it was typed.
#[cfg(not(windows))]
fn shell(cmd: &str) -> std::process::Command {
    let mut c = std::process::Command::new("sh");
    c.arg("-c").arg(cmd);
    c
}

/// On Windows the line goes to cmd verbatim. Passed as an ordinary
/// argument, it would be quoted for a program, with a backslash before
/// every inner quote, which cmd does not understand. With `/s`, cmd
/// removes exactly the outer pair of quotes and runs what is inside as
/// written; `/d` leaves out any AutoRun command set in the registry.
#[cfg(windows)]
fn shell(cmd: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new("cmd");
    c.raw_arg(format!("/d /s /c \"{cmd}\""));
    c
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
    Fail::Usage(format!("usage: emx {s}"))
}

fn config_err(s: String) -> Fail {
    Fail::Usage(s)
}

/// A timestamp from the service, as date and minute with its zone: the
/// service sends UTC, and a time without a zone reads as local time. A
/// timestamp that carries no zone is shown without one.
fn when(ts: &str) -> String {
    let (Some(day), Some(minute)) = (ts.get(..10), ts.get(11..16)) else {
        return ts.to_string();
    };
    // After the minute come the seconds and their fraction, if any, and
    // then the zone, if any.
    let rest = ts.get(16..).unwrap_or("");
    let zone = match rest.trim_start_matches(|c: char| c == ':' || c == '.' || c.is_ascii_digit()) {
        "" => return format!("{day} {minute}"),
        "Z" | "z" | "+00:00" | "-00:00" => "UTC",
        zone => zone,
    };
    format!("{day} {minute} {zone}")
}

fn trim(s: &str, n: usize) -> String {
    let s = clean(&s.replace(['\n', '\r'], " "));
    if s.chars().count() <= n {
        s
    } else {
        let cut: String = s.chars().take(n - 1).collect();
        format!("{cut}\u{2026}")
    }
}

/// A plain reading of HTML for a terminal: tags and comments removed,
/// block ends as line breaks, style and script dropped, entities decoded
/// once. One pass over the text, however large a style block is.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        decode_entities(&rest[..i], &mut out);
        rest = &rest[i + 1..];
        if let Some(comment) = rest.strip_prefix("!--") {
            rest = comment.find("-->").map(|e| &comment[e + 3..]).unwrap_or("");
            continue;
        }
        let end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..end];
        rest = rest.get(end + 1..).unwrap_or("");
        let closing = tag.starts_with('/');
        let name = tag
            .trim_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if !closing && (name == "style" || name == "script") {
            // Skip to the end of the closing tag.
            rest = match find_ignore_case(rest, &format!("</{name}")) {
                Some(at) => {
                    let after = &rest[at..];
                    after.find('>').map(|e| &after[e + 1..]).unwrap_or("")
                }
                None => "",
            };
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
    decode_entities(rest, &mut out);
    let mut lines: Vec<&str> = out.lines().map(|l| l.trim()).collect();
    lines.dedup_by(|a, b| a.is_empty() && b.is_empty());
    lines.join("\n").trim().to_string()
}

/// Where `needle`, which is ASCII, first occurs in `hay`, in any case.
fn find_ignore_case(hay: &str, needle: &str) -> Option<usize> {
    let n = needle.as_bytes();
    hay.as_bytes()
        .windows(n.len())
        .position(|w| w.eq_ignore_ascii_case(n))
}

/// Appends `text` with its character references decoded, each once: the
/// named ones mail uses, and numeric ones such as `&#34;`, which is how
/// the service writes a double quote. `&amp;lt;` stays `&lt;`.
fn decode_entities(text: &str, out: &mut String) {
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest
            .char_indices()
            .take(12)
            .find(|&(_, c)| c == ';')
            .map(|(e, _)| e);
        let decoded = end.and_then(|e| {
            let name = &rest[1..e];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => {
                    let n = if let Some(hex) = name.strip_prefix("#x").or(name.strip_prefix("#X")) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        name.strip_prefix('#').and_then(|d| d.parse().ok())
                    };
                    n.and_then(char::from_u32)
                }
            }?;
            Some((c, e + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(if c == '\u{a0}' { ' ' } else { c });
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
}

/// Text from a message, safe to show on a terminal: control characters
/// other than line breaks and tabs are dropped. A mail could otherwise
/// set the window's title, clear the screen, write the clipboard or
/// disguise a link with escape sequences.
fn clean(s: &str) -> String {
    s.chars()
        .filter(|&c| c == '\n' || c == '\t' || !c.is_control())
        .collect()
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
            Err(Fail::Local(m) | Fail::Usage(m)) => m,
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
    fn entities_are_decoded_once() {
        assert_eq!(strip_tags("<p>Er sagte &#34;ja&#34;&#13;</p>"), "Er sagte \"ja\"");
        assert_eq!(
            strip_tags("<p>&amp;lt;b&amp;gt; is a tag</p>"),
            "&lt;b&gt; is a tag"
        );
        assert_eq!(
            strip_tags("A&nbsp;&#x41;&apos;s &unknown; & co &#xZZ;"),
            "A A's &unknown; & co &#xZZ;"
        );
        assert_eq!(strip_tags("<!-- a > b --><p>x</p>"), "x");
        assert_eq!(strip_tags("<STYLE>p{}</Style ><p>y</p><script>"), "y");
    }

    #[test]
    fn a_large_style_block_is_quick() {
        let html = format!("<style>{}</style><p>Hallo</p>", "p{color:red}".repeat(100_000));
        let started = std::time::Instant::now();
        assert_eq!(strip_tags(&html), "Hallo");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn terminal_sequences_are_dropped() {
        assert_eq!(
            clean("\u{1b}]0;PWNED\u{7}Hi \u{1b}[31mred\u{1b}[0m\u{9b}2J\r\n\tok"),
            "]0;PWNEDHi [31mred[0m2J\n\tok"
        );
        assert_eq!(trim("a\u{1b}[2Jb", 10), "a[2Jb");
    }

    #[test]
    fn times_carry_their_zone() {
        assert_eq!(when("2026-09-24T06:12:40Z"), "2026-09-24 06:12 UTC");
        assert_eq!(when("2026-09-24T06:12:40.123Z"), "2026-09-24 06:12 UTC");
        assert_eq!(when("2026-09-24T08:12:40+02:00"), "2026-09-24 08:12 +02:00");
        assert_eq!(when(""), "");
        assert_eq!(when("soon"), "soon");
        // Without a zone, no zone is made up from the seconds.
        assert_eq!(when("2026-09-24T06:12:40"), "2026-09-24 06:12");
        assert_eq!(when("2026-09-24T06:12:40.123456"), "2026-09-24 06:12");
        assert_eq!(when("2026-09-24T06:12:40.5-05:00"), "2026-09-24 06:12 -05:00");
        assert_eq!(when("2026-09-24T06:12Z"), "2026-09-24 06:12 UTC");
        assert_eq!(when("2026-09-24T06:12:40z"), "2026-09-24 06:12 UTC");
    }

    fn me_json(v: &str) -> emx_sdk::Me {
        let v = v.replacen('{', r#"{"tenant":{"id":"t1"},"#, 1);
        serde_json::from_str(&v).unwrap()
    }

    #[test]
    fn accounts_by_address_and_the_default_sender() {
        let me = me_json(
            r#"{"id":"u1","name":"Samuel","accounts":[
                {"id":"s1","address":"kontakt@elchi.test","rights":{"read":true}},
                {"id":"u1","address":"samuel@elchi.test","rights":{"own":true}}],
              "sendFrom":[{"address":"kontakt@elchi.test","primary":true},
                {"address":"samuel@elchi.test","primary":true},
                {"address":"s@elchi.test"}]}"#,
        );
        assert_eq!(account_id(&me, "Kontakt@Elchi.test").unwrap(), "s1");
        assert!(account_id(&me, "nobody@elchi.test").is_err());
        assert_eq!(default_from(&me).as_deref(), Some("samuel@elchi.test"));
        let me = me_json(
            r#"{"id":"u1","sendFrom":[{"address":"a@x.test"},{"address":"b@x.test","primary":true}]}"#,
        );
        assert_eq!(default_from(&me).as_deref(), Some("b@x.test"));
        assert_eq!(default_from(&me_json(r#"{"id":"u1"}"#)), None);
    }

    #[test]
    fn a_limit_outside_the_range_is_refused() {
        let m = local_message(list(&offline(), ME, &args(&["list", "--limit", "0"]), false));
        assert_eq!(m, "--limit is a number from 1 to 200");
    }

    #[test]
    fn watch_waits_as_long_as_the_service_asks() {
        let limited = Error::Api {
            status: 429,
            code: "rate_limited".into(),
            message: "slow down".into(),
            request_id: None,
            retry_after: Some(20),
        };
        assert_eq!(retry_wait(&limited, 1), 20);
        assert_eq!(retry_wait(&limited, 60), 60);
        assert_eq!(retry_wait(&Error::Transport("gone".into()), 4), 4);
    }

    #[test]
    fn trims_by_characters() {
        assert_eq!(trim("Grüezi mitenand", 8), "Grüezi \u{2026}");
        assert_eq!(trim("short\nline", 20), "short line");
    }
}
