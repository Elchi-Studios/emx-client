// SPDX-License-Identifier: AGPL-3.0-only

//! The Rust side of the app: holds the token, talks to EMX through the
//! SDK, and follows the account so the window hears about new mail. The
//! window never sees the token; it asks for what it needs by name.
//!
//! The SDK's calls block, a send for up to two and a half minutes. Every
//! command is therefore async and runs its call on a thread kept for
//! blocking work: a plain command runs on the main thread, and the
//! window would freeze while it waits.

use emx_sdk::{Changes, Client, Contact, Draft, Error, FullMessage, Mailbox, Me, Message, Page};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Emitter, Manager, State, Url, WebviewWindowBuilder};

/// What the app keeps between calls.
#[derive(Default)]
struct App {
    client: Option<Client>,
    /// Ends the watcher of the previous sign-in.
    watching: Option<Arc<AtomicBool>>,
    /// The accounts of the current sign-in, until the window asks to
    /// follow them: it does once it listens, so that it misses no notice.
    unfollowed: Vec<String>,
    /// Counts sign-ins begun and sign-outs. A sign-in takes a call to
    /// EMX, and the window can sign out meanwhile; the sign-in only
    /// takes effect if nothing came after it began.
    generation: u64,
}

impl App {
    /// Begins a sign-in and answers its number.
    fn begin(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// Completes sign-in `n` with its client and the accounts to follow,
    /// or answers false when a sign-out or a newer sign-in came since it
    /// began.
    fn complete(&mut self, n: u64, client: Client, accounts: Vec<String>) -> bool {
        if n != self.generation {
            return false;
        }
        self.stop_watching();
        self.client = Some(client);
        self.watching = Some(Arc::new(AtomicBool::new(false)));
        self.unfollowed = accounts;
        true
    }

    /// What the watchers of the current sign-in need, once: its client,
    /// the flag that ends them and its accounts. Nothing when signed out
    /// or when they run already.
    fn follow(&mut self) -> Option<(Client, Arc<AtomicBool>, Vec<String>)> {
        if self.unfollowed.is_empty() {
            return None;
        }
        let accounts = std::mem::take(&mut self.unfollowed);
        Some((self.client.clone()?, self.watching.clone()?, accounts))
    }

    /// Forgets the client, and makes any sign-in under way come to
    /// nothing.
    fn sign_out(&mut self) {
        self.generation += 1;
        self.client = None;
        self.stop_watching();
    }

    fn stop_watching(&mut self) {
        self.unfollowed.clear();
        if let Some(stop) = self.watching.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

type Shared = Mutex<App>;

/// An error the window can show. `retryable` says that the same call may
/// work in a moment: the network or the service is away, which is not a
/// reason to sign in again.
#[derive(Serialize, Debug, Clone)]
struct Failure {
    code: String,
    message: String,
    retryable: bool,
}

impl From<Error> for Failure {
    fn from(e: Error) -> Self {
        let retryable = e.is_retryable();
        let (code, message) = match &e {
            Error::Api {
                status: 401, code, ..
            } => (
                code.clone(),
                "The token was refused. Sign in again with a current one.".into(),
            ),
            Error::Transport(_) => (
                "offline".into(),
                "EMX cannot be reached. Check the internet connection and try again.".into(),
            ),
            // The per-minute limit: the only 429 that passes.
            Error::Api { status: 429, .. } if retryable => (
                "rate_limited".into(),
                "EMX received too many requests from this app in the last minute. Try again in a moment."
                    .into(),
            ),
            Error::Api {
                status: 429, code, ..
            } if code == "hourly_limit" => (
                code.clone(),
                "This hour's limit of recipients outside your organisation is reached. The message was not sent; send it again later."
                    .into(),
            ),
            Error::Api { status, code, .. } if retryable && *status >= 500 => (
                code.clone(),
                "EMX is not answering right now. Try again in a moment.".into(),
            ),
            _ => (e.code().unwrap_or("error").to_string(), e.to_string()),
        };
        Failure {
            code,
            message,
            retryable,
        }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure {
            code: "app".into(),
            message,
            retryable: false,
        }
    }
}

type Outcome<T> = Result<T, Failure>;

fn signed_out() -> Failure {
    Failure {
        code: "signed_out".into(),
        message: "Not signed in.".into(),
        retryable: false,
    }
}

fn client(state: &Shared) -> Outcome<Client> {
    state.lock().unwrap().client.clone().ok_or_else(signed_out)
}

/// Runs `f`, which blocks, on a thread kept for blocking work, so that
/// neither the main thread nor the async runtime's few threads wait for
/// the network.
async fn blocking<T, E>(f: impl FnOnce() -> Result<T, E> + Send + 'static) -> Outcome<T>
where
    T: Send + 'static,
    E: Send + 'static,
    Failure: From<E>,
{
    match tauri::async_runtime::spawn_blocking(f).await {
        Ok(result) => result.map_err(Failure::from),
        Err(e) => Err(<Failure as From<String>>::from(e.to_string())),
    }
}

// --- The token on disk ---------------------------------------------------

fn token_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir.join("session.json"))
}

fn save_token(app: &AppHandle, base_url: &str, token: &str) -> Result<(), String> {
    let p = token_path(app)?;
    let body = serde_json::json!({"baseUrl": base_url, "token": token});
    let data = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    write_private(&p, &data).map_err(|e| format!("{}: {e}", p.display()))
}

/// Writes `data` to `path` so that the file is either the old one or the
/// new one, never empty or half written, and readable by the person
/// alone, whatever permissions an earlier file had: a file beside it is
/// written and synced first, then put in its place.
fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        f.write_all(data)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn load_token(app: &AppHandle) -> Option<(String, String)> {
    let raw = fs::read_to_string(token_path(app).ok()?).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let base = v.get("baseUrl")?.as_str()?.to_string();
    let token = v.get("token")?.as_str()?.to_string();
    Some((base, token))
}

// --- Sign in -------------------------------------------------------------

/// Signs in with a token: asks EMX who it belongs to. The window starts
/// following the accounts with [`follow`] once it listens. With `keep`,
/// the token is written to disk as well, in the same step, so that a
/// sign-out meanwhile cannot leave it behind.
async fn connect(
    app: &AppHandle,
    state: &Shared,
    base_url: String,
    token: String,
    keep: bool,
) -> Outcome<Me> {
    let n = state.lock().unwrap().begin();
    let (c, me) = blocking({
        let (base_url, token) = (base_url.clone(), token.clone());
        move || {
            let mut c = Client::with_base_url(&base_url, &token)?
                .user_agent(&format!("emx-desktop/{}", env!("CARGO_PKG_VERSION")));
            // Certificate authorities to trust instead of the bundled
            // ones, as for the command line.
            if let Some(p) = std::env::var_os("EMX_CA_FILE").filter(|p| !p.is_empty()) {
                let pem = fs::read(&p)
                    .map_err(|e| Error::Config(format!("{}: {e}", std::path::Path::new(&p).display())))?;
                c = c.trust_only(&pem)?;
            }
            let me = c.me()?;
            Ok::<_, Error>((c, me))
        }
    })
    .await?;
    let mut s = state.lock().unwrap();
    if n != s.generation {
        return Err(signed_out());
    }
    if keep {
        save_token(app, &base_url, &token)?;
    }
    let accounts = me.accounts.iter().map(|a| a.id.clone()).collect();
    if !s.complete(n, c, accounts) {
        return Err(signed_out());
    }
    Ok(me)
}

/// Starts following the accounts of the sign-in, once the window
/// listens for what the watchers tell it: a notice sent before would be
/// lost. A second call does nothing.
#[tauri::command]
async fn follow(app: AppHandle, state: State<'_, Shared>) -> Outcome<()> {
    let Some((c, stop, accounts)) = state.lock().unwrap().follow() else {
        return Ok(());
    };
    for account in accounts {
        watch(app.clone(), c.clone(), account, stop.clone());
    }
    Ok(())
}

/// Signs in with a token and keeps it for next time.
#[tauri::command]
async fn sign_in(app: AppHandle, state: State<'_, Shared>, base_url: String, token: String) -> Outcome<Me> {
    let base = if base_url.trim().is_empty() {
        emx_sdk::DEFAULT_BASE_URL.to_string()
    } else {
        base_url
    };
    connect(&app, &state, base, token.trim().to_string(), true).await
}

/// Signs in with the kept token, if there is one. A failure that is
/// `retryable` leaves the token where it is: the window says it is
/// offline and calls this again, instead of asking to sign in.
#[tauri::command]
async fn resume(app: AppHandle, state: State<'_, Shared>) -> Outcome<Option<Me>> {
    match load_token(&app) {
        Some((base, token)) => connect(&app, &state, base, token, false).await.map(Some),
        None => Ok(None),
    }
}

/// Forgets the token here. Revoking it is done in the web client.
#[tauri::command]
async fn sign_out(app: AppHandle, state: State<'_, Shared>) -> Outcome<()> {
    let mut s = state.lock().unwrap();
    s.sign_out();
    if let Ok(p) = token_path(&app) {
        let _ = fs::remove_file(p);
    }
    Ok(())
}

// --- Mail ----------------------------------------------------------------

#[tauri::command]
async fn me(state: State<'_, Shared>) -> Outcome<Me> {
    let c = client(&state)?;
    blocking(move || c.me()).await
}

#[tauri::command]
async fn mailboxes(state: State<'_, Shared>, account: String) -> Outcome<Vec<Mailbox>> {
    let c = client(&state)?;
    blocking(move || c.mailboxes(&account)).await
}

#[derive(Serialize)]
struct MessagePage {
    messages: Vec<Message>,
    cursor: String,
}

#[tauri::command]
async fn messages(
    state: State<'_, Shared>,
    account: String,
    mailbox: String,
    cursor: String,
) -> Outcome<MessagePage> {
    let c = client(&state)?;
    let Page { items, cursor } = blocking(move || c.messages(&account, &mailbox, 50, &cursor)).await?;
    Ok(MessagePage {
        messages: items,
        cursor,
    })
}

#[tauri::command]
async fn message(state: State<'_, Shared>, account: String, id: String) -> Outcome<FullMessage> {
    let c = client(&state)?;
    blocking(move || c.message(&account, &id)).await
}

#[tauri::command]
async fn thread(state: State<'_, Shared>, account: String, id: String) -> Outcome<Vec<Message>> {
    let c = client(&state)?;
    blocking(move || c.thread(&account, &id)).await
}

/// One page of a search in every folder; pass the page's cursor for the
/// next one.
#[tauri::command]
async fn search(
    state: State<'_, Shared>,
    account: String,
    query: String,
    cursor: String,
) -> Outcome<MessagePage> {
    let c = client(&state)?;
    let Page { items, cursor } = blocking(move || c.search_page(&account, &query, "", 50, &cursor)).await?;
    Ok(MessagePage {
        messages: items,
        cursor,
    })
}

#[tauri::command]
async fn changes(state: State<'_, Shared>, account: String, since: i64) -> Outcome<Changes> {
    let c = client(&state)?;
    blocking(move || c.changes(&account, since)).await
}

fn strs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

#[tauri::command]
async fn keywords(
    state: State<'_, Shared>,
    account: String,
    ids: Vec<String>,
    add: Vec<String>,
    remove: Vec<String>,
) -> Outcome<Vec<String>> {
    let c = client(&state)?;
    blocking(move || c.keywords(&account, &strs(&ids), &strs(&add), &strs(&remove))).await
}

#[tauri::command]
async fn move_messages(
    state: State<'_, Shared>,
    account: String,
    ids: Vec<String>,
    to: String,
) -> Outcome<Vec<String>> {
    let c = client(&state)?;
    blocking(move || c.r#move(&account, &strs(&ids), &to)).await
}

#[tauri::command]
async fn delete_messages(
    state: State<'_, Shared>,
    account: String,
    ids: Vec<String>,
) -> Outcome<Vec<String>> {
    let c = client(&state)?;
    blocking(move || c.delete(&account, &strs(&ids))).await
}

#[tauri::command]
async fn snooze(
    state: State<'_, Shared>,
    account: String,
    ids: Vec<String>,
    until: String,
) -> Outcome<Vec<String>> {
    let c = client(&state)?;
    blocking(move || c.snooze(&account, &strs(&ids), &until)).await
}

/// A part of a message, base64, with its content type, for saving or
/// showing.
#[derive(Serialize)]
struct PartData {
    content_type: String,
    base64: String,
}

#[tauri::command]
async fn part(state: State<'_, Shared>, account: String, id: String, part: String) -> Outcome<PartData> {
    let c = client(&state)?;
    let (data, content_type) = blocking(move || c.part(&account, &id, &part)).await?;
    Ok(PartData {
        content_type,
        base64: base64(&data),
    })
}

/// Writes a part into the Downloads folder, under its own name, without
/// replacing a file that is there already. Answers with the path.
#[tauri::command]
async fn save_part(
    app: AppHandle,
    state: State<'_, Shared>,
    account: String,
    id: String,
    part: String,
    filename: String,
) -> Outcome<String> {
    let c = client(&state)?;
    let (data, _) = blocking(move || c.part(&account, &id, &part)).await?;
    let dir = app.path().download_dir().map_err(|e| e.to_string())?;
    blocking(move || save_new(&dir, &safe_filename(&filename), &data)).await
}

/// Writes `data` to `name` in `dir`, or to `name (2)` and so on when
/// that is taken, and answers the path.
fn save_new(dir: &std::path::Path, name: &str, data: &[u8]) -> Result<String, String> {
    let mut path = dir.join(name);
    let mut n = 1;
    while path.exists() {
        n += 1;
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
            _ => (name.to_string(), String::new()),
        };
        path = dir.join(format!("{stem} ({n}){ext}"));
    }
    fs::write(&path, data).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path.display().to_string())
}

/// The longest file name kept, in bytes: file systems allow 255, and
/// room is left for the ` (2)` of a name that is taken.
const NAME_MAX: usize = 200;

/// A file name with nothing that could leave the folder or upset a
/// file system: no separators or characters Windows refuses, no name
/// Windows keeps for a device (`CON`, `NUL.pdf`), and at most
/// [`NAME_MAX`] bytes, shortened in front of the extension.
fn safe_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut cleaned = cleaned.trim().trim_matches('.').trim().to_string();
    if cleaned.is_empty() {
        return "attachment".into();
    }
    let stem = cleaned
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ((stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.chars().count() == 4
        && stem
            .chars()
            .nth(3)
            .is_some_and(|c| c.is_ascii_digit() || "¹²³".contains(c)));
    if reserved {
        cleaned.insert(0, '_');
    }
    shorten(&cleaned, NAME_MAX)
}

/// `name` in at most `max` bytes, cut on a character boundary in front
/// of a short extension, which stays.
fn shorten(name: &str, max: usize) -> String {
    if name.len() <= max {
        return name.to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && e.len() <= 16 && !e.contains(' ') => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    let mut cut = max.saturating_sub(ext.len());
    while !stem.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{ext}", stem[..cut].trim_end())
}

/// Sends with the composer's idempotency key, the same each time Send
/// is pressed for one message, so that pressing it again after a
/// timeout cannot send the message twice.
#[tauri::command]
async fn send(state: State<'_, Shared>, draft: Draft, idempotency_key: String) -> Outcome<i64> {
    deliver(client(&state)?, draft, idempotency_key).await
}

async fn deliver(c: Client, draft: Draft, key: String) -> Outcome<i64> {
    Ok(blocking(move || c.send_mail_with_key(&draft, &key))
        .await?
        .recipients)
}

#[tauri::command]
async fn contacts(state: State<'_, Shared>, account: String, contact_state: String) -> Outcome<Vec<Contact>> {
    let c = client(&state)?;
    blocking(move || c.contacts(&account, &contact_state)).await
}

#[tauri::command]
async fn set_contact(
    state: State<'_, Shared>,
    account: String,
    address: String,
    contact_state: String,
) -> Outcome<()> {
    let c = client(&state)?;
    blocking(move || c.set_contact(&account, &address, &contact_state, "")).await
}

// --- Following the account -------------------------------------------------

/// What the window hears when something changed.
#[derive(Serialize, Clone)]
struct ChangeNotice {
    account: String,
    modseq: i64,
}

/// How often a watcher looks whether it should stop, while it waits for
/// the next event or before it connects again.
const LOOK: std::time::Duration = std::time::Duration::from_secs(1);

/// Waits `secs` seconds, or less when `stop` is set meanwhile. Answers
/// whether to go on.
fn pause(stop: &AtomicBool, secs: u64) -> bool {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < until {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(LOOK.min(until.saturating_duration_since(std::time::Instant::now())));
    }
    !stop.load(Ordering::Relaxed)
}

/// Follows one account's event stream on a thread and tells the window
/// the modseq of each change. The window asks for the changes itself,
/// from the modseq it last handled, so a notice that arrives while it is
/// busy loses nothing. The stream is reopened when it ends, with a pause
/// that grows while the service is unreachable. Every new stream starts
/// by announcing the current modseq; the window hears of it only when it
/// is above the last one passed on, which is when a change was missed
/// while the stream was down.
///
/// The thread looks at `stop` at least once a second, while it waits for
/// an event and while it pauses, and tells the window nothing once it is
/// set: after a sign-out, nothing more comes from the previous account.
fn watch(app: AppHandle, client: Client, account: String, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let mut wait = 1u64;
        let mut last = 0i64;
        while !stop.load(Ordering::Relaxed) {
            let opened = std::time::Instant::now();
            let mut stream = match client.events(&account) {
                Ok(s) => s,
                Err(e) => {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    if !e.is_retryable() {
                        // Refused for good, as an expired token is: the
                        // window hears of it, rather than going quiet.
                        let _ = app.emit("emx://refused", Failure::from(e));
                        return;
                    }
                    if !pause(&stop, wait) {
                        return;
                    }
                    wait = (wait * 2).min(60);
                    continue;
                }
            };
            loop {
                let next = stream.next_within(LOOK);
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                match next {
                    emx_sdk::Next::Event(emx_sdk::Event::Change { modseq }) if modseq > last => {
                        last = modseq;
                        let _ = app.emit(
                            "emx://change",
                            ChangeNotice {
                                account: account.clone(),
                                modseq,
                            },
                        );
                    }
                    emx_sdk::Next::Event(_) | emx_sdk::Next::Quiet => {}
                    emx_sdk::Next::Broken(_) | emx_sdk::Next::Ended => break,
                }
            }
            // A stream that keeps ending at once must not become a busy
            // loop; one that lasted resets the pause.
            if opened.elapsed() < std::time::Duration::from_secs(10) {
                if !pause(&stop, wait) {
                    return;
                }
                wait = (wait * 2).min(60);
            } else {
                wait = 1;
            }
        }
    });
}

