//! A journaled, reversible change to a game folder.
//!
//! Everything lives under `<game root>/.tatu-patches/<id>/`: `manifest.json`
//! and `backup/`, which holds every file the patch replaced under its path
//! relative to the game root. Inside the game folder on purpose: the record
//! of what to undo can't drift from the files it describes, and it survives
//! Tatu's own config being lost or the game moving to another library.
//!
//! Apply writes each file into the manifest *before* touching it, so a crash
//! at any point leaves a manifest revert can finish from — and any error
//! mid-apply reverts on the spot. A backup, once made, is never overwritten
//! (the cartridge's Goldberg swap learned that the hard way).
//!
//! Revert decides per file from content hashes, never from "what should be
//! there":
//! - still what the patch placed, or gone: put the backup back, or delete it
//!   if the patch added it;
//! - already the original again (a "verify files" restored it): just drop the
//!   backup;
//! - anything else (the game updated it since): leave the file AND its backup
//!   alone and report it — the patch stays recorded until that's sorted out.
//!
//! Names are matched case-insensitively against what's on disk on every OS:
//! a game shipping `Libxess.dll` must be backed up, not joined by a second
//! `libxess.dll` that Wine could load instead.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{UpscalerError, io_err};

pub const PATCHES_DIR: &str = ".tatu-patches";
const MANIFEST: &str = "manifest.json";
const BACKUP: &str = "backup";
const TMP_SUFFIX: &str = ".tatu-tmp";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchState {
    Applying,
    Applied,
    /// A revert left files it couldn't decide on; see [`RevertReport::conflicts`].
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// Path under the game root, `/`-separated, with the case it has on disk.
    pub rel: String,
    pub placed_sha256: String,
    /// The game's file this one replaced, now at `backup/<rel>`.
    pub backup_sha256: Option<String>,
    /// Ours whatever its content later: the program rewrites it itself
    /// (OptiScaler.ini, saved from OptiScaler's own menu).
    #[serde(default)]
    pub owned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub state: PatchState,
    pub entries: Vec<Entry>,
    /// What the program writes while the game runs (logs, override folders),
    /// removed on revert. Only what didn't already exist at apply time.
    #[serde(default)]
    pub runtime: Vec<String>,
    /// Folders the patch created, removed on revert once empty.
    #[serde(default)]
    pub created_dirs: Vec<String>,
    /// The caller's own notes: what was installed and how.
    #[serde(default)]
    pub meta: serde_json::Value,
}

impl Manifest {
    /// The game's files this patch replaced — what its backup holds.
    pub fn backed_up(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| e.backup_sha256.is_some())
            .map(|e| e.rel.clone())
            .collect()
    }
}

pub struct Source {
    pub from: PathBuf,
    /// Destination under the game root, `/`-separated.
    pub rel: String,
    pub owned: bool,
}

pub struct Plan {
    pub id: String,
    pub sources: Vec<Source>,
    pub runtime: Vec<String>,
    pub meta: serde_json::Value,
    /// Refused while any of these exes (lowercase file names) runs: Windows
    /// won't let a revert replace a DLL the game has loaded, so neither side
    /// should start then. See [`game_exes`].
    pub running: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct RevertReport {
    /// The game's own file is back.
    pub restored: Vec<String>,
    /// Files the patch added, gone.
    pub removed: Vec<String>,
    /// Added by the patch, but changed afterwards by something else: left in place.
    pub kept: Vec<String>,
    /// Replaced by the patch and changed again afterwards: file and backup
    /// both left in place, the patch stays recorded.
    pub conflicts: Vec<String>,
    pub errors: Vec<String>,
}

impl RevertReport {
    pub fn complete(&self) -> bool {
        self.conflicts.is_empty() && self.errors.is_empty()
    }
}

struct Step {
    from: PathBuf,
    from_sha: String,
    dest: PathBuf,
    rel: String,
    existing: bool,
    owned: bool,
}

pub fn apply(root: &Path, plan: &Plan) -> Result<Manifest, UpscalerError> {
    validate_id(&plan.id)?;
    let dir = patch_dir(root, &plan.id);
    if dir.join(MANIFEST).exists() {
        return Err(UpscalerError::AlreadyApplied(plan.id.clone()));
    }
    if is_running(&plan.running) {
        return Err(UpscalerError::GameRunning);
    }

    let taken = files_of_other_patches(root, &plan.id);
    let mut steps = Vec::new();
    let mut bytes = 0u64;
    for source in &plan.sources {
        validate_rel(&source.rel)?;
        let (dest, rel) = resolve_ci(root, &source.rel);
        if let Some(other) = taken.get(&rel.to_lowercase()) {
            return Err(UpscalerError::Overlap(rel, other.clone()));
        }
        // A link would move into the backup as a link, and a relative one
        // would point nowhere from there: never touch them.
        if fs::symlink_metadata(&dest).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(UpscalerError::Invalid(format!(
                "{rel} es un enlace simbólico en el juego: no lo modifico"
            )));
        }
        if dest.is_dir() {
            return Err(UpscalerError::Invalid(format!(
                "{rel} es una carpeta en el juego"
            )));
        }
        let existing = dest.is_file();
        let from_sha = sha256_file(&source.from)?;
        // Already byte-identical: nothing to change, nothing to back up —
        // unless it's the patch's own file (its config), which Tatu may edit
        // afterwards: recorded all the same, so revert gives back exactly the
        // copy that was there.
        if existing && !source.owned && sha256_file(&dest)? == from_sha {
            continue;
        }
        // A backup nobody accounts for (its manifest is gone) may be the only
        // copy of a game file: caught here, before anything changes.
        let orphan = dir.join(BACKUP).join(&rel);
        if existing && orphan.exists() {
            return Err(UpscalerError::OrphanBackup(orphan.display().to_string()));
        }
        bytes += fs::metadata(&source.from)
            .map_err(|e| io_err(&source.from, e))?
            .len();
        steps.push(Step {
            from: source.from.clone(),
            from_sha,
            dest,
            rel,
            existing,
            owned: source.owned,
        });
    }
    preflight(root, &steps, bytes)?;

