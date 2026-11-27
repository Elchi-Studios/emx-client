//! The system tray with the count of unread mail, and a notification for
//! each new message.
//!
//! Notifications go through the system's own notification service, and
//! are plain ones: Windows (Focus Assist), macOS (Focus) and the Linux
//! desktops (Do Not Disturb) hold such notifications back while the
//! person has asked not to be disturbed, which is what is wanted. The
//! app asks for nothing urgent that would break through.

use emx_sdk::{Client, Message};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_notification::NotificationExt;

const TRAY: &str = "main";

/// The unread count of each account's inbox, and the tray's menu line
/// that shows the total.
pub struct Unread {
    counts: Mutex<HashMap<String, i64>>,
    line: MenuItem<Wry>,
}

/// The line in the tray's menu.
pub fn menu_line(total: i64) -> String {
    match total {
        0 => "No unread mail".into(),
        1 => "1 unread message".into(),
        n => format!("{n} unread messages"),
    }
}

/// The tray's tooltip.
pub fn tooltip(total: i64) -> String {
    if total == 0 {
        "EMX".into()
    } else {
        format!("EMX, {total} unread")
    }
}

/// Shows the main window again.
pub fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Puts the icon in the tray with its menu.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let line = MenuItem::with_id(app, "unread", menu_line(0), false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open EMX", true, None::<&str>)?;
    let update = MenuItem::with_id(app, "update", "Check for updates", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit EMX", true, None::<&str>)?;
    let rule = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&line, &rule, &open, &update, &quit])?;
    let mut tray = TrayIconBuilder::with_id(TRAY)
        .tooltip(tooltip(0))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_window(app),
            "update" => crate::update::check_by_hand(app.clone()),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    app.manage(Unread {
        counts: Mutex::new(HashMap::new()),
        line,
    });
    Ok(())
}

/// Records the unread count of one account and updates the tray.
pub fn set_unread(app: &AppHandle, account: &str, count: i64) {
    let Some(u) = app.try_state::<Unread>() else {
        return;
    };
    let total = {
        let mut counts = u.counts.lock().unwrap();
        counts.insert(account.to_string(), count);
        counts.values().sum::<i64>()
    };
    let _ = u.line.set_text(menu_line(total));
    if let Some(tray) = app.tray_by_id(TRAY) {
        let _ = tray.set_tooltip(Some(tooltip(total)));
        // Only macOS shows text beside the icon.
        #[cfg(target_os = "macos")]
        let _ = tray.set_title(if total > 0 { Some(total.to_string()) } else { None });
    }
}

/// Forgets the counts, at sign-out.
pub fn clear_unread(app: &AppHandle) {
    if let Some(u) = app.try_state::<Unread>() {
        u.counts.lock().unwrap().clear();
    }
    set_unread(app, "", 0);
}

/// What a notification says: who wrote, and about what. A sealed
/// message has no subject to show.
pub fn wording(m: &Message) -> (String, String) {
    let who = if m.from.name.is_empty() {
        m.from.address.clone()
    } else {
        m.from.name.clone()
    };
    if m.sealed {
        ("New message".into(), String::new())
    } else {
        (who, m.subject.clone())
    }
}

/// The messages among `updated` that are new in the inbox: unread, not a
/// draft, and not seen by an earlier call. Everything looked at is
/// remembered, so a later change to the same message (a flag, a move
/// away and back) does not announce it again.
pub fn fresh<'a>(updated: &'a [Message], inbox: &str, known: &mut HashSet<String>) -> Vec<&'a Message> {
    let mut out = Vec::new();
    for m in updated {
        if !known.insert(m.id.clone()) {
            continue;
        }
        if m.mailbox_id == inbox && !m.is_seen() && !m.keywords.iter().any(|k| k == "$draft") {
            out.push(m);
        }
    }
    out
}

/// Posts one notification. A failure (no notification service, refused
/// permission) is not worth telling the person about.
pub fn announce(app: &AppHandle, title: &str, body: &str) {
    let mut n = app.notification().builder().title(title);
    if !body.is_empty() {
        n = n.body(body);
    }
    let _ = n.show();
}

/// What the watcher of one account knows between changes.
#[derive(Default)]
pub struct Follower {
    inbox: Option<String>,
    since: i64,
    known: HashSet<String>,
}

impl Follower {
    /// Handles one change of the account at `modseq`. The first call
    /// only learns the starting point: nothing that was already there is
    /// announced.
    pub fn changed(&mut self, app: &AppHandle, client: &Client, account: &str, modseq: i64) {
        let first = self.inbox.is_none();
        if first {
            let Ok(boxes) = client.mailboxes(account) else {
                return;
            };
            let Some(inbox) = boxes.iter().find(|b| b.role == "inbox") else {
                return;
            };
            if let Ok(page) = client.messages(account, &inbox.id, 50, "") {
                self.known.extend(page.items.into_iter().map(|m| m.id));
            }
            self.inbox = Some(inbox.id.clone());
            self.since = modseq;
            set_unread(app, account, inbox.unseen);
            return;
        }
        let inbox = self.inbox.clone().unwrap_or_default();
        let mut since = self.since;
        loop {
            let Ok(changes) = client.changes(account, since) else {
                return;
            };
            for m in fresh(&changes.updated, &inbox, &mut self.known) {
                let (title, body) = wording(m);
                announce(app, &title, &body);
            }
            since = changes.modseq.max(since);
            if !changes.has_more {
                break;
            }
        }
        self.since = since.max(modseq);
        if let Ok(boxes) = client.mailboxes(account) {
            if let Some(b) = boxes.iter().find(|b| b.id == inbox) {
                set_unread(app, account, b.unseen);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, mailbox: &str, keywords: &[&str]) -> Message {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "mailboxId": mailbox,
            "receivedAt": "2026-11-27T10:00:00Z",
            "subject": "Offerte",
            "from": {"name": "Anna", "address": "anna@example.ch"},
            "keywords": keywords,
        }))
        .unwrap()
    }

    #[test]
    fn only_new_unread_inbox_mail_is_announced() {
        let mut known: HashSet<String> = ["old".to_string()].into();
        let updated = vec![
            msg("old", "inbox", &[]),
            msg("new", "inbox", &[]),
            msg("read", "inbox", &["$seen"]),
            msg("draft", "inbox", &["$draft"]),
            msg("sent", "other", &[]),
        ];
        let got: Vec<&str> = fresh(&updated, "inbox", &mut known)
            .iter()
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(got, ["new"]);
        // A second change to the same messages announces nothing.
        assert!(fresh(&updated, "inbox", &mut known).is_empty());
    }

    #[test]
    fn a_sealed_message_shows_neither_sender_nor_subject() {
        let mut m = msg("a", "inbox", &[]);
        assert_eq!(wording(&m), ("Anna".into(), "Offerte".into()));
        m.sealed = true;
        assert_eq!(wording(&m), ("New message".into(), String::new()));
        m.sealed = false;
        m.from.name.clear();
        assert_eq!(wording(&m).0, "anna@example.ch");
    }

    #[test]
    fn the_tray_says_how_much_is_unread() {
        assert_eq!(menu_line(0), "No unread mail");
        assert_eq!(menu_line(1), "1 unread message");
        assert_eq!(menu_line(12), "12 unread messages");
        assert_eq!(tooltip(0), "EMX");
        assert_eq!(tooltip(3), "EMX, 3 unread");
    }
}
