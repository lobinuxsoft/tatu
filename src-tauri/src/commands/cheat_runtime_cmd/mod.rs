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

pub mod backend;
pub mod features;
pub mod orphans;
pub mod toggles;
pub mod values;

use std::collections::HashMap;
use std::sync::Mutex;

use cheat_runtime::{ActiveCheat, Pid};
use tatu_proto::WireOutcome;

/// One enabled cheat as seen by the registry. The variant decides
/// which backend will service the eventual disable: a Linux record
/// keeps the live [`ActiveCheat`] so its in-memory undo log is the
/// source of truth; a Bridge record keeps the [`WireOutcome`] the
/// bridge returned (the bridge holds no per-cheat state) plus the
/// wineprefix it lives in.
pub enum ActiveCheatEntry {
    Linux(ActiveCheat),
    Bridge {
        wineprefix: String,
        outcome: WireOutcome,
        symbols: HashMap<String, u64>,
    },
}

impl ActiveCheatEntry {
    pub fn symbols(&self) -> HashMap<String, u64> {
        match self {
            ActiveCheatEntry::Linux(c) => c.symbols().clone(),
            ActiveCheatEntry::Bridge { symbols, .. } => symbols.clone(),
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

/// Drop every active cheat whose PID is gone. Their `Drop` impl will try
/// to roll back writes against the dead process; those calls fail silently
/// (ESRCH) so the only effect is freeing the in-memory registry slot.
/// Bridge entries are skipped — the bridge holds the live state, not
/// the tracker, and recovery on the bridge side uses the wineprefix
/// lifetime, not a Linux PID liveness check.
pub(super) fn purge_stale_cheats(active: &ActiveCheats) -> Result<(), String> {
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
    for entry in guard.values() {
        for (k, v) in entry.symbols() {
            merged.insert(k, v);
        }
    }
    Ok(merged)
}