// --- Links in a message ------------------------------------------------------

/// Whether a link from a message may leave the app for the system's
/// browser or mail program: web pages and mail addresses only, never a
/// file, a script or another program's scheme.
fn opens_outside(url: &Url) -> bool {
    match url.scheme() {
        "http" | "https" => url.host().is_some(),
        "mailto" => true,
        _ => false,
    }
}

/// A link clicked in a message: the message's frame asks for a new
/// window, since every link there opens in one. No window is made in the
/// app; the link goes to the system, if it is one that may.
fn open_link<R: tauri::Runtime>(url: Url) -> NewWindowResponse<R> {
    if opens_outside(&url) {
        let _ = open::that_detached(url.as_str());
    }
    NewWindowResponse::Deny
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Mutex::new(App::default()))
        // The window is made here rather than from the configuration
        // alone, so that it can hand links in a message to the system.
        .setup(|app| {
            let config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
                .ok_or("the main window is not configured")?;
            WebviewWindowBuilder::from_config(app.handle(), &config)?
                .on_new_window(|url, _| open_link(url))
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            sign_in,
            resume,
            sign_out,
            follow,
            me,
            mailboxes,
            messages,
            message,
            thread,
            search,
            changes,
            keywords,
            move_messages,
            delete_messages,
            snooze,
            part,
            save_part,
            send,
            contacts,
            set_contact,
        ])
        .run(tauri::generate_context!())
        .expect("the app could not start");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain command runs on the main thread, and the window freezes
    /// while it waits for EMX: every command here must be async.
    #[test]
    fn every_command_is_async() {
        let source = include_str!("lib.rs");
        let mut commands = 0;
        let mut lines = source.lines();
        while let Some(line) = lines.next() {
            if line.trim_start().starts_with("#[tauri::command") {
                commands += 1;
                let next = lines.next().unwrap_or("");
                assert!(next.trim_start().starts_with("async fn "), "not async: {next}");
            }
        }
        assert!(commands >= 20, "found {commands} commands");
    }

    #[test]
    fn the_token_file_is_replaced_whole_and_private() {
        let dir = std::env::temp_dir().join(format!("emx-desktop-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("session.json");
        fs::write(&p, "old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        }
        write_private(&p, b"new").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_names_fit_every_file_system() {
        assert_eq!(safe_filename("Rechnung 2026.pdf"), "Rechnung 2026.pdf");
        assert_eq!(safe_filename("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(safe_filename(" .. "), "attachment");
        assert_eq!(safe_filename("CON"), "_CON");
        assert_eq!(safe_filename("nul.pdf"), "_nul.pdf");
        assert_eq!(safe_filename("com1.txt"), "_com1.txt");
        assert_eq!(safe_filename("LPT9"), "_LPT9");
        assert_eq!(safe_filename("com\u{b9}.log"), "_com\u{b9}.log");
        assert_eq!(safe_filename("console.pdf"), "console.pdf");
        assert_eq!(safe_filename("com10.txt"), "com10.txt");
        let long = format!("{}.pdf", "\u{8acb}".repeat(90));
        let short = safe_filename(&long);
        assert!(short.len() <= NAME_MAX, "{}", short.len());
        assert!(short.ends_with("\u{8acb}.pdf"), "{short}");
        let no_ext = "a".repeat(300);
        assert_eq!(safe_filename(&no_ext).len(), NAME_MAX);
    }

    #[test]
    fn only_web_and_mail_links_leave_the_app() {
        let ok = |s: &str| opens_outside(&Url::parse(s).unwrap());
        assert!(ok("https://example.ch/rechnung?id=1"));
        assert!(ok("http://example.ch"));
        assert!(ok("mailto:anna@example.ch?subject=Offerte"));
        assert!(!ok("file:///etc/passwd"));
        assert!(!ok("javascript:alert(1)"));
        assert!(!ok("data:text/html,hi"));
        assert!(!ok("tel:+41441234567"));
        assert!(!ok("smb://server/share"));
        assert!(!ok("ms-settings:privacy"));
    }

    #[test]
    fn blocking_work_runs_away_from_the_caller() {
        let here = std::thread::current().id();
        let there = tauri::async_runtime::block_on(blocking(|| Ok::<_, String>(std::thread::current().id())))
            .unwrap();
        assert_ne!(here, there);
    }

    /// A server that answers every send as sent and reports the key it
    /// came with.
    fn send_server() -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let (mut key, mut length, mut line) = (String::new(), 0usize, String::new());
                while reader.read_line(&mut line).unwrap_or(0) > 0 && line != "\r\n" {
                    let lower = line.to_ascii_lowercase();
                    if lower.starts_with("idempotency-key:") {
                        key = line["idempotency-key:".len()..].trim().to_string();
                    }
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                    line.clear();
                }
                let _ = reader.read_exact(&mut vec![0u8; length]);
                let _ = tx.send(key);
                let body = r#"{"sent":true,"recipients":1}"#;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (base, rx)
    }

    #[test]
    fn a_send_carries_the_composers_key() {
        let (base, keys) = send_server();
        let c = Client::with_base_url(&base, "emx_test").unwrap();
        let draft = Draft {
            from: "samuel@elchi.dev".into(),
            to: "anna@example.ch".into(),
            ..Default::default()
        };
        for _ in 0..2 {
            let sent =
                tauri::async_runtime::block_on(deliver(c.clone(), draft.clone(), "emx-desktop-1".into()));
            assert_eq!(sent.unwrap(), 1);
            assert_eq!(keys.recv().unwrap(), "emx-desktop-1");
        }
        let empty = tauri::async_runtime::block_on(deliver(c, draft, String::new()));
        assert!(empty.is_err(), "a send without a key is refused");
    }

    fn a_client() -> Client {
        Client::with_base_url("http://127.0.0.1:9", "emx_test").unwrap()
    }

    #[test]
    fn a_sign_in_under_way_comes_to_nothing_after_sign_out() {
        let mut app = App::default();
        let n = app.begin();
        app.sign_out();
        assert!(!app.complete(n, a_client(), vec!["me".into()]));
        assert!(app.client.is_none());
        assert!(app.watching.is_none());
        assert!(app.follow().is_none());
    }

    #[test]
    fn the_accounts_are_followed_once_and_not_after_sign_out() {
        let mut app = App::default();
        let n = app.begin();
        assert!(app.complete(n, a_client(), vec!["me".into(), "shared".into()]));
        let (_, stop, accounts) = app.follow().unwrap();
        assert_eq!(accounts, vec!["me".to_string(), "shared".to_string()]);
        assert!(app.follow().is_none(), "a second call starts nothing");
        app.sign_out();
        assert!(stop.load(Ordering::Relaxed));

        let n = app.begin();
        assert!(app.complete(n, a_client(), vec!["me".into()]));
        app.sign_out();
        assert!(app.follow().is_none(), "nothing is followed after sign-out");
    }

    #[test]
    fn a_pause_ends_early_on_stop() {
        let stop = AtomicBool::new(true);
        let started = std::time::Instant::now();
        assert!(!pause(&stop, 60));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn the_newest_sign_in_wins() {
        let mut app = App::default();
        let older = app.begin();
        let newer = app.begin();
        assert!(app.complete(newer, a_client(), vec!["me".into()]));
        let stop = app.watching.clone().unwrap();
        assert!(!app.complete(older, a_client(), vec!["me".into()]));
        assert!(
            !stop.load(Ordering::Relaxed),
            "the newer sign-in's watchers go on"
        );
        app.sign_out();
        assert!(stop.load(Ordering::Relaxed), "sign-out ends the watchers");
    }

    #[test]
    fn failures_say_whether_signing_in_again_helps() {
        let offline = Failure::from(Error::Transport("io: Connection refused (os error 111)".into()));
        assert_eq!(offline.code, "offline");
        assert!(offline.retryable);
        assert!(!offline.message.contains("os error"), "{}", offline.message);

        let refused = Failure::from(Error::Api {
            status: 401,
            code: "bad_token".into(),
            message: "the token is not valid".into(),
            request_id: None,
            retry_after: None,
        });
        assert_eq!(refused.code, "bad_token");
        assert!(!refused.retryable);

        let down = Failure::from(Error::Api {
            status: 503,
            code: "http_503".into(),
            message: "HTTP 503".into(),
            request_id: None,
            retry_after: None,
        });
        assert!(down.retryable);
        assert!(!down.message.contains("HTTP 503"));

        let scope = Failure::from(Error::Api {
            status: 403,
            code: "scope".into(),
            message: "this token lacks the mail:write scope".into(),
            request_id: None,
            retry_after: None,
        });
        assert!(!scope.retryable);
        assert!(scope.message.contains("mail:write"));

        let busy = Failure::from(Error::Api {
            status: 429,
            code: "rate_limited".into(),
            message: "at most 600 requests per minute per token".into(),
            request_id: None,
            retry_after: Some(20),
        });
        assert_eq!(busy.code, "rate_limited");
        assert!(busy.retryable);
        assert!(busy.message.contains("too many requests"), "{}", busy.message);
        assert!(!busy.message.contains("HTTP 429"), "{}", busy.message);

        let hourly = Failure::from(Error::Api {
            status: 429,
            code: "hourly_limit".into(),
            message: "more than 300 external recipients this hour".into(),
            request_id: None,
            retry_after: None,
        });
        assert_eq!(hourly.code, "hourly_limit");
        assert!(!hourly.retryable);
        assert!(hourly.message.contains("not sent"), "{}", hourly.message);

        let daily = Failure::from(Error::Api {
            status: 429,
            code: "daily_limit".into(),
            message: "the daily sending limit is reached".into(),
            request_id: None,
            retry_after: None,
        });
        assert_eq!(daily.code, "daily_limit");
        assert!(!daily.retryable);
    }
}
