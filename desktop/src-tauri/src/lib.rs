//! The Rust side of the app: holds the token, talks to EMX through the
//! SDK, and follows the account so the window hears about new mail. The
//! window never sees the token; it asks for what it needs by name.

use emx_sdk::{Changes, Client, Contact, Draft, Error, FullMessage, Mailbox, Me, Message, Page};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

/// What the app keeps between calls.
#[derive(Default)]
struct App {
    client: Option<Client>,
    /// Ends the watcher of the previous sign-in.
    watching: Option<Arc<std::sync::atomic::AtomicBool>>,
}

type Shared = Mutex<App>;

/// An error the window can show.
#[derive(Serialize)]
struct Failure {
    code: String,
    message: String,
}

impl From<Error> for Failure {
    fn from(e: Error) -> Self {
        Failure {
            code: e.code().unwrap_or("error").to_string(),
            message: match &e {
                Error::Api { status: 401, .. } => "The token was refused. Sign in again with a current one.".into(),
                _ => e.to_string(),
            },
        }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure { code: "app".into(), message }
    }
}

type Outcome<T> = Result<T, Failure>;

fn client(state: &State<Shared>) -> Outcome<Client> {
    state
        .lock()
        .unwrap()
        .client
        .clone()
        .ok_or_else(|| Failure { code: "signed_out".into(), message: "Not signed in.".into() })
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
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let f = opts.open(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    serde_json::to_writer(f, &body).map_err(|e| e.to_string())
}

fn load_token(app: &AppHandle) -> Option<(String, String)> {
    let raw = fs::read_to_string(token_path(app).ok()?).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let base = v.get("baseUrl")?.as_str()?.to_string();
    let token = v.get("token")?.as_str()?.to_string();
    Some((base, token))
}

// --- Sign in -------------------------------------------------------------

fn connect(app: &AppHandle, state: &State<Shared>, base_url: &str, token: &str) -> Outcome<Me> {
    let c = Client::with_base_url(base_url, token)
        .map_err(Failure::from)?
        .user_agent(&format!("emx-desktop/{}", env!("CARGO_PKG_VERSION")));
    let me = c.me()?;
    let mut s = state.lock().unwrap();
    if let Some(stop) = s.watching.take() {
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    s.client = Some(c.clone());
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    s.watching = Some(stop.clone());
    drop(s);
    for account in &me.accounts {
        watch(app.clone(), c.clone(), account.id.clone(), stop.clone());
    }
    Ok(me)
}

/// Signs in with a token and keeps it for next time.
#[tauri::command]
fn sign_in(app: AppHandle, state: State<Shared>, base_url: String, token: String) -> Outcome<Me> {
    let base = if base_url.trim().is_empty() { emx_sdk::DEFAULT_BASE_URL.to_string() } else { base_url };
    let me = connect(&app, &state, &base, token.trim())?;
    save_token(&app, &base, token.trim())?;
    Ok(me)
}

/// Signs in with the kept token, if there is one.
#[tauri::command]
fn resume(app: AppHandle, state: State<Shared>) -> Outcome<Option<Me>> {
    match load_token(&app) {
        Some((base, token)) => connect(&app, &state, &base, &token).map(Some),
        None => Ok(None),
    }
}

/// Forgets the token here. Revoking it is done in the web client.
#[tauri::command]
fn sign_out(app: AppHandle, state: State<Shared>) -> Outcome<()> {
    let mut s = state.lock().unwrap();
    s.client = None;
    if let Some(stop) = s.watching.take() {
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if let Ok(p) = token_path(&app) {
        let _ = fs::remove_file(p);
    }
    Ok(())
}

// --- Mail ----------------------------------------------------------------

#[tauri::command]
fn me(state: State<Shared>) -> Outcome<Me> {
    Ok(client(&state)?.me()?)
}

#[tauri::command]
fn mailboxes(state: State<Shared>, account: String) -> Outcome<Vec<Mailbox>> {
    Ok(client(&state)?.mailboxes(&account)?)
}

#[derive(Serialize)]
struct MessagePage {
    messages: Vec<Message>,
    cursor: String,
}

#[tauri::command]
fn messages(state: State<Shared>, account: String, mailbox: String, cursor: String) -> Outcome<MessagePage> {
    let Page { items, cursor } = client(&state)?.messages(&account, &mailbox, 50, &cursor)?;
    Ok(MessagePage { messages: items, cursor })
}

#[tauri::command]
fn message(state: State<Shared>, account: String, id: String) -> Outcome<FullMessage> {
    Ok(client(&state)?.message(&account, &id)?)
}

#[tauri::command]
fn thread(state: State<Shared>, account: String, id: String) -> Outcome<Vec<Message>> {
    Ok(client(&state)?.thread(&account, &id)?)
}

#[tauri::command]
fn search(state: State<Shared>, account: String, query: String) -> Outcome<Vec<Message>> {
    Ok(client(&state)?.search(&account, &query)?)
}

#[tauri::command]
fn changes(state: State<Shared>, account: String, since: i64) -> Outcome<Changes> {
    Ok(client(&state)?.changes(&account, since)?)
}

#[tauri::command]
fn keywords(
    state: State<Shared>,
    account: String,
    ids: Vec<String>,
    add: Vec<String>,
    remove: Vec<String>,
) -> Outcome<Vec<String>> {
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let add: Vec<&str> = add.iter().map(String::as_str).collect();
    let remove: Vec<&str> = remove.iter().map(String::as_str).collect();
    Ok(client(&state)?.keywords(&account, &ids, &add, &remove)?)
}

#[tauri::command]
fn move_messages(state: State<Shared>, account: String, ids: Vec<String>, to: String) -> Outcome<Vec<String>> {
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    Ok(client(&state)?.r#move(&account, &ids, &to)?)
}

#[tauri::command]
fn delete_messages(state: State<Shared>, account: String, ids: Vec<String>) -> Outcome<Vec<String>> {
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    Ok(client(&state)?.delete(&account, &ids)?)
}

#[tauri::command]
fn snooze(state: State<Shared>, account: String, ids: Vec<String>, until: String) -> Outcome<Vec<String>> {
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    Ok(client(&state)?.snooze(&account, &ids, &until)?)
}

/// A part of a message, base64, with its content type, for saving or
/// showing.
#[derive(Serialize)]
struct PartData {
    content_type: String,
    base64: String,
}

#[tauri::command]
fn part(state: State<Shared>, account: String, id: String, part: String) -> Outcome<PartData> {
    let (data, content_type) = client(&state)?.part(&account, &id, &part)?;
    Ok(PartData { content_type, base64: base64(&data) })
}

/// Writes a part into the Downloads folder, under its own name, without
/// replacing a file that is there already. Answers with the path.
#[tauri::command]
fn save_part(app: AppHandle, state: State<Shared>, account: String, id: String, part: String, filename: String) -> Outcome<String> {
    let (data, _) = client(&state)?.part(&account, &id, &part)?;
    let dir = app.path().download_dir().map_err(|e| e.to_string())?;
    let name = safe_filename(&filename);
    let mut path = dir.join(&name);
    let mut n = 1;
    while path.exists() {
        n += 1;
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
            _ => (name.clone(), String::new()),
        };
        path = dir.join(format!("{stem} ({n}){ext}"));
    }
    fs::write(&path, &data).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path.display().to_string())
}

/// A file name with nothing that could leave the folder or upset a
/// file system.
fn safe_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        "attachment".into()
    } else {
        cleaned
    }
}