    let mut created_dirs: Vec<String> = Vec::new();
    for step in &steps {
        for missing in missing_dirs(root, &step.rel) {
            if !created_dirs.contains(&missing) {
                created_dirs.push(missing);
            }
        }
    }
    let runtime = plan
        .runtime
        .iter()
        .filter(|r| !resolve_ci(root, r).0.exists())
        .cloned()
        .collect();

    let mut manifest = Manifest {
        id: plan.id.clone(),
        state: PatchState::Applying,
        entries: Vec::new(),
        runtime,
        created_dirs,
        meta: plan.meta.clone(),
    };
    fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
    claim_manifest(&dir, &manifest)?;

    for step in &steps {
        if let Err(e) = place(&dir, step, &mut manifest) {
            // Whatever got done so far is in the manifest: undo it now.
            let _ = revert(root, &plan.id);
            return Err(e);
        }
    }
    manifest.state = PatchState::Applied;
    write_manifest(&dir, &manifest)?;
    Ok(manifest)
}

/// Journal first, then copy beside the destination, then two renames: the
/// game's file into the backup, ours into its place. Every intermediate state
/// — a crash included — is one revert knows how to undo, and the copy beside
/// the destination is always covered by a journaled entry.
fn place(dir: &Path, step: &Step, manifest: &mut Manifest) -> Result<(), UpscalerError> {
    let backup_sha = if step.existing {
        Some(sha256_file(&step.dest)?)
    } else {
        None
    };
    manifest.entries.push(Entry {
        rel: step.rel.clone(),
        placed_sha256: step.from_sha.clone(),
        backup_sha256: backup_sha,
        owned: step.owned,
    });
    write_manifest(dir, manifest)?;

    if let Some(parent) = step.dest.parent() {
        fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
    }
    let tmp = tmp_path(&step.dest);
    fs::copy(&step.from, &tmp).map_err(|e| io_err(&tmp, e))?;
    clear_readonly(&tmp);
    if sha256_file(&tmp)? != step.from_sha {
        return Err(UpscalerError::Invalid(format!(
            "La copia de {} salió distinta del original",
            step.rel
        )));
    }

    if step.existing {
        let backup = dir.join(BACKUP).join(&step.rel);
        // Checked before anything changed too; this covers a race with
        // something else writing into the backup.
        if backup.exists() {
            return Err(UpscalerError::BackupExists(step.rel.clone()));
        }
        if let Some(parent) = backup.parent() {
            fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
        }
        fs::rename(&step.dest, &backup).map_err(|e| io_err(&step.dest, e))?;
    }
    fs::rename(&tmp, &step.dest).map_err(|e| io_err(&step.dest, e))
}

