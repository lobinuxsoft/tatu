//! Launching CE Linux for a Steam game with a per-game patched `.CT` table.
//!
//! Source tables live under `$XDG_CONFIG_HOME/backlog-tracker/cheat-tables/<app_id>/`.
//! On launch the chosen table is read, auto-attach Lua is injected for the resolved
//! game `.exe`, the result is written to `$XDG_CACHE_HOME/backlog-tracker/cheat-tables-launched/<app_id>/`,
//! and `cheatengine-x86_64` is spawned detached with that path as positional arg.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::{InjectError, binary_path, ensure_auto_attach};

const CONFIG_SUBDIR: &str = "backlog-tracker/cheat-tables";
const CACHE_SUBDIR: &str = "backlog-tracker/cheat-tables-launched";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CtTableEntry {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub modified_unix: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("could not resolve config dir (XDG_CONFIG_HOME / HOME unset?)")]
    NoConfigDir,
    #[error("could not resolve cache dir (XDG_CACHE_HOME / HOME unset?)")]
    NoCacheDir,
    #[error("CE not installed: {0}")]
    CeNotInstalled(String),
    #[error("table name is not a single .CT file name: {0:?}")]
    InvalidTableName(String),
    #[error("table not found: {0}")]
    TableNotFound(PathBuf),
    #[error("inject failed: {0}")]
    Inject(#[from] InjectError),
    #[error("spawn failed: {0}")]
    Spawn(io::Error),
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

pub fn tables_dir_for(app_id: &str) -> Result<PathBuf, LaunchError> {
    Ok(dirs::config_dir()
        .ok_or(LaunchError::NoConfigDir)?
        .join(CONFIG_SUBDIR)
        .join(app_id))
}

pub fn launched_cache_for(app_id: &str) -> Result<PathBuf, LaunchError> {
    Ok(dirs::cache_dir()
        .ok_or(LaunchError::NoCacheDir)?
        .join(CACHE_SUBDIR)
        .join(app_id))
}

pub fn list_tables(app_id: &str) -> Result<Vec<CtTableEntry>, LaunchError> {
    let dir = tables_dir_for(app_id)?;
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    Ok(list_tables_in(&dir)?)
}

/// Inject auto-attach Lua into `<tables_dir>/<table_name>`, write the patched copy
/// under the cache dir, and spawn `cheatengine-x86_64` with it. Returns the path
/// of the patched table that was handed to CE.
///
/// The patched copy is regenerated every call (no caching) — cheap, and avoids
/// stale state if the user edited the source table between launches.
pub fn open_for_game(
    app_id: &str,
    exe_name: &str,
    table_name: &str,
) -> Result<PathBuf, LaunchError> {
    let table_name = sanitize_table_name(table_name)
        .ok_or_else(|| LaunchError::InvalidTableName(table_name.to_string()))?;
    let table_src = tables_dir_for(app_id)?.join(table_name);
    if !table_src.is_file() {
        return Err(LaunchError::TableNotFound(table_src));
    }

    let binary = binary_path().map_err(|e| LaunchError::CeNotInstalled(e.to_string()))?;
    if !binary.is_file() {
        return Err(LaunchError::CeNotInstalled(format!(
            "binary not found at {}",
            binary.display()
        )));
    }

    let source_xml = std::fs::read_to_string(&table_src)?;
    let patched = ensure_auto_attach(&source_xml, exe_name)?;

    let cache_dir = launched_cache_for(app_id)?;
    std::fs::create_dir_all(&cache_dir)?;
    let patched_path = cache_dir.join(table_name);
    std::fs::write(&patched_path, &patched)?;

    Command::new(&binary)
        .arg(&patched_path)
        .spawn()
        .map_err(LaunchError::Spawn)?;

    Ok(patched_path)
}

fn sanitize_table_name(name: &str) -> Option<&str> {
    let path = Path::new(name);
    if path.components().count() != 1 {
        return None;
    }
    let bare = path.file_name()?.to_str()?;
    if bare != name {
        return None;
    }
    if !name.to_lowercase().ends_with(".ct") {
        return None;
    }
    Some(name)
}

fn list_tables_in(dir: &Path) -> io::Result<Vec<CtTableEntry>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ct"))
        {
            continue;
        }
        let md = entry.metadata()?;
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let modified_unix = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        out.push(CtTableEntry {
            name: name.to_string(),
            path: path.clone(),
            size_bytes: md.len(),
            modified_unix,
        });
    }
    out.sort_by_key(|e| e.name.to_lowercase());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn sanitize_table_name_accepts_bare_ct() {
        assert_eq!(sanitize_table_name("game.CT"), Some("game.CT"));
        assert_eq!(sanitize_table_name("game.ct"), Some("game.ct"));
        assert_eq!(
            sanitize_table_name("my-fancy_table.ct"),
            Some("my-fancy_table.ct")
        );
    }

    #[test]
    fn sanitize_table_name_rejects_traversal() {
        assert_eq!(sanitize_table_name("../etc/passwd.ct"), None);
        assert_eq!(sanitize_table_name("sub/dir.ct"), None);
        assert_eq!(sanitize_table_name("/abs/path.ct"), None);
        assert_eq!(sanitize_table_name(""), None);
    }

    #[test]
    fn sanitize_table_name_rejects_non_ct_extension() {
        assert_eq!(sanitize_table_name("game.txt"), None);
        assert_eq!(sanitize_table_name("game"), None);
    }

    #[test]
    fn list_tables_in_returns_empty_for_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let entries = list_tables_in(tmp.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn list_tables_in_filters_to_ct_extension() {
        let tmp = TempDir::new().unwrap();
        write_file(&tmp.path().join("a.CT"), "<CheatTable/>");
        write_file(&tmp.path().join("b.ct"), "<CheatTable/>");
        write_file(&tmp.path().join("readme.txt"), "ignore me");
        write_file(&tmp.path().join("nested/c.CT"), "<CheatTable/>");

        let mut entries = list_tables_in(tmp.path()).unwrap();
        entries.sort_by(|x, y| x.name.cmp(&y.name));
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["a.CT", "b.ct"]);
    }

    #[test]
    fn list_tables_in_sorts_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        write_file(&tmp.path().join("Zelda.CT"), "<CheatTable/>");
        write_file(&tmp.path().join("alpha.CT"), "<CheatTable/>");

        let entries = list_tables_in(tmp.path()).unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alpha.CT", "Zelda.CT"]);
    }

    #[test]
    fn list_tables_in_captures_metadata() {
        let tmp = TempDir::new().unwrap();
        let body = "<CheatTable><LuaScript></LuaScript></CheatTable>";
        write_file(&tmp.path().join("a.CT"), body);

        let entries = list_tables_in(tmp.path()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].size_bytes as usize, body.len());
        assert!(entries[0].modified_unix > 0);
    }
}
