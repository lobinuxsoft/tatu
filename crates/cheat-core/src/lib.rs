pub mod attach;
pub mod db;
pub mod memory;
pub mod resolve;
pub mod types;

use crate::attach::{AttachError, find_process_by_exe};
use crate::memory::MemoryError;
use crate::resolve::{ResolveError, resolve_address};
use crate::types::{CheatAction, CheatTable};

#[derive(Debug, thiserror::Error)]
pub enum CheatError {
    #[error("cheat '{0}' not found in table")]
    CheatNotFound(String),
    #[error(transparent)]
    Attach(#[from] AttachError),
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error(transparent)]
    Memory(#[from] MemoryError),
}

pub fn trigger_cheat(table: &CheatTable, cheat_id: &str) -> Result<(), CheatError> {
    let cheat = table
        .cheats
        .iter()
        .find(|c| c.id == cheat_id)
        .ok_or_else(|| CheatError::CheatNotFound(cheat_id.to_string()))?;

    let attached = find_process_by_exe(&table.exe_pattern)?;
    let address = resolve_address(&cheat.address, &attached)?;

    match &cheat.action {
        CheatAction::WriteOnce { value } => {
            memory::write_bytes(attached.pid, address, &value.to_le_bytes())?;
        }
    }

    Ok(())
}

pub fn is_process_running(exe_pattern: &str) -> bool {
    find_process_by_exe(exe_pattern).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_cheat_unknown_id_errors_before_attaching() {
        let table = CheatTable {
            app_id: 1,
            game_name: "test".into(),
            exe_pattern: "irrelevant-since-cheat-lookup-comes-first".into(),
            cheats: vec![],
        };
        let result = trigger_cheat(&table, "nonexistent");
        assert!(matches!(result, Err(CheatError::CheatNotFound(_))));
    }

    #[test]
    fn is_process_running_false_for_unknown_pattern() {
        assert!(!is_process_running(
            "definitely-not-real-process-xyzzy-99999-cheat-core-test"
        ));
    }
}