/// Undo patch `id`. Keeps going past files it can't handle and records them,
/// so it can simply be run again once the game is closed or the conflict is
/// sorted out.
pub fn revert(root: &Path, id: &str) -> Result<RevertReport, UpscalerError> {
    validate_id(id)?;
    let dir = patch_dir(root, id);
    let mut manifest = load(root, id)?.ok_or_else(|| UpscalerError::NotApplied(id.to_string()))?;
    let mut report = RevertReport::default();

    let mut remaining = Vec::new();
    for entry in manifest.entries.iter().rev() {
        match revert_entry(root, &dir, entry, &mut report) {
            Ok(true) => {}
            Ok(false) => remaining.push(entry.clone()),
            Err(e) => {
                report.errors.push(e.to_string());
                remaining.push(entry.clone());
            }
        }
    }
    remaining.reverse();

    let mut remaining_runtime = Vec::new();
    for rel in &manifest.runtime {
        let (path, _) = resolve_ci(root, rel);
        let removed = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else if path.exists() {
            fs::remove_file(&path)
        } else {
            Ok(())
        };
        if let Err(e) = removed {
            report.errors.push(io_err(&path, e).to_string());
            remaining_runtime.push(rel.clone());
        }
    }
    for rel in manifest.created_dirs.iter().rev() {
        let _ = fs::remove_dir(resolve_ci(root, rel).0);
    }

    if remaining.is_empty() && remaining_runtime.is_empty() && !has_files(&dir.join(BACKUP)) {
        fs::remove_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
        let _ = fs::remove_dir(root.join(PATCHES_DIR));
    } else {
        manifest.entries = remaining;
        manifest.runtime = remaining_runtime;
        manifest.state = PatchState::Conflict;
        write_manifest(&dir, &manifest)?;
    }
    Ok(report)
}

/// `Ok(true)` when the entry is settled, `Ok(false)` when it has to stay
/// recorded (a conflict).
fn revert_entry(
    root: &Path,
    dir: &Path,
    entry: &Entry,
    report: &mut RevertReport,
) -> Result<bool, UpscalerError> {
    let (dest, _) = resolve_ci(root, &entry.rel);
    let _ = fs::remove_file(tmp_path(&dest));
    let current = if dest.is_file() {
        Some(sha256_file(&dest)?)
    } else {
        None
    };
    let ours = match &current {
        None => true,
        Some(sha) => entry.owned || *sha == entry.placed_sha256,
    };
    let backup = dir.join(BACKUP).join(&entry.rel);

    let Some(original) = &entry.backup_sha256 else {
        if !ours {
            report.kept.push(entry.rel.clone());
        } else if current.is_some() {
            fs::remove_file(&dest).map_err(|e| io_err(&dest, e))?;
            report.removed.push(entry.rel.clone());
        }
        return Ok(true);
    };

    if current.as_ref() == Some(original) {
        if backup.exists() {
            fs::remove_file(&backup).map_err(|e| io_err(&backup, e))?;
        }
        report.restored.push(entry.rel.clone());
        return Ok(true);
    }
    // Only ever put back a backup that is still the original, byte for byte.
    if ours && backup.is_file() && sha256_file(&backup)? == *original {
        // A game update may have removed the folder it lived in.
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| io_err(parent, e))?;
        }
        fs::rename(&backup, &dest).map_err(|e| io_err(&dest, e))?;
        report.restored.push(entry.rel.clone());
        return Ok(true);
    }
    report.conflicts.push(entry.rel.clone());
    Ok(false)
}

/// For a patch left in conflict because the game changed the files after it
/// (an update): keep the game's current files and drop the backups, closing
/// the record. Refused while any file could still be restored — that's what
/// revert is for.
pub fn keep_current(root: &Path, id: &str) -> Result<(), UpscalerError> {
    let manifest = load(root, id)?.ok_or_else(|| UpscalerError::NotApplied(id.to_string()))?;
    for entry in &manifest.entries {
        let (dest, _) = resolve_ci(root, &entry.rel);
        let current = if dest.is_file() {
            Some(sha256_file(&dest)?)
        } else {
            None
        };
        let changed_since = current.as_ref().is_some_and(|sha| {
            *sha != entry.placed_sha256 && Some(sha) != entry.backup_sha256.as_ref()
        });
        if !changed_since {
            return Err(UpscalerError::Invalid(format!(
                "{} todavía se puede restaurar: usá Revertir",
                entry.rel
            )));
        }
    }
    let dir = patch_dir(root, id);
    fs::remove_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
    let _ = fs::remove_dir(root.join(PATCHES_DIR));
    Ok(())
}

/// Patch `id`'s manifest, if applied (fully, partly, or with conflicts). What
/// it names is checked like a plan: a manifest is a file in the game folder,
/// and revert deletes what it lists.
pub fn load(root: &Path, id: &str) -> Result<Option<Manifest>, UpscalerError> {
    validate_id(id)?;
    let path = patch_dir(root, id).join(MANIFEST);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_err(&path, e)),
    };
    let damaged = |why: String| UpscalerError::Manifest(format!("{}: {why}", path.display()));
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| damaged(e.to_string()))?;
    if manifest.id != id {
        return Err(damaged(format!("dice ser \"{}\"", manifest.id)));
    }
    let rels = manifest
        .entries
        .iter()
        .map(|e| &e.rel)
        .chain(&manifest.runtime)
        .chain(&manifest.created_dirs);
    for rel in rels {
        validate_rel(rel).map_err(|_| damaged(format!("ruta inválida {rel}")))?;
    }
    Ok(Some(manifest))
}

/// Every patch recorded in this game folder.
pub fn list(root: &Path) -> Vec<Manifest> {
    patch_ids(root)
        .into_iter()
        .filter_map(|id| load(root, &id).ok().flatten())
        .collect()
}

