//! Tauri commands that expose the `cheat-runtime` crate to the frontend.
//!
//! The runtime operates on per-game **manifests** living under
//! `$XDG_CONFIG_HOME/backlog-tracker/trainers/<app_id>/`. Each manifest is a
//! self-describing JSON binding each user-facing feature to the CE Auto-
//! Assembler script that implements it (see `cheat_runtime::manifest`).
//!
//! Aurora's raw JSON exports are **not** consumed here yet — the feature ↔
//! script binding is still an open reverse-engineering problem (documented
//! in personal memory). Once solved, an Aurora → manifest converter lands
//! and these commands light up for the captured trainers.
//!
//! ## Submodule layout
//!
//! `#[tauri::command]` items are referenced from `tauri::generate_handler!`
//! by *path*, and the macro expands to sibling items inside the module
//! that owns the `pub fn` (e.g. `__cmd__name`). `pub use` re-exports don't
//! cover those siblings, so each submodule is left `pub` and the binary's
//! `generate_handler!` call uses the full submodule path.
//!
//! - [`features`] — `cheat_runtime_list_features` + `FeatureView`.
//! - [`toggles`] — `cheat_runtime_enable` / `cheat_runtime_disable`.
//! - [`orphans`] — recovery banner: list / restore / dismiss persisted hooks
//!   left behind by a tracker crash or a forced game exit.
//! - [`values`] — typed read/write/freeze over pointer-chains.

pub mod features;
pub mod orphans;
pub mod toggles;
pub mod values;

use std::collections::HashMap;
use std::sync::Mutex;

use cheat_runtime::{ActiveCheat, Pid};

/// Tauri-managed registry of currently enabled cheats, keyed by feature UUID.
pub type ActiveCheats = Mutex<HashMap<String, ActiveCheat>>;

/// True if `/proc/<pid>/` still exists — the lightest possible liveness
/// check. Used to detect when the user closed and re-launched the game
/// out-of-band; without this the enable shortcut returns the registry
/// entry from the dead PID and the new game never gets hooked.
pub(super) fn pid_is_alive(pid: Pid) -> bool {
    std::path::Path::new(&format!("/proc/{}", pid.as_raw())).exists()
}

/// Drop every active cheat whose PID is gone. Their `Drop` impl will try
/// to roll back writes against the dead process; those calls fail silently
/// (ESRCH) so the only effect is freeing the in-memory registry slot.
pub(super) fn purge_stale_cheats(active: &ActiveCheats) -> Result<(), String> {
    let mut guard = active
        .lock()
        .map_err(|e| format!("active registry poisoned: {e}"))?;
    let stale: Vec<String> = guard
        .iter()
        .filter(|(_, c)| !pid_is_alive(c.pid()))
        .map(|(k, _)| k.clone())
        .collect();
    for uuid in stale {
        guard.remove(&uuid);
    }
    Ok(())
}

/// Merge the symbol tables of every currently-active cheat into one map.
/// Same-name collisions are resolved last-write-wins — in practice they
/// don't happen because each AA toggle owns its alloc/scan symbols.
pub(super) fn merged_symbols(active: &ActiveCheats) -> Result<HashMap<String, u64>, String> {
    let guard = active
        .lock()
        .map_err(|e| format!("active registry poisoned: {e}"))?;
    let mut merged = HashMap::new();
    for cheat in guard.values() {
        for (k, v) in cheat.symbols() {
            merged.insert(k.clone(), *v);
        }
    }
    Ok(merged)
}
