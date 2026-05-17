use crate::types::CheatTable;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn load_cheat_table(cheats_dir: &Path, app_id: u64) -> Result<CheatTable, DbError> {
    let path = cheats_dir.join(format!("{app_id}.json"));
    let content = std::fs::read_to_string(&path)?;
    let table = serde_json::from_str(&content)?;
    Ok(table)
}

pub fn list_app_ids_with_cheats(cheats_dir: &Path) -> Result<Vec<u64>, DbError> {
    if !cheats_dir.exists() {
        return Ok(Vec::new());
    }

    let mut ids = Vec::new();
    for entry in std::fs::read_dir(cheats_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };

        if let Ok(id) = stem.parse::<u64>() {
            ids.push(id);
        }
    }

    ids.sort_unstable();
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_fixture(dir: &Path, app_id: u64, json: &str) {
        std::fs::write(dir.join(format!("{app_id}.json")), json).expect("write fixture");
    }

    fn minimal_cheat_table_json(app_id: u64) -> String {
        format!(
            r#"{{
                "app_id": {app_id},
                "game_name": "Test Game",
                "exe_pattern": "test.exe",
                "cheats": [{{
                    "id": "test",
                    "name": "Test Cheat",
                    "address": {{ "kind": "Static", "module": "test.exe", "offset": "0x100" }},
                    "action": {{ "kind": "WriteOnce", "value": {{ "type": "u32", "value": 42 }} }}
                }}]
            }}"#
        )
    }

    #[test]
    fn load_cheat_table_roundtrips() {
        let dir = TempDir::new().expect("tempdir");
        write_fixture(dir.path(), 12345, &minimal_cheat_table_json(12345));

        let table = load_cheat_table(dir.path(), 12345).expect("load");
        assert_eq!(table.app_id, 12345);
        assert_eq!(table.game_name, "Test Game");
        assert_eq!(table.cheats.len(), 1);
        assert_eq!(table.cheats[0].id, "test");
    }

    #[test]
    fn load_cheat_table_missing_file_returns_io_error() {
        let dir = TempDir::new().expect("tempdir");
        let result = load_cheat_table(dir.path(), 99999);
        assert!(matches!(result, Err(DbError::Io(_))));
    }

    #[test]
    fn load_cheat_table_malformed_json_returns_json_error() {
        let dir = TempDir::new().expect("tempdir");
        write_fixture(dir.path(), 1, "{ not valid json");

        let result = load_cheat_table(dir.path(), 1);
        assert!(matches!(result, Err(DbError::Json(_))));
    }

    #[test]
    fn list_app_ids_returns_sorted_numeric_filenames() {
        let dir = TempDir::new().expect("tempdir");
        write_fixture(dir.path(), 49520, &minimal_cheat_table_json(49520));
        write_fixture(dir.path(), 12345, &minimal_cheat_table_json(12345));
        write_fixture(dir.path(), 67890, &minimal_cheat_table_json(67890));
        std::fs::write(dir.path().join("ignored.json"), "{}").unwrap();
        std::fs::write(dir.path().join("README.md"), "docs").unwrap();

        let ids = list_app_ids_with_cheats(dir.path()).expect("list");
        assert_eq!(ids, vec![12345, 49520, 67890]);
    }

    #[test]
    fn list_app_ids_missing_dir_returns_empty() {
        let dir = TempDir::new().expect("tempdir");
        let nonexistent = dir.path().join("does-not-exist");
        let ids = list_app_ids_with_cheats(&nonexistent).expect("list");
        assert!(ids.is_empty());
    }
}