/// Patch folders whose manifest can't be read: shown, never silently skipped.
pub fn broken(root: &Path) -> Vec<String> {
    patch_ids(root)
        .into_iter()
        .filter(|id| load(root, id).is_err())
        .collect()
}

fn patch_ids(root: &Path) -> Vec<String> {
    let Ok(dirs) = fs::read_dir(root.join(PATCHES_DIR)) else {
        return Vec::new();
    };
    dirs.flatten()
        .filter(|d| d.path().is_dir())
        .map(|d| d.file_name().to_string_lossy().into_owned())
        .collect()
}

/// Patch ids are folder names Tatu makes up: lowercase letters, digits, `-`.
fn validate_id(id: &str) -> Result<(), UpscalerError> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(UpscalerError::Invalid(format!("Parche inválido: {id}")))
    }
}

/// The patch that placed `rel` (any casing), if one did.
pub fn owner(root: &Path, rel: &str) -> Option<String> {
    let (_, rel) = resolve_ci(root, rel);
    files_of_other_patches(root, "").remove(&rel.to_lowercase())
}

fn files_of_other_patches(root: &Path, id: &str) -> HashMap<String, String> {
    list(root)
        .into_iter()
        .filter(|m| m.id != id)
        .flat_map(|m| {
            let id = m.id.clone();
            m.entries
                .into_iter()
                .map(move |e| (e.rel.to_lowercase(), id.clone()))
        })
        .collect()
}

fn patch_dir(root: &Path, id: &str) -> PathBuf {
    root.join(PATCHES_DIR).join(id)
}

/// The first manifest write: `create_new`, so two applies racing for the same
/// patch can't both get past it.
fn claim_manifest(dir: &Path, manifest: &Manifest) -> Result<(), UpscalerError> {
    let path = dir.join(MANIFEST);
    let mut file = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(UpscalerError::AlreadyApplied(manifest.id.clone()));
        }
        Err(e) => return Err(io_err(&path, e)),
    };
    let json = serde_json::to_vec_pretty(manifest).expect("manifest serializes");
    file.write_all(&json)
        .and_then(|()| file.sync_all())
        .map_err(|e| io_err(&path, e))
}

/// Replace the manifest atomically, flushed to disk before the rename: a
/// power cut leaves the old or the new one, never a torn file.
fn write_manifest(dir: &Path, manifest: &Manifest) -> Result<(), UpscalerError> {
    let path = dir.join(MANIFEST);
    let tmp = dir.join(format!("{MANIFEST}{TMP_SUFFIX}"));
    let json = serde_json::to_vec_pretty(manifest).expect("manifest serializes");
    let mut file = fs::File::create(&tmp).map_err(|e| io_err(&tmp, e))?;
    file.write_all(&json)
        .and_then(|()| file.sync_all())
        .map_err(|e| io_err(&tmp, e))?;
    drop(file);
    fs::rename(&tmp, &path).map_err(|e| io_err(&path, e))
}

/// `rel` under `root`, each component matched case-insensitively against what
/// is on disk (exact case first). Returns the path and its on-disk `rel`;
/// components that don't exist keep the case they were given.
pub(crate) fn resolve_ci(root: &Path, rel: &str) -> (PathBuf, String) {
    let mut path = root.to_path_buf();
    let mut parts = Vec::new();
    for part in rel.split('/').filter(|p| !p.is_empty()) {
        let names: Vec<String> = fs::read_dir(&path)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        let name = names
            .iter()
            .find(|n| n.as_str() == part)
            .or_else(|| names.iter().find(|n| n.eq_ignore_ascii_case(part)))
            .cloned()
            .unwrap_or_else(|| part.to_string());
        path.push(&name);
        parts.push(name);
    }
    (path, parts.join("/"))
}

/// Folders on the way to `rel` that don't exist yet, outermost first.
fn missing_dirs(root: &Path, rel: &str) -> Vec<String> {
    let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
    (1..parts.len())
        .map(|n| parts[..n].join("/"))
        .filter(|dir| !resolve_ci(root, dir).0.exists())
        .collect()
}

fn validate_rel(rel: &str) -> Result<(), UpscalerError> {
    let bad = rel.is_empty()
        || rel.starts_with('/')
        || rel.contains('\\')
        || rel.contains(':')
        || rel
            .split('/')
            .any(|p| p == ".." || p == "." || p.eq_ignore_ascii_case(PATCHES_DIR));
    if bad {
        return Err(UpscalerError::Invalid(format!(
            "Ruta inválida en el parche: {rel}"
        )));
    }
    Ok(())
}

