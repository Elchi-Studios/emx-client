//! Updating from the GitHub releases, through Tauri's updater. It asks
//! before it installs anything, and checks on its own once at start.
//!
//! A build without the updater's public key in `tauri.conf.json` (an
//! unsigned build from source) cannot verify an update, and so never
//! offers one; the check fails and the automatic one says nothing.

use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

/// Checks at start, and says something only when there is an update.
pub fn check_at_start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let _ = check(&app, false).await;
    });
}

/// Checks because the person asked: it also says when there is nothing
/// new, or when the check failed.
pub fn check_by_hand(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = check(&app, true).await {
            tell(
                &app,
                "EMX could not check for updates",
                &e.to_string(),
                MessageDialogKind::Error,
            )
            .await;
        }
    });
}

async fn tell(app: &AppHandle, title: &str, text: &str, kind: MessageDialogKind) {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    app.dialog().message(text).title(title).kind(kind).show(move |_| {
        let _ = tx.try_send(());
    });
    let _ = rx.recv().await;
}

/// Asks a yes or no question and waits for the answer.
async fn ask(app: &AppHandle, title: &str, text: &str, yes: &str, no: &str) -> bool {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    app.dialog()
        .message(text)
        .title(title)
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(yes.into(), no.into()))
        .show(move |answer| {
            let _ = tx.try_send(answer);
        });
    rx.recv().await.unwrap_or(false)
}

async fn check(app: &AppHandle, say_when_current: bool) -> Result<(), tauri_plugin_updater::Error> {
    let Some(update) = app.updater()?.check().await? else {
        if say_when_current {
            tell(
                app,
                "EMX is up to date",
                &format!("You have the newest version, {}.", app.package_info().version),
                MessageDialogKind::Info,
            )
            .await;
        }
        return Ok(());
    };
    let text = format!(
        "EMX {} is available, and you have {}. Install it now? EMX restarts to finish.",
        update.version, update.current_version
    );
    if !ask(app, "Update EMX", &text, "Install and restart", "Later").await {
        return Ok(());
    }
    update.download_and_install(|_, _| {}, || {}).await?;
    app.restart();
}
