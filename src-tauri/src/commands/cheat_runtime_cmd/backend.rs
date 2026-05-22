//! Per-game cheat bridge attachment — Phase 6 wire of epic #106.
//!
//! Three Tauri commands manage the [`crate::state::BridgeEntry`] map:
//! `get` returns whether Tatu is enabled for an appid (and the
//! attached wineprefix); `set` flips the toggle on with an explicit
//! wineprefix; `clear` flips it off. The legacy Linux backend was
//! dropped in favour of a single bridge path — see README.

use std::sync::Mutex;

use tauri::State;

use crate::state::{AppState, BridgeEntry};
use crate::steam::resolve_wineprefix;
use crate::tatu_launcher::{TatuLauncherStatus, status as tatu_launcher_status};

/// Return the bridge attachment for `app_id` if Tatu is enabled, or
/// `None` if the game is not configured. The frontend uses this to
/// render the banner state (Enabled / Disabled).
#[tauri::command]
pub fn cheat_runtime_backend_get(
    app_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Option<BridgeEntry>, String> {
    let guard = state.lock().map_err(|e| format!("state poisoned: {e}"))?;
    Ok(guard.cheat_backend.get(&app_id).cloned())
}

/// Attach the bridge to `app_id` with the supplied wineprefix.
/// Idempotent — repeated calls overwrite the prefix if it changed.
#[tauri::command]
pub fn cheat_runtime_backend_set(
    app_id: String,
    backend: BridgeEntry,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let snapshot = {
        let mut guard = state.lock().map_err(|e| format!("state poisoned: {e}"))?;
        guard.cheat_backend.insert(app_id, backend);
        guard.clone()
    };
    snapshot.save();
    Ok(())
}

/// Detach the bridge from `app_id`. The launcher.toml entry stays so
/// the user's per-game `proton` / `target_exe` overrides survive a
/// disable / re-enable cycle (the Disable handler in the frontend
/// flips `tatu_enabled` to false without removing the block).
#[tauri::command]
pub fn cheat_runtime_backend_clear(
    app_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let snapshot = {
        let mut guard = state.lock().map_err(|e| format!("state poisoned: {e}"))?;
        guard.cheat_backend.remove(&app_id);
        guard.clone()
    };
    snapshot.save();
    Ok(())
}

/// Read-side helper used by value-cheats / recovery routers in this
/// module tree. `Some(entry)` means Tatu is enabled for the appid;
/// `None` means the toggle is off and the caller should refuse with
/// a clear message instead of falling back silently.
pub(super) fn resolve_backend(
    state: &State<'_, Mutex<AppState>>,
    app_id: &str,
) -> Option<BridgeEntry> {
    state
        .lock()
        .ok()
        .and_then(|s| s.cheat_backend.get(app_id).cloned())
}

/// Suggest the wineprefix the bridge should attach to for `app_id`.
/// Returns `Err` when Tatu can't service the game (drop-in not
/// installed or wineprefix missing) so the frontend can surface a
/// precise reason instead of silently failing.
#[tauri::command]
pub fn cheat_runtime_backend_recommend(app_id: String) -> Result<BridgeEntry, String> {
    let prefix = resolve_wineprefix(&app_id).ok_or_else(|| {
        format!("no Wine prefix found for appid {app_id} — launch the game once under any Proton first")
    })?;
    if !matches!(tatu_launcher_status(), TatuLauncherStatus::Installed { .. }) {
        return Err("Tatu Launcher drop-in not installed — click Install in the cheats panel banner first".to_string());
    }
    Ok(BridgeEntry {
        wineprefix: prefix.to_string_lossy().into_owned(),
    })
}
