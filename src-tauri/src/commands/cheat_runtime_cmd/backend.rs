//! Per-game cheat backend selection — Phase 6 wire of epic #106.
//!
//! Two Tauri commands expose the [`crate::state::GameBackend`] map to
//! the frontend so the user (or a future auto-detect heuristic) can
//! pick whether a given title's hooks should run through the Linux
//! ptrace runtime or `tatu-bridge --connect` inside its wineprefix.
//!
//! The selection is persisted in `AppState::cheat_backend` and so
//! survives across tracker restarts.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::{AppState, GameBackend};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum BackendChoice {
    /// Default native Linux ptrace runtime.
    Linux,
    /// `tatu-bridge --connect` under Wine. `wineprefix` is the prefix
    /// root (`$STEAM_COMPAT_DATA_PATH/pfx`) the bridge dialogue needs
    /// to compute the AF_UNIX socket path.
    Bridge { wineprefix: String },
}

impl From<&GameBackend> for BackendChoice {
    fn from(b: &GameBackend) -> Self {
        match b {
            GameBackend::Linux => BackendChoice::Linux,
            GameBackend::Bridge { wineprefix } => BackendChoice::Bridge {
                wineprefix: wineprefix.clone(),
            },
        }
    }
}

impl From<BackendChoice> for GameBackend {
    fn from(c: BackendChoice) -> Self {
        match c {
            BackendChoice::Linux => GameBackend::Linux,
            BackendChoice::Bridge { wineprefix } => GameBackend::Bridge { wineprefix },
        }
    }
}

/// Return the backend currently selected for `app_id`. Missing entries
/// surface as `Linux` — the default — so the frontend doesn't have to
/// distinguish "never configured" from "explicitly set to Linux".
#[tauri::command]
pub fn cheat_runtime_backend_get(
    app_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<BackendChoice, String> {
    let guard = state.lock().map_err(|e| format!("state poisoned: {e}"))?;
    Ok(guard
        .cheat_backend
        .get(&app_id)
        .map(BackendChoice::from)
        .unwrap_or(BackendChoice::Linux))
}

/// Persist the selection for `app_id`. Writes through to disk so the
/// tracker remembers the choice across restarts.
#[tauri::command]
pub fn cheat_runtime_backend_set(
    app_id: String,
    backend: BackendChoice,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let backend: GameBackend = backend.into();
    let snapshot = {
        let mut guard = state.lock().map_err(|e| format!("state poisoned: {e}"))?;
        // Linux is the default — drop the entry instead of bloating
        // state.json with redundant explicit-default rows. Bridge
        // entries always persist because they carry a wineprefix.
        match backend {
            GameBackend::Linux => {
                guard.cheat_backend.remove(&app_id);
            }
            other @ GameBackend::Bridge { .. } => {
                guard.cheat_backend.insert(app_id, other);
            }
        }
        guard.clone()
    };
    snapshot.save();
    Ok(())
}

/// Read-side helper used by the value-cheats / recovery routers in
/// this same module tree. Returns a clone so callers can release the
/// state lock before dialling the bridge / spawning ptrace work.
#[allow(dead_code)] // wired by commits 3 (value cheats) and 4 (recovery)
pub(super) fn resolve_backend(state: &State<'_, Mutex<AppState>>, app_id: &str) -> GameBackend {
    state
        .lock()
        .ok()
        .and_then(|s| s.cheat_backend.get(app_id).cloned())
        .unwrap_or(GameBackend::Linux)
}
