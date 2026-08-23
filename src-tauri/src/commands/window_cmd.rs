//! The game detail view lives in its own OS window rather than a modal
//! locked inside the main one (#187): it can be moved to a second monitor,
//! resized past the main window's bounds, and kept open next to the list.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

/// Label of the single detail window. Reused rather than spawning one window
/// per game — clicking through a library would otherwise bury the desktop.
const DETAIL_LABEL: &str = "detail";

/// Which game the detail window should be showing.
///
/// Passed through managed state instead of a URL query string so that
/// retargeting an already-open window is the same operation as opening it,
/// and so the id never has to survive a round trip through URL encoding.
#[derive(Default)]
pub struct DetailTarget(pub Mutex<Option<u64>>);

/// Open the detail window on `app_id`, or retarget and focus it if it is
/// already open.
#[tauri::command]
pub fn open_detail_window(app: AppHandle, app_id: u64) -> Result<(), String> {
    *app.state::<DetailTarget>().0.lock().unwrap() = Some(app_id);

    if let Some(existing) = app.get_webview_window(DETAIL_LABEL) {
        // Tell it to re-render before raising it, so the user never sees the
        // previous game in the foreground.
        existing
            .emit("detail-target-changed", app_id)
            .map_err(|e| e.to_string())?;
        let _ = existing.unminimize();
        existing.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    WebviewWindowBuilder::new(&app, DETAIL_LABEL, WebviewUrl::App("detail.html".into()))
        .title("Tatu — detalle")
        .inner_size(760.0, 900.0)
        .min_inner_size(420.0, 400.0)
        .resizable(true)
        .center()
        .build()
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// The game the detail window was opened for. Called by that window on boot.
#[tauri::command]
pub fn detail_target(app: AppHandle) -> Option<u64> {
    *app.state::<DetailTarget>().0.lock().unwrap()
}
