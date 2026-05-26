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
pub mod import;
pub mod orphans;
pub mod toggles;
pub mod values;

use std::collections::HashMap;
use std::sync::Mutex;

use cheat_runtime::{ActiveCheat, FreezeRegistry, Pid};

/// One enabled cheat as seen by the registry. Linux-only post-pivot
/// #128 — the wine-side Bridge variant was dropped along with the
/// bridge architecture. The variant is preserved (vs collapsing to a
/// transparent newtype) so future backends (Phase E in-process
/// extension, #135) can re-add their own arm without breaking call
/// sites that match on it.
pub enum ActiveCheatEntry {
    Linux(ActiveCheat),
}

impl ActiveCheatEntry {
    pub fn symbols(&self) -> HashMap<String, u64> {
        match self {
            ActiveCheatEntry::Linux(c) => c.symbols().clone(),
        }
    }
}

/// Tauri-managed registry of currently enabled cheats, keyed by feature UUID.
pub type ActiveCheats = Mutex<HashMap<String, ActiveCheatEntry>>;

/// True if `/proc/<pid>/` still exists — the lightest possible liveness
/// check. Used to detect when the user closed and re-launched the game
/// out-of-band; without this the enable shortcut returns the registry
/// entry from the dead PID and the new game never gets hooked.
pub(super) fn pid_is_alive(pid: Pid) -> bool {
    std::path::Path::new(&format!("/proc/{}", pid.as_raw())).exists()
}

/// Drop every active cheat whose backing process is gone. Linux entries
/// are checked via `/proc/<pid>/`; Bridge entries are checked by trying
/// a short-timeout TCP connect to the wineprefix's port file — the
/// bridge `--connect` process is killed by its `--launch` parent when
/// the game exits, so a refused connection means the cheat is orphaned.
///
/// When a Bridge entry is purged, any freeze loops keyed on its
/// `feature_uuid` are also cancelled so the UI / disk state lines up
/// with the dead process. Freeze workers self-abort after ~1 s of
/// consecutive errors anyway, but the explicit stop avoids the brief
/// window where stale frozen-value indicators flicker on the frontend.
pub(super) fn purge_stale_cheats(
    active: &ActiveCheats,
    freezes: Option<&FreezeRegistry>,
) -> Result<(), String> {
    let mut guard = active
        .lock()
        .map_err(|e| format!("active registry poisoned: {e}"))?;
    let stale: Vec<String> = guard
        .iter()
        .filter_map(|(uuid, entry)| match entry {
            ActiveCheatEntry::Linux(c) if !pid_is_alive(c.pid()) => Some(uuid.clone()),
            _ => None,
        })
        .collect();
    for uuid in &stale {
        guard.remove(uuid);
        if let Some(reg) = freezes {
            // Freeze keys carry shape `value:<app_id>:<feature_uuid>` —
            // we don't track app_id here, so suffix-match any key
            // ending with this uuid and cancel it.
            if let Ok(keys) = reg.active_keys() {
                for key in keys {
                    if key.ends_with(uuid) {
                        let _ = reg.stop(&key);
                    }
                }
            }
        }
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
    for entry in guard.values() {
        for (k, v) in entry.symbols() {
            merged.insert(k, v);
        }
    }
    Ok(merged)
}