/// Before anything changes: every folder we write into accepts writes, the
/// disk has room for the copies (backups are moves, they cost nothing).
fn preflight(root: &Path, steps: &[Step], bytes: u64) -> Result<(), UpscalerError> {
    let mut probed: Vec<PathBuf> = Vec::new();
    let dirs = std::iter::once(root.to_path_buf()).chain(
        steps
            .iter()
            .filter_map(|s| s.dest.parent().map(nearest_existing)),
    );
    for dir in dirs {
        if probed.contains(&dir) {
            continue;
        }
        let probe = dir.join(".tatu-write-test");
        if fs::write(&probe, b"").is_err() {
            return Err(UpscalerError::NotWritable(dir.display().to_string()));
        }
        let _ = fs::remove_file(&probe);
        probed.push(dir);
    }

    let needed = bytes + bytes / 10;
    if let Some(available) = available_bytes(root)
        && available < needed
    {
        return Err(UpscalerError::NoSpace {
            needed_mb: needed.div_ceil(1 << 20),
            available_mb: available >> 20,
        });
    }
    Ok(())
}

fn nearest_existing(path: &Path) -> PathBuf {
    path.ancestors()
        .find(|p| p.is_dir())
        .unwrap_or(path)
        .to_path_buf()
}

fn available_bytes(path: &Path) -> Option<u64> {
    let path = fs::canonicalize(path).ok()?;
    let path = path.to_string_lossy();
    let path = path.trim_start_matches(r"\\?\").to_lowercase();
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| path.starts_with(&d.mount_point().to_string_lossy().to_lowercase()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

fn has_files(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let path = e.path();
        if path.is_dir() {
            has_files(&path)
        } else {
            true
        }
    })
}

fn tmp_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(TMP_SUFFIX);
    dest.with_file_name(name)
}

