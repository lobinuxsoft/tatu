//! `cheat_runtime_prereqs_check` / `cheat_runtime_prereqs_install` — UI
//! bridge for #98. The frontend calls `_check` after rendering a feature
//! list to decide whether to draw the install banner; `_install` runs
//! the downloader on the user's click.
//!
//! Detection lives in `cheat_runtime::prereqs` (fs-only); the installer
//! lives in `tatu_tracker::prereqs` (ureq + zip). This file is the thin
//! seam that resolves the Steam game dir per appid and hands the work
//! to the right layer.

use cheat_runtime::{Prereq, PrereqStatus, detect_reframework, load_manifests_for};
use serde::Serialize;

use crate::prereqs::{install_mono_collector, install_reframework};
use crate::steam::exe::find_install_path;
use crate::steam::{LaunchOptOutcome, detect_game_exe, set_winhttp_override};

#[derive(Debug, Serialize)]
pub struct PrereqReport {
    /// Which prereq this row describes. Matches the
    /// `kind` discriminant the manifest emitted (`reframework`).
    pub kind: String,
    /// Current detection result for that prereq.
    pub status: PrereqStatus,
    /// Resolved game directory we ran the check against — surfaced so
    /// the UI can render an "open folder" link.
    pub game_dir: String,
}

/// Report the install status of every distinct prereq declared by any
/// manifest under `app_id`. Returns an empty vec when no manifest needs
/// a prereq — the UI then hides the banner entirely.
#[tauri::command]
pub fn cheat_runtime_prereqs_check(app_id: String) -> Result<Vec<PrereqReport>, String> {
    let exe_hint = detect_game_exe(&app_id).ok();
    let manifests = load_manifests_for(&app_id, exe_hint.as_deref()).map_err(|e| e.to_string())?;

    // Collect the distinct prereqs across every manifest for this game
    // — same prereq declared by two CT tables of the same game should
    // surface once in the UI, not twice.
    let mut seen = std::collections::HashSet::new();
    let mut needed: Vec<Prereq> = Vec::new();
    for m in &manifests {
        for p in &m.prereqs {
            if seen.insert(p.clone()) {
                needed.push(p.clone());
            }
        }
    }
    if needed.is_empty() {
        return Ok(Vec::new());
    }

    let game_dir = find_install_path(&app_id).map_err(|e| e.to_string())?;
    let game_dir_str = game_dir.display().to_string();

    let mut out = Vec::with_capacity(needed.len());
    for prereq in needed {
        let (kind, status) = match prereq {
            Prereq::Reframework => ("reframework", detect_reframework(&game_dir)),
        };
        out.push(PrereqReport {
            kind: kind.to_string(),
            status,
            game_dir: game_dir_str.clone(),
        });
    }
    Ok(out)
}

#[derive(Debug, Serialize)]
pub struct InstallResult {
    pub kind: String,
    pub dll_path: String,
    pub size_bytes: u64,
    pub release_tag: String,
    pub backed_up: Option<String>,
}

/// Install a specific prereq for `app_id`. `kind` matches the discriminant
/// surfaced by [`cheat_runtime_prereqs_check`] (e.g. `"reframework"`).
/// Synchronous — the frontend already shows a spinner; running this on
/// a worker thread inside Tauri would add complexity for no win since the
/// download is short (6-12 MB ≈ seconds) and Tauri commands already run
/// off the UI thread.
#[tauri::command]
pub fn cheat_runtime_prereqs_install(
    app_id: String,
    kind: String,
) -> Result<InstallResult, String> {
    let game_dir = find_install_path(&app_id).map_err(|e| e.to_string())?;
    match kind.as_str() {
        "reframework" => {
            let info = install_reframework(&game_dir).map_err(|e| e.to_string())?;
            Ok(InstallResult {
                kind,
                dll_path: info.dll_path,
                size_bytes: info.size_bytes,
                release_tag: info.release_tag,
                backed_up: info.backed_up,
            })
        }
        other => Err(format!("unknown prereq kind {other:?}")),
    }
}

/// Drop the Mono collector (`winhttp.dll`) next to `app_id`'s game exe.
///
/// Unlike [`cheat_runtime_prereqs_install`], this isn't a manifest-declared
/// prereq — it's a runtime injection vector gated on the game being Unity
/// Mono (the installer checks `detect_unity_backend` itself). The bytes come
/// from the artifact compiled into tatu, so there's no download. The caller
/// still owns the companion launch-option step
/// (`WINEDLLOVERRIDES=winhttp=n,b`), which is smoke-time and user-driven.
#[tauri::command]
pub fn cheat_runtime_install_mono_collector(app_id: String) -> Result<InstallResult, String> {
    let game_dir = find_install_path(&app_id).map_err(|e| e.to_string())?;
    let info = install_mono_collector(&game_dir).map_err(|e| e.to_string())?;
    Ok(InstallResult {
        kind: "mono-collector".to_string(),
        dll_path: info.dll_path,
        size_bytes: info.size_bytes,
        release_tag: info.release_tag,
        backed_up: info.backed_up,
    })
}

/// Apply `WINEDLLOVERRIDES=winhttp=n,b` to `app_id`'s Steam launch options so
/// Proton loads the Mono collector. Companion to
/// [`cheat_runtime_install_mono_collector`]: the DLL placement and this launch
/// option together make up the collector's auto-install vector. Fails if Steam
/// is running (it would overwrite the edit on exit).
#[tauri::command]
pub fn cheat_runtime_set_winhttp_override(app_id: String) -> Result<LaunchOptOutcome, String> {
    set_winhttp_override(&app_id).map_err(|e| e.to_string())
}
