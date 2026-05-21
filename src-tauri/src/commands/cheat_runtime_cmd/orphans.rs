//! Orphan-hook recovery: list / restore / dismiss persisted records left
//! behind by a tracker crash or a forced game exit.
//!
//! Phase 6: recovery dispatches on [`cheat_runtime::BackendKind`]. A
//! `Linux` record replays the undo log via the local ptrace runtime
//! (existing behaviour). A `Bridge` record dials
//! `<wineprefix>/drive_c/users/Public/tatu-bridge.sock` and asks the
//! bridge to restore every byte through `Request::PatchBytes` —
//! crucial when the trampoline was written from inside the
//! wineprefix and is only addressable through the bridge's
//! `OpenProcess` handle.

use std::collections::HashMap;
use std::path::Path;

use cheat_runtime::bridge_client::BridgeClient;
use cheat_runtime::{
    ActiveCheat, BackendKind, PersistedHook, load_all_persisted_hooks, load_manifests_for,
};
use serde::Serialize;
use tatu_proto::{Request, Response};
use tauri::State;

use super::ActiveCheats;

/// A persisted hook whose recorded PID is still alive — the user can
/// either restore the original bytes or dismiss the record (e.g. they
/// already re-launched the game and the bytes are gone anyway). Records
/// whose PID is dead are deleted silently and don't appear here.
#[derive(Debug, Serialize)]
pub struct OrphanHook {
    pub app_id: String,
    pub feature_uuid: String,
    pub exe: String,
    pub pid: i32,
    /// Best-effort feature display name resolved from the manifest for
    /// the same app_id + uuid. Empty when the manifest is gone (manifest
    /// was deleted while the hook was active).
    pub name: String,
    pub writes: usize,
    pub allocs: usize,
}

/// Scan the persist directory; delete records for dead PIDs; return the
/// rest tagged with their feature name. Called by the frontend at app
/// startup to surface a "restore orphan hooks" banner.
///
/// **Critical**: records whose `feature_uuid` is currently in the live
/// `ActiveCheats` registry are NOT orphans — they're the in-memory copy
/// of the same hook, persisted to disk for crash recovery. Including
/// them in the banner and letting the user click "Restore" would roll
/// back a hook the engine still considers active (#100).
#[tauri::command]
pub fn cheat_runtime_orphans_list(
    active: State<'_, ActiveCheats>,
) -> Result<Vec<OrphanHook>, String> {
    let live_uuids: std::collections::HashSet<String> = active
        .lock()
        .map_err(|e| format!("active registry poisoned: {e}"))?
        .keys()
        .cloned()
        .collect();
    let mut report = load_all_persisted_hooks().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let mut name_cache: HashMap<String, HashMap<String, String>> = HashMap::new();
    for record in report.records.drain(..) {
        if live_uuids.contains(&record.feature_uuid) {
            // Record matches a cheat that's still active in this session
            // — not an orphan, suppress.
            continue;
        }
        if !record.pid_alive() {
            // Game already exited — kernel reclaimed memory, nothing to
            // restore. Drop the file silently.
            if let Err(e) = record.delete() {
                eprintln!(
                    "[orphans] couldn't delete record for dead pid {}: {e}",
                    record.pid
                );
            }
            continue;
        }
        let name = name_cache
            .entry(record.app_id.clone())
            .or_insert_with(|| build_name_index(&record.app_id))
            .get(&record.feature_uuid)
            .cloned()
            .unwrap_or_default();
        out.push(OrphanHook {
            app_id: record.app_id.clone(),
            feature_uuid: record.feature_uuid.clone(),
            exe: record.exe.clone(),
            pid: record.pid,
            name,
            writes: record.writes.len(),
            allocs: record.allocs.len(),
        });
    }
    for (path, err) in report.failed {
        eprintln!("[orphans] skip malformed record {}: {err}", path.display());
    }
    Ok(out)
}

