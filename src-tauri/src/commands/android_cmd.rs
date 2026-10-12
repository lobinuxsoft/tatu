use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tatu_android::{Adb, Device};
use tauri::{AppHandle, Emitter, State};

use crate::SharedState;
use crate::android;

/// The in-flight send's stop signal — runtime-only coordination, same shape
/// and reasoning as `gog_cmd::GogDownloadCancel` (one modal, so one send at
/// a time; a new send replaces the slot rather than stacking).
#[derive(Default)]
pub struct AndroidSendCancel(pub Mutex<Option<Arc<AtomicBool>>>);

#[derive(serde::Serialize)]
pub struct AndroidGameFolder {
    path: String,
    /// The folder's own name — what it'll be called on the phone too.
    name: String,
    remote_root: String,
}

/// What the modal shows before anything touches the phone: which local
/// folder would be sent, and where to (last used, or the default). Fails
/// early — with a readable reason — for a Steam game that isn't installed
/// on this machine.
#[tauri::command]
pub fn android_game_folder(
    state: State<'_, SharedState>,
    source: String,
    id: u64,
) -> Result<AndroidGameFolder, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    let dir = crate::installed::game_folder(&s, &source, id)?;
    Ok(AndroidGameFolder {
        path: dir.display().to_string(),
        name: dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        remote_root: s.android.remote_root_or_default().to_string(),
    })
}

#[tauri::command]
pub async fn android_list_devices(state: State<'_, SharedState>) -> Result<Vec<Device>, String> {
    let adb_path = {
        let s = state.lock().map_err(|e| e.to_string())?;
        s.android.adb_path.clone()
    };
    tokio::task::spawn_blocking(move || Adb::locate(&adb_path).and_then(|adb| adb.devices()))
        .await
        .map_err(|e| format!("Task error: {e}"))?
        .map_err(|e| e.to_string())
}

/// For when adb isn't anywhere Tatu looks on its own. Empty goes back to
/// auto-detection.
#[tauri::command]
pub fn android_set_adb_path(state: State<'_, SharedState>, path: String) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.android.adb_path = path.trim().to_string();
    s.save();
    Ok(())
}

/// Wireless debugging, first time: the pairing address + 6-digit code from
/// the phone's "Vincular dispositivo con código". Wi-Fi is also the only way
/// power can be measured — over a USB cable the phone is charging.
#[tauri::command]
pub async fn android_pair(
    state: State<'_, SharedState>,
    address: String,
    code: String,
) -> Result<(), String> {
    let adb_path = adb_path(&state)?;
    tokio::task::spawn_blocking(move || Adb::locate(&adb_path)?.pair(address.trim(), code.trim()))
        .await
        .map_err(|e| format!("Task error: {e}"))?
        .map_err(|e| e.to_string())
}

/// `adb connect` to the address wireless debugging shows (not the pairing
/// one). Once connected the phone is listed like a USB one.
#[tauri::command]
pub async fn android_connect(state: State<'_, SharedState>, address: String) -> Result<(), String> {
    let adb_path = adb_path(&state)?;
    tokio::task::spawn_blocking(move || Adb::locate(&adb_path)?.connect(address.trim()))
        .await
        .map_err(|e| format!("Task error: {e}"))?
        .map_err(|e| e.to_string())
}

fn adb_path(state: &State<'_, SharedState>) -> Result<String, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.android.adb_path.clone())
}

/// "Descargar adb": Google's platform-tools, downloaded by this machine
/// straight from Google (`tatu_android::platform_tools` — Tatu itself never
/// ships them), then saved as the adb Tatu uses from now on.
#[tauri::command]
pub async fn android_download_adb(state: State<'_, SharedState>) -> Result<String, String> {
    let dir = android::platform_tools_dir();
    let adb = tokio::task::spawn_blocking(move || tatu_android::platform_tools::download(&dir))
        .await
        .map_err(|e| format!("Task error: {e}"))?
        .map_err(|e| e.to_string())?;
    let path = adb.display().to_string();
    let mut s = state.lock().map_err(|e| e.to_string())?;
    s.android.adb_path = path.clone();
    s.save();
    Ok(path)
}

/// Sends one game's whole folder to `<remote_root>/<folder name>` on the
/// phone (see `tatu_android::send_folder` for the staging/verify/rename
/// guarantees) and resolves to that final path. `spawn_blocking`d — it's
/// all adb child processes and file hashing. Emits `android_send_progress`
/// (`{phase: "pushing"|"verifying", current, total}`, bytes) as it goes:
/// every couple of seconds while copying, 5/s while verifying.
#[tauri::command]
pub async fn android_send_game(
    app: AppHandle,
    state: State<'_, SharedState>,
    cancel_state: State<'_, AndroidSendCancel>,
    source: String,
    id: u64,
    serial: String,
    remote_root: String,
) -> Result<String, String> {
    let remote_root = remote_root.trim().trim_end_matches('/').to_string();
    tatu_android::validate_remote_path(&remote_root).map_err(|e| e.to_string())?;
    let (dir, adb_path) = {
        let mut s = state.lock().map_err(|e| e.to_string())?;
        let dir = crate::installed::game_folder(&s, &source, id)?;
        // Remembered up front: even a send that fails or gets cancelled
        // still means "this is where my games go".
        s.android.remote_root = remote_root.clone();
        s.save();
        (dir, s.android.adb_path.clone())
    };

    let cancel = Arc::new(AtomicBool::new(false));
    *cancel_state.0.lock().map_err(|e| e.to_string())? = Some(cancel.clone());

    tokio::task::spawn_blocking(move || {
        let adb = Adb::locate(&adb_path)?;
        tatu_android::send_folder(&adb, &serial, &dir, &remote_root, &cancel, |progress| {
            let _ = app.emit("android_send_progress", progress);
        })
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?
    .map_err(|e| e.to_string())
}

/// Stops the in-flight send (adb is killed, the staging copy removed). A
/// no-op when nothing is sending — a stale click racing the send's own
/// completion isn't an error.
#[tauri::command]
pub fn android_cancel_send(state: State<'_, AndroidSendCancel>) {
    if let Ok(guard) = state.0.lock()
        && let Some(flag) = guard.as_ref()
    {
        flag.store(true, Ordering::Relaxed);
    }
}

/// The game's saved best configuration, if the user wrote one down.
#[tauri::command]
pub fn gamenative_get_config(
    state: State<'_, SharedState>,
    source: String,
    id: u64,
) -> Result<Option<android::GameNativeConfig>, String> {
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(s.android
        .configs
        .get(&android::config_key(&source, id))
        .cloned())
}

/// Saves (or, when everything in it is blank, deletes) the game's best
/// configuration. Only ever recorded — nothing is sent to GameNative.
#[tauri::command]
pub fn gamenative_save_config(
    state: State<'_, SharedState>,
    source: String,
    id: u64,
    config: android::GameNativeConfig,
) -> Result<(), String> {
    let key = android::config_key(&source, id);
    let mut s = state.lock().map_err(|e| e.to_string())?;
    if config.is_empty() {
        s.android.configs.remove(&key);
    } else {
        s.android.configs.insert(key, config);
    }
    s.save();
    Ok(())
}