pub(crate) fn sha256_file(path: &Path) -> Result<String, UpscalerError> {
    let mut file = fs::File::open(path).map_err(|e| io_err(path, e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(|e| io_err(path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// `fs::copy` carries the read-only attribute over, and Windows then refuses
/// to delete the file on revert. (Unix deletes by folder permission.)
#[cfg(windows)]
fn clear_readonly(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = fs::set_permissions(path, perms);
        }
    }
}

#[cfg(not(windows))]
fn clear_readonly(_path: &Path) {}

/// The exes whose running means "the game is open": the main one and every
/// exe next to where the patch goes — an Unreal game's root exe is a stub
/// that starts `<Project>/Binaries/Win64/*-Shipping.exe` and exits.
pub fn game_exes(root: &Path, main_exe: Option<&str>, dirs: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = main_exe
        .and_then(|exe| exe.rsplit(['/', '\\']).next())
        .map(|name| vec![name.to_lowercase()])
        .unwrap_or_default();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(resolve_ci(root, dir).0) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.ends_with(".exe") && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// Whether any of `exes` (lowercase file names) is running. Windows: by
/// process name. Linux: Wine/Proton keep the exe's Windows path in the
/// process command line.
#[cfg(windows)]
pub fn is_running(exes: &[String]) -> bool {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    if exes.is_empty() {
        return false;
    }
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    system
        .processes()
        .values()
        .any(|p| exes.contains(&p.name().to_string_lossy().to_lowercase()))
}

#[cfg(not(windows))]
pub fn is_running(exes: &[String]) -> bool {
    if exes.is_empty() {
        return false;
    }
    let Ok(procs) = fs::read_dir("/proc") else {
        return false;
    };
    procs
        .flatten()
        .filter(|p| {
            p.file_name()
                .to_string_lossy()
                .bytes()
                .all(|b| b.is_ascii_digit())
        })
        .filter_map(|p| fs::read(p.path().join("cmdline")).ok())
        .any(|cmdline| {
            cmdline.split(|b| *b == 0).any(|arg| {
                let arg = String::from_utf8_lossy(arg).to_lowercase();
                arg.rsplit(['/', '\\'])
                    .next()
                    .is_some_and(|name| exes.iter().any(|e| e == name))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for e in fs::read_dir(&dir).unwrap().flatten() {
                let path = e.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    let rel = path
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.push((rel, fs::read(&path).unwrap()));
                }
            }
        }
        out.sort();
        out
    }

    /// A game with its own libxess and a package replacing it, adding two
    /// files (one in a new folder) and an ini the program owns.
    fn setup() -> (tempfile::TempDir, PathBuf, Plan) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("game");
        write(&root.join("Game.exe"), b"exe");
        write(&root.join("libxess.dll"), b"game xess v1");
        let pkg = tmp.path().join("pkg");
        write(&pkg.join("OptiScaler.dll"), b"opti");
        write(&pkg.join("libxess.dll"), b"opti xess v2");
        write(&pkg.join("D3D12_Optiscaler/D3D12Core.dll"), b"core");
        write(&pkg.join("OptiScaler.ini"), b"[Upscalers]");
        let plan = Plan {
            id: "optiscaler".into(),
            sources: vec![
                Source {
                    from: pkg.join("OptiScaler.dll"),
                    rel: "dxgi.dll".into(),
                    owned: false,
                },
                Source {
                    from: pkg.join("libxess.dll"),
                    rel: "libxess.dll".into(),
                    owned: false,
                },
                Source {
                    from: pkg.join("D3D12_Optiscaler/D3D12Core.dll"),
                    rel: "D3D12_Optiscaler/D3D12Core.dll".into(),
                    owned: false,
                },
                Source {
                    from: pkg.join("OptiScaler.ini"),
                    rel: "OptiScaler.ini".into(),
                    owned: true,
                },
            ],
            runtime: vec!["OptiScaler.log".into()],
            meta: serde_json::json!({"version": "test"}),
            running: Vec::new(),
        };
        (tmp, root, plan)
    }

    #[test]
    fn revert_puts_the_folder_back_byte_for_byte() {
        let (_tmp, root, plan) = setup();
        let before = snapshot(&root);

        let manifest = apply(&root, &plan).unwrap();
        assert_eq!(manifest.backed_up(), vec!["libxess.dll".to_string()]);
        assert_eq!(fs::read(root.join("libxess.dll")).unwrap(), b"opti xess v2");
        assert_eq!(fs::read(root.join("dxgi.dll")).unwrap(), b"opti");

        // The program rewrites its own ini and writes a log while the game runs.
        fs::write(
            root.join("OptiScaler.ini"),
            b"[Upscalers]\nDx12Upscaler=xess",
        )
        .unwrap();
        fs::write(root.join("OptiScaler.log"), b"log").unwrap();

        let report = revert(&root, "optiscaler").unwrap();
        assert!(report.complete(), "{report:?}");
        assert_eq!(report.restored, vec!["libxess.dll"]);
        assert_eq!(snapshot(&root), before);
        assert!(!root.join(PATCHES_DIR).exists());
        assert!(!root.join("D3D12_Optiscaler").exists());
    }

    #[test]
    fn a_file_changed_after_the_patch_is_never_overwritten_or_lost() {
        let (_tmp, root, plan) = setup();
        apply(&root, &plan).unwrap();
        // A game update rewrites both a file we replaced and one we added.
        fs::write(root.join("libxess.dll"), b"game xess v3").unwrap();
        fs::write(root.join("dxgi.dll"), b"someone else's dxgi").unwrap();

        let report = revert(&root, "optiscaler").unwrap();
        assert_eq!(report.conflicts, vec!["libxess.dll"]);
        assert_eq!(report.kept, vec!["dxgi.dll"]);
        assert_eq!(fs::read(root.join("libxess.dll")).unwrap(), b"game xess v3");
        assert_eq!(
            fs::read(root.join("dxgi.dll")).unwrap(),
            b"someone else's dxgi"
        );
        // The original is still backed up and the patch still recorded.
        let manifest = load(&root, "optiscaler").unwrap().unwrap();
        assert_eq!(manifest.state, PatchState::Conflict);
        assert_eq!(manifest.backed_up(), vec!["libxess.dll".to_string()]);
        let backup = root
            .join(PATCHES_DIR)
            .join("optiscaler")
            .join(BACKUP)
            .join("libxess.dll");
        assert_eq!(fs::read(backup).unwrap(), b"game xess v1");

        // Once the file is gone (the user deleted it), running revert again finishes.
        fs::remove_file(root.join("libxess.dll")).unwrap();
        let report = revert(&root, "optiscaler").unwrap();
        assert!(report.complete(), "{report:?}");
        assert_eq!(fs::read(root.join("libxess.dll")).unwrap(), b"game xess v1");
        assert!(!root.join(PATCHES_DIR).exists());
    }

    #[test]
    fn an_original_restored_by_verify_just_drops_the_backup() {
        let (_tmp, root, plan) = setup();
        apply(&root, &plan).unwrap();
        fs::write(root.join("libxess.dll"), b"game xess v1").unwrap();
        let report = revert(&root, "optiscaler").unwrap();
        assert!(report.complete());
        assert_eq!(fs::read(root.join("libxess.dll")).unwrap(), b"game xess v1");
        assert!(!root.join(PATCHES_DIR).exists());
    }

    #[test]
    fn an_interrupted_apply_reverts_cleanly() {
        let (_tmp, root, _plan) = setup();
        let dir = root.join(PATCHES_DIR).join("optiscaler");
        // Crash 1: libxess moved into the backup, ours never renamed in.
        write(&dir.join(BACKUP).join("libxess.dll"), b"game xess v1");
        fs::remove_file(root.join("libxess.dll")).unwrap();
        write(&root.join("libxess.dll.tatu-tmp"), b"opti xess v2");
        // Crash 2: an entry journaled whose original was never moved.
        write(&root.join("dxgi.dll"), b"reshade");
        let manifest = Manifest {
            id: "optiscaler".into(),
            state: PatchState::Applying,
            entries: vec![
                Entry {
                    rel: "libxess.dll".into(),
                    placed_sha256: "whatever".into(),
                    backup_sha256: Some(
                        sha256_file(&dir.join(BACKUP).join("libxess.dll")).unwrap(),
                    ),
                    owned: false,
                },
                Entry {
                    rel: "dxgi.dll".into(),
                    placed_sha256: "whatever".into(),
                    backup_sha256: Some(sha256_file(&root.join("dxgi.dll")).unwrap()),
                    owned: false,
                },
            ],
            runtime: vec![],
            created_dirs: vec![],
            meta: serde_json::Value::Null,
        };
        write_manifest(&dir, &manifest).unwrap();

        let report = revert(&root, "optiscaler").unwrap();
        assert!(report.complete(), "{report:?}");
        assert_eq!(fs::read(root.join("libxess.dll")).unwrap(), b"game xess v1");
        assert_eq!(fs::read(root.join("dxgi.dll")).unwrap(), b"reshade");
        assert!(!root.join("libxess.dll.tatu-tmp").exists());
        assert!(!root.join(PATCHES_DIR).exists());
    }

    #[test]
    fn names_match_whatever_case_the_game_uses() {
        let (_tmp, root, plan) = setup();
        fs::rename(root.join("libxess.dll"), root.join("LibXess.DLL")).unwrap();
        let manifest = apply(&root, &plan).unwrap();
        assert_eq!(manifest.backed_up(), vec!["LibXess.DLL".to_string()]);
        let names: Vec<String> = fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_lowercase())
            .filter(|n| n == "libxess.dll")
            .collect();
        assert_eq!(names.len(), 1, "never a second copy in another case");

        revert(&root, "optiscaler").unwrap();
        assert_eq!(fs::read(root.join("LibXess.DLL")).unwrap(), b"game xess v1");
    }

    #[test]
    fn identical_files_are_left_alone_and_patches_never_stack() {
        let (tmp, root, plan) = setup();
        fs::write(root.join("libxess.dll"), b"opti xess v2").unwrap();
        let manifest = apply(&root, &plan).unwrap();
        assert!(manifest.backed_up().is_empty());
        assert!(manifest.entries.iter().all(|e| e.rel != "libxess.dll"));

        assert!(matches!(
            apply(&root, &plan),
            Err(UpscalerError::AlreadyApplied(_))
        ));

        let other = Plan {
            id: "dlss".into(),
            sources: vec![Source {
                from: tmp.path().join("pkg/OptiScaler.dll"),
                rel: "DXGI.dll".into(),
                owned: false,
            }],
            runtime: vec![],
            meta: serde_json::Value::Null,
            running: Vec::new(),
        };
        assert!(matches!(
            apply(&root, &other),
            Err(UpscalerError::Overlap(_, _))
        ));
        assert!(revert(&root, "optiscaler").unwrap().complete());
        // Untouched by the patch, so still there.
        assert_eq!(fs::read(root.join("libxess.dll")).unwrap(), b"opti xess v2");
    }

    #[test]
    fn a_log_that_was_there_before_survives_revert() {
        let (_tmp, root, plan) = setup();
        fs::write(root.join("OptiScaler.log"), b"from a manual install").unwrap();
        apply(&root, &plan).unwrap();
        revert(&root, "optiscaler").unwrap();
        assert_eq!(
            fs::read(root.join("OptiScaler.log")).unwrap(),
            b"from a manual install"
        );
    }

    #[test]
    fn paths_that_escape_the_game_are_refused() {
        let (tmp, root, _plan) = setup();
        for rel in [
            "../evil.dll",
            "/abs.dll",
            "C:/x.dll",
            ".tatu-patches/x.dll",
            "a\\b.dll",
        ] {
            let plan = Plan {
                id: "x".into(),
                sources: vec![Source {
                    from: tmp.path().join("pkg/OptiScaler.dll"),
                    rel: rel.into(),
                    owned: false,
                }],
                runtime: vec![],
                meta: serde_json::Value::Null,
                running: Vec::new(),
            };
            assert!(
                matches!(apply(&root, &plan), Err(UpscalerError::Invalid(_))),
                "{rel}"
            );
        }
    }

    #[test]
    fn a_failure_mid_apply_rolls_everything_back() {
        let (_tmp, root, plan) = setup();
        let before = snapshot(&root);
        // A file where the backup folder should go: moving the game's
        // libxess aside fails after other files were already placed.
        write(
            &root.join(PATCHES_DIR).join("optiscaler/backup"),
            b"in the way",
        );

        assert!(apply(&root, &plan).is_err());
        assert_eq!(snapshot(&root), before);
        assert!(!root.join(PATCHES_DIR).exists());
    }

    #[test]
    fn a_tampered_backup_is_never_put_back() {
        let (_tmp, root, plan) = setup();
        apply(&root, &plan).unwrap();
        let backup = root.join(PATCHES_DIR).join("optiscaler/backup/libxess.dll");
        fs::write(&backup, b"not the original").unwrap();
        let report = revert(&root, "optiscaler").unwrap();
        assert_eq!(report.conflicts, vec!["libxess.dll"]);
        assert_eq!(fs::read(root.join("libxess.dll")).unwrap(), b"opti xess v2");
        assert!(backup.exists());
    }

    #[test]
    fn runtime_folders_go_but_folders_the_game_had_stay() {
        let (_tmp, root, mut plan) = setup();
        plan.runtime.push("DlssOverrides".into());
        write(&root.join("Licenses/game.txt"), b"game");
        apply(&root, &plan).unwrap();
        write(&root.join("DlssOverrides/preset.json"), b"{}");
        assert!(revert(&root, "optiscaler").unwrap().complete());
        assert!(!root.join("DlssOverrides").exists());
        assert!(root.join("Licenses/game.txt").is_file());
    }

    #[test]
    fn a_manifest_naming_paths_outside_the_game_is_refused() {
        let (_tmp, root, _plan) = setup();
        let victim = root.parent().unwrap().join("victim.txt");
        fs::write(&victim, b"keep me").unwrap();
        let dir = root.join(PATCHES_DIR).join("evil");
        fs::create_dir_all(&dir).unwrap();
        let manifest = Manifest {
            id: "evil".into(),
            state: PatchState::Applied,
            entries: vec![Entry {
                rel: "../victim.txt".into(),
                placed_sha256: sha256_file(&victim).unwrap(),
                backup_sha256: None,
                owned: true,
            }],
            runtime: vec!["..".into()],
            created_dirs: vec![],
            meta: serde_json::Value::Null,
        };
        write_manifest(&dir, &manifest).unwrap();
        assert!(matches!(
            revert(&root, "evil"),
            Err(UpscalerError::Manifest(_))
        ));
        assert!(matches!(
            revert(&root, ".."),
            Err(UpscalerError::Invalid(_))
        ));
        assert_eq!(fs::read(&victim).unwrap(), b"keep me");
        assert!(root.join("Game.exe").is_file());
        assert_eq!(broken(&root), vec!["evil".to_string()]);
    }

    #[test]
    fn an_orphan_backup_stops_apply_before_anything_changes() {
        let (_tmp, root, plan) = setup();
        write(
            &root.join(PATCHES_DIR).join("optiscaler/backup/libxess.dll"),
            b"the real original",
        );
        let before = snapshot(&root);
        assert!(matches!(
            apply(&root, &plan),
            Err(UpscalerError::OrphanBackup(_))
        ));
        assert_eq!(snapshot(&root), before);
    }

    #[test]
    fn keep_current_closes_a_conflict_but_never_a_restorable_patch() {
        let (_tmp, root, plan) = setup();
        apply(&root, &plan).unwrap();
        assert!(
            keep_current(&root, "optiscaler").is_err(),
            "still restorable"
        );

        fs::write(root.join("libxess.dll"), b"game xess v3").unwrap();
        revert(&root, "optiscaler").unwrap();
        keep_current(&root, "optiscaler").unwrap();
        assert_eq!(fs::read(root.join("libxess.dll")).unwrap(), b"game xess v3");
        assert!(!root.join(PATCHES_DIR).exists());
    }

    #[test]
    fn the_patches_folder_is_off_limits_in_any_case() {
        let (tmp, root, _plan) = setup();
        let plan = Plan {
            id: "x".into(),
            sources: vec![Source {
                from: tmp.path().join("pkg/OptiScaler.dll"),
                rel: ".TATU-PATCHES/optiscaler/backup/libxess.dll".into(),
                owned: false,
            }],
            runtime: vec![],
            meta: serde_json::Value::Null,
            running: Vec::new(),
        };
        assert!(matches!(
            apply(&root, &plan),
            Err(UpscalerError::Invalid(_))
        ));
    }

    #[test]
    fn an_identical_config_is_still_recorded_so_later_edits_revert() {
        // A manual OptiScaler install already has the very same ini: it's
        // recorded anyway, so edits made afterwards (Tatu's settings, the
        // in-game menu) don't outlive the revert.
        let (_tmp, root, plan) = setup();
        write(&root.join("OptiScaler.ini"), b"[Upscalers]");
        apply(&root, &plan).unwrap();
        fs::write(
            root.join("OptiScaler.ini"),
            b"[Upscalers]
Dx12Upscaler=xess",
        )
        .unwrap();
        assert!(revert(&root, "optiscaler").unwrap().complete());
        assert_eq!(
            fs::read(root.join("OptiScaler.ini")).unwrap(),
            b"[Upscalers]"
        );
    }
}
