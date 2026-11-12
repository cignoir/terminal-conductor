//! Auto-update from GitHub Releases (tauri-plugin-updater).
//!
//! check → download (progress events) → install only when the user asks.
//! Download and install are separate on purpose: on Windows the plugin starts
//! the installer and exits the process, so installing must not happen until
//! the user has agreed and the panes have been shut down cleanly.

use serde_json::{json, Value};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Overrides the update endpoint. Used to test the update flow end to end
/// against a local server. Updates are still verified against the embedded
/// public key, so this cannot be used to install unsigned builds.
const ENDPOINT_ENV: &str = "TC_UPDATE_ENDPOINT";

/// The update found by `updater_check`.
pub struct PendingUpdate(pub Mutex<Option<Update>>);

/// The downloaded installer, used by `updater_install`.
pub struct DownloadedUpdate(pub Mutex<Option<Vec<u8>>>);

fn updater(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    let mut builder = app.updater_builder();
    if let Ok(url) = std::env::var(ENDPOINT_ENV) {
        let url = Url::parse(&url).map_err(|e| format!("{ENDPOINT_ENV}: {e}"))?;
        builder = builder.endpoints(vec![url]).map_err(|e| e.to_string())?;
    }
    builder.build().map_err(|e| e.to_string())
}

/// Checks for a newer version. Returns its version and release notes.
#[tauri::command]
pub async fn updater_check(app: AppHandle) -> Result<Value, String> {
    // Dev builds would compare against the released version and offer to
    // overwrite the installed app, so they never check.
    if cfg!(debug_assertions) && std::env::var(ENDPOINT_ENV).is_err() {
        return Ok(json!({ "available": false, "dev": true }));
    }
    match updater(&app)?.check().await {
        Ok(Some(update)) => {
            let version = update.version.clone();
            let notes = update.body.clone();
            let state = app.state::<PendingUpdate>();
            let mut pending = state.0.lock().unwrap();
            // A different version invalidates an earlier download.
            if pending.as_ref().map(|u| &u.version) != Some(&version) {
                *app.state::<DownloadedUpdate>().0.lock().unwrap() = None;
            }
            *pending = Some(update);
            Ok(json!({ "available": true, "version": version, "notes": notes }))
        }
        Ok(None) => Ok(json!({ "available": false })),
        Err(e) => Err(e.to_string()),
    }
}

/// Downloads the installer only. Emits `update:progress` and `update:downloaded`.
#[tauri::command]
pub async fn updater_download(app: AppHandle) -> Result<(), String> {
    // Clone rather than take, so a failed download can be retried without
    // checking again.
    let update = app.state::<PendingUpdate>().0.lock().unwrap().clone();
    let Some(update) = update else {
        return Err("No update has been found".into());
    };

    let mut downloaded: usize = 0;
    let progress_app = app.clone();
    let bytes = update
        .download(
            move |chunk, total| {
                downloaded += chunk;
                let percent = total
                    .map(|t| (downloaded as f64 / t as f64 * 100.0).min(100.0))
                    .unwrap_or(0.0);
                let _ = progress_app.emit("update:progress", json!({ "percent": percent }));
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;

    *app.state::<DownloadedUpdate>().0.lock().unwrap() = Some(bytes);
    let _ = app.emit("update:downloaded", json!({ "version": update.version }));
    Ok(())
}

/// Stops every pane cleanly, then runs the downloaded installer. On Windows
/// the installer replaces the app and the process exits, so this does not
/// return on success. The frontend saves its documents before calling this.
#[tauri::command]
pub async fn updater_install(app: AppHandle) -> Result<(), String> {
    let update = app.state::<PendingUpdate>().0.lock().unwrap().clone();
    let Some(update) = update else {
        return Err("No update has been found".into());
    };
    let bytes = app.state::<DownloadedUpdate>().0.lock().unwrap().take();
    let Some(bytes) = bytes else {
        return Err("The update has not been downloaded yet".into());
    };

    // Let Claude Code exit on its own and save session state first.
    let shutdown_app = app.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || crate::shutdown_app(&shutdown_app)).await;

    if let Err(e) = update.install(&bytes) {
        // Keep the download so the user can retry without downloading again.
        *app.state::<DownloadedUpdate>().0.lock().unwrap() = Some(bytes);
        return Err(e.to_string());
    }
    app.restart();
}