#[tauri::command]
fn send(state: State<Shared>, draft: Draft) -> Outcome<i64> {
    Ok(client(&state)?.send_mail(&draft)?.recipients)
}

#[tauri::command]
fn contacts(state: State<Shared>, account: String, contact_state: String) -> Outcome<Vec<Contact>> {
    Ok(client(&state)?.contacts(&account, &contact_state)?)
}

#[tauri::command]
fn set_contact(state: State<Shared>, account: String, address: String, contact_state: String) -> Outcome<()> {
    Ok(client(&state)?.set_contact(&account, &address, &contact_state, "")?)
}

// --- Following the account -------------------------------------------------

/// What the window hears when something changed.
#[derive(Serialize, Clone)]
struct ChangeNotice {
    account: String,
    modseq: i64,
}

/// Follows one account's event stream on a thread and tells the window
/// the modseq of each change. The window asks for the changes itself,
/// from the modseq it last handled, so a notice that arrives while it is
/// busy loses nothing. The stream is reopened when it ends, with a pause
/// that grows while the service is unreachable.
fn watch(app: AppHandle, client: Client, account: String, stop: Arc<std::sync::atomic::AtomicBool>) {
    std::thread::spawn(move || {
        let mut pause = 1u64;
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            let stream = match client.events(&account) {
                Ok(s) => s,
                Err(e) => {
                    if !e.is_retryable() {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(pause));
                    pause = (pause * 2).min(60);
                    continue;
                }
            };
            pause = 1;
            for event in stream {
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                match event {
                    Ok(emx_sdk::Event::Change { modseq }) => {
                        let _ = app.emit("emx://change", ChangeNotice { account: account.clone(), modseq });
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        }
    });
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Mutex::new(App::default()))
        .invoke_handler(tauri::generate_handler![
            sign_in,
            resume,
            sign_out,
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
