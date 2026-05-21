//! Tauri commands + helpers for manifest prerequisites.
//!
//! Surfaces two read/install endpoints to the frontend so the cheats
//! panel can render the install banner + dim toggle rows when a
//! prerequisite is missing:
//!
//! - `prereqs_status_for_app` — collects every Prereq across every
//!   manifest for `app_id`, deduplicates by kind, and reports its
//!   current satisfaction state.
//! - `prereqs_install` — dispatches to the matching backend module
//!   (today only `reframework::install`).
//!
//! The toggle command (`cheat_runtime_enable`) consults
//! [`check_feature_prereqs`] before attaching so an out-of-band UI
//! state (or a script invocation) can't bypass the gate.

use cheat_runtime::{Manifest, Prereq, load_manifests_for};
use serde::{Deserialize, Serialize};

use crate::reframework;
use crate::steam::find_install_path;

/// Status of one prerequisite for one game. `kind` mirrors the
/// `Prereq` tag so the frontend dispatches the install handler by
/// the same string.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PrereqStatusView {
    Reframework {
        satisfied: bool,
        required_for_anticheat: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        dll_size_bytes: Option<u64>,
    },
}

#[tauri::command]
pub fn prereqs_status_for_app(app_id: String) -> Result<Vec<PrereqStatusView>, String> {
    let manifests = load_manifests_for(&app_id).map_err(|e| e.to_string())?;
    let unique = unique_prereqs(&manifests);
    if unique.is_empty() {
        return Ok(Vec::new());
    }
    let game_dir = find_install_path(&app_id).ok();
    Ok(unique
        .into_iter()
        .map(|p| view_for(&p, game_dir.as_deref()))
        .collect())
}

#[tauri::command]
pub fn prereqs_install(app_id: String, kind: String) -> Result<String, String> {
    let game_dir = find_install_path(&app_id)?;
    match kind.as_str() {
        "reframework" => reframework::install(&game_dir)
            .map(|report| report.version_tag)
            .map_err(|e| e.to_string()),
        other => Err(format!("unknown prereq kind: {other}")),
    }
}

/// Backend-side gate used by `cheat_runtime_enable`. Walks the
/// manifests for `app_id`, finds the one owning `feature_uuid`, and
/// errors with a human-readable message if any of its prereqs are
/// unsatisfied. No prereqs → trivially ok.
pub(crate) fn check_feature_prereqs(app_id: &str, feature_uuid: &str) -> Result<(), String> {
    let manifests = load_manifests_for(app_id).map_err(|e| e.to_string())?;
    let Some(manifest) = manifests
        .iter()
        .find(|m| m.features.iter().any(|f| f.uuid == feature_uuid))
    else {
        return Ok(()); // unknown feature — the caller will surface its own error
    };
    if manifest.prereqs.is_empty() {
        return Ok(());
    }
    let game_dir = find_install_path(app_id)?;
    for prereq in &manifest.prereqs {
        if !is_satisfied(prereq, &game_dir) {
            return Err(prereq_unsatisfied_message(prereq));
        }
    }
    Ok(())
}

fn unique_prereqs(manifests: &[Manifest]) -> Vec<Prereq> {
    let mut seen: Vec<Prereq> = Vec::new();
    for m in manifests {
        for p in &m.prereqs {
            if !seen.iter().any(|s| prereq_kind(s) == prereq_kind(p)) {
                seen.push(p.clone());
            }
        }
    }
    seen
}

fn prereq_kind(p: &Prereq) -> &'static str {
    match p {
        Prereq::Reframework { .. } => "reframework",
    }
}

fn view_for(p: &Prereq, game_dir: Option<&std::path::Path>) -> PrereqStatusView {
    match p {
        Prereq::Reframework {
            required_for_anticheat,
        } => {
            let (satisfied, dll_size_bytes) = match game_dir {
                Some(dir) => match reframework::status(dir) {
                    reframework::ReframeworkStatus::Installed { dll_size_bytes, .. } => {
                        (true, Some(dll_size_bytes))
                    }
                    reframework::ReframeworkStatus::NotInstalled => (false, None),
                },
                None => (false, None),
            };
            PrereqStatusView::Reframework {
                satisfied,
                required_for_anticheat: *required_for_anticheat,
                dll_size_bytes,
            }
        }
    }
}

fn is_satisfied(p: &Prereq, game_dir: &std::path::Path) -> bool {
    match p {
        Prereq::Reframework { .. } => matches!(
            reframework::status(game_dir),
            reframework::ReframeworkStatus::Installed { .. }
        ),
    }
}

fn prereq_unsatisfied_message(p: &Prereq) -> String {
    match p {
        Prereq::Reframework { .. } => "REFramework is required for this game but is not \
             installed. Click 'Install REFramework' in the cheats panel banner first."
            .to_string(),
    }
}