/// Restore an orphan hook: load the record, run the rollback via the
/// same atomic POKEDATA path a live disable would have used, delete the
/// on-disk file. Idempotent — calling twice is a no-op for the second
/// call (the file is gone).
#[tauri::command]
pub fn cheat_runtime_orphans_restore(app_id: String, feature_uuid: String) -> Result<(), String> {
    let report = load_all_persisted_hooks().map_err(|e| e.to_string())?;
    let record = report
        .records
        .into_iter()
        .find(|r| r.app_id == app_id && r.feature_uuid == feature_uuid)
        .ok_or_else(|| {
            format!("no persisted hook record for app {app_id} feature {feature_uuid}")
        })?;
    if !record.pid_alive() {
        // Game exited between list and restore — nothing to do, just
        // clean up.
        record.delete().map_err(|e| e.to_string())?;
        return Ok(());
    }
    eprintln!(
        "[orphans] restoring {} writes via {:?} for pid {} ({})",
        record.writes.len(),
        record.backend,
        record.pid,
        record.exe
    );
    match record.backend {
        BackendKind::Linux => {
            let cheat = ActiveCheat::from_persisted(&record);
            cheat
                .disable()
                .map_err(|e| format!("rollback failed: {e}"))?;
        }
        BackendKind::Bridge => {
            restore_via_bridge(&record)?;
        }
    }
    record.delete().map_err(|e| e.to_string())?;
    Ok(())
}

/// Rollback path for a `BackendKind::Bridge` record. Opens the bridge
/// inside the wineprefix the original enable wrote down, replays the
/// undo log in reverse via `Request::PatchBytes` (the same wire the
/// live enable would have used), and asks the bridge to free any
/// codecaves the original enable allocated. The bridge stays running
/// — recovery shouldn't kick the game off the bridge just because the
/// tracker crashed mid-session.
fn restore_via_bridge(record: &PersistedHook) -> Result<(), String> {
    let prefix = record
        .wineprefix
        .as_deref()
        .ok_or_else(|| "bridge record missing wineprefix path".to_string())?;
    let mut client = BridgeClient::connect(Path::new(prefix))
        .map_err(|e| format!("dial bridge at {prefix}: {e}"))?;

    // Reverse-order replay matches ActiveCheat::disable, which walks
    // undo entries from last-applied to first-applied — guarantees a
    // multi-page trampoline disassembles in the opposite order it was
    // written.
    for write in record.writes.iter().rev() {
        let resp = client
            .request(Request::PatchBytes {
                addr: write.addr,
                bytes: write.original.clone(),
                // We're rolling back; the game is presumably idle
                // because the user closed the tracker — don't pay
                // the SuspendThread enumeration cost.
                suspend_threads: false,
            })
            .map_err(|e| format!("bridge PatchBytes @ {:#x}: {e}", write.addr))?;
        match resp {
            Response::PatchBytes => {}
            Response::Err { message } => {
                return Err(format!("bridge rejected PatchBytes: {message}"));
            }
            other => return Err(format!("bridge unexpected response: {other:?}")),
        }
    }

    for alloc in &record.allocs {
        let resp = client
            .request(Request::RemoteFree { addr: alloc.addr })
            .map_err(|e| format!("bridge RemoteFree {}: {e}", alloc.symbol))?;
        match resp {
            Response::RemoteFreed => {}
            Response::Err { message } => {
                // A leaked codecave is annoying, not catastrophic —
                // log + keep going. The hook is already rolled back.
                eprintln!(
                    "[orphans] bridge couldn't free codecave {}@{:#x}: {message}",
                    alloc.symbol, alloc.addr
                );
            }
            other => return Err(format!("bridge unexpected response: {other:?}")),
        }
    }
    Ok(())
}

/// Drop an orphan record without rolling it back. The user picks this
/// when they've already re-launched the game (so the bytes are fresh
/// anyway) and just wants the banner to go away.
#[tauri::command]
pub fn cheat_runtime_orphans_dismiss(app_id: String, feature_uuid: String) -> Result<(), String> {
    let stub = PersistedHook {
        app_id,
        feature_uuid,
        pid: 0,
        exe: String::new(),
        backend: cheat_runtime::BackendKind::default(),
        wineprefix: None,
        started_at: None,
        writes: Vec::new(),
        allocs: Vec::new(),
    };
    stub.delete().map_err(|e| e.to_string())
}

/// Resolve uuid → feature.name for every feature in every manifest of
/// `app_id`. Used by [`cheat_runtime_orphans_list`] to render readable
/// names in the recovery banner.
fn build_name_index(app_id: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Ok(manifests) = load_manifests_for(app_id) {
        for m in manifests {
            for f in m.features {
                out.insert(f.uuid, f.name);
            }
        }
    }
    out
}
