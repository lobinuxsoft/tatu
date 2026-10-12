// The tracker's side of the PC tab: which exe and folder ARE the game on
// this machine, and what to remember between visits. The upscaler work —
// detection, the OptiScaler release, the reversible patches with their
// backups — is the `tatu-upscalers` crate; the record of what a patch
// changed lives inside the game folder, not here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::installed::GameRef;
use crate::state::AppState;

/// Everything the PC tab keeps in `state.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PcState {
    /// `source:id` → folder (under the game root) the user picked for
    /// OptiScaler, when the detected one is wrong.
    #[serde(default)]
    pub target_dirs: HashMap<String, String>,
    /// [`launch_key`] → the launch options Tatu changed for OptiScaler, so
    /// removing it puts back exactly what the user had. Here and not in the
    /// game folder: launch options belong to this machine's Steam — and to
    /// one of its accounts, each with its own.
    #[serde(default)]
    pub launch_changes: HashMap<String, LaunchChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchChange {
    /// The Steam account (`userdata/<account>`) whose launch options these
    /// are. `0`: recorded before accounts were — the active one.
    #[serde(default)]
    pub account: u64,
    /// DLL name without `.dll`, as it went into `WINEDLLOVERRIDES`.
    pub dll: String,
    pub before: String,
    pub after: String,
}

pub fn key(source: &str, id: u64) -> String {
    format!("{source}:{id}")
}

#[cfg_attr(not(unix), allow(dead_code))]
pub fn launch_key(account: u64, app: &str) -> String {
    format!("{account}:{app}")
}

/// Every launch-option change Tatu recorded for Steam app `app`, on any
/// account, with its key: [`launch_key`], or — recorded before accounts
/// were — the app id alone.
pub fn launch_changes_for(pc: &PcState, app: &str) -> Vec<(String, LaunchChange)> {
    pc.launch_changes
        .iter()
        .filter(|(key, _)| {
            key.as_str() == app || key.rsplit_once(':').is_some_and(|(_, a)| a == app)
        })
        .map(|(key, change)| (key.clone(), change.clone()))
        .collect()
}

/// Where OptiScaler and friends download to — next to `state.json`.
pub fn cache_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("backlog-tracker")
        .join("upscalers")
}

/// What the PC tab needs from the state for one game, copied out under the
/// lock; everything else (folders, exes, launch options) is file-system work
/// done without it.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub game: GameRef,
    /// A shortcut's own target exe.
    pub shortcut_exe: Option<String>,
    pub target_override: Option<String>,
    pub launch_changes: Vec<LaunchChange>,
}

pub fn snapshot(state: &AppState, source: &str, id: u64) -> Result<Snapshot, String> {
    Ok(Snapshot {
        game: crate::installed::game_ref(state, source, id)?,
        shortcut_exe: (source == "non_steam")
            .then(|| {
                state
                    .non_steam
                    .iter()
                    .find(|g| g.id == id)
                    .map(|g| g.exe.clone())
            })
            .flatten(),
        target_override: state.pc.target_dirs.get(&key(source, id)).cloned(),
        launch_changes: if source == "steam" {
            launch_changes_for(&state.pc, &id.to_string())
                .into_iter()
                .map(|(_, change)| change)
                .collect()
        } else {
            Vec::new()
        },
    })
}

/// The game's main exe, relative to its root (`/`-separated): what Steam's
/// Play button runs, a shortcut's own target, the largest exe otherwise.
/// `None` for a native Linux build (no Windows exe at all).
pub fn exe_rel(snap: &Snapshot, root: &Path) -> Option<String> {
    let rel = match &snap.game {
        GameRef::Steam(id) => crate::steam::pick_main_exe_in(root, *id).ok()?,
        GameRef::NonSteam { .. } => {
            let exe = PathBuf::from(snap.shortcut_exe.as_deref()?.trim().trim_matches('"'));
            exe.strip_prefix(root).ok()?.to_string_lossy().into_owned()
        }
        _ => crate::steam::pick_main_exe_in(root, 0).ok()?,
    };
    Some(rel.replace('\\', "/"))
}

/// The folder OptiScaler goes into: the user's pick, else next to the exe
/// that really runs (Unreal's `Binaries/Win64`, see `tatu_upscalers::detect`).
pub fn target_rel(snap: &Snapshot, root: &Path, exe: Option<&str>) -> (String, bool) {
    match &snap.target_override {
        Some(dir) if root.join(dir).is_dir() => (dir.clone(), true),
        _ => (tatu_upscalers::detect::target_dir(root, exe), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_records_are_found_per_app_old_and_new() {
        // A record from before accounts were tracked (keyed by the app, no
        // `account`) must still load — a state.json that fails to parse
        // falls back to its backups — and still be found to undo.
        let pc: PcState = serde_json::from_str(
            r#"{"launch_changes": {
                "1091500": {"dll": "winmm", "before": "", "after": "WINEDLLOVERRIDES=winmm=n,b %command%"},
                "49007254:1091500": {"account": 49007254, "dll": "winmm", "before": "-dx12", "after": "x"},
                "49007254:10915": {"account": 49007254, "dll": "version", "before": "", "after": "y"}
            }}"#,
        )
        .unwrap();
        let mut found = launch_changes_for(&pc, "1091500");
        found.sort_by(|a, b| a.0.cmp(&b.0));
        let keys: Vec<(&str, u64)> = found.iter().map(|(k, c)| (k.as_str(), c.account)).collect();
        assert_eq!(keys, [("1091500", 0), ("49007254:1091500", 49007254)]);
        assert_eq!(launch_key(49007254, "1091500"), "49007254:1091500");
        assert!(
            launch_changes_for(&pc, "91500").is_empty(),
            "no suffix matches"
        );
    }
}
