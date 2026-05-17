use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::CheatError;
use crate::attach::find_process_by_exe;
use crate::memory::write_bytes;
use crate::resolve::resolve_address;
use crate::types::{CheatAction, CheatTable};

pub const DEFAULT_INTERVAL_MS: u64 = 16;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct FreezeKey {
    pub app_id: u64,
    pub cheat_id: String,
}

struct FreezeHandle {
    cancel: Arc<AtomicBool>,
    // Kept so the thread can be joined on Drop if a caller ever wants
    // synchronous shutdown — current API does not block on join, the OS
    // reaps the thread within `interval_ms` of cancel being signalled.
    _join: JoinHandle<()>,
}

#[derive(Default)]
pub struct FreezeRegistry {
    active: Mutex<HashMap<FreezeKey, FreezeHandle>>,
}

impl FreezeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a freeze loop for the given cheat. Returns `Ok(true)` if a new
    /// loop was spawned, `Ok(false)` if one was already active for this key.
    pub fn start(&self, table: &CheatTable, cheat_id: &str) -> Result<bool, CheatError> {
        let key = FreezeKey {
            app_id: table.app_id,
            cheat_id: cheat_id.to_string(),
        };

        let mut active = self.active.lock().expect("freeze registry poisoned");
        if active.contains_key(&key) {
            return Ok(false);
        }

        let cheat = table
            .cheats
            .iter()
            .find(|c| c.id == cheat_id)
            .ok_or_else(|| CheatError::CheatNotFound(cheat_id.to_string()))?;

        let CheatAction::Freeze { value, interval_ms } = &cheat.action else {
            return Err(CheatError::ActionMismatch {
                cheat_id: cheat_id.to_string(),
                expected: "Freeze",
                actual: cheat.action.kind_name(),
            });
        };

        // Resolve once. cheat-core v1 does not support DMA games where
        // addresses relocate per-scene; PointerChain offsets are walked
        // here and the resulting absolute address is captured by the
        // worker. If the game relocates, the loop will write to a stale
        // address (silent no-op) or fault and exit on the next iteration.
        let attached = find_process_by_exe(&table.exe_pattern)?;
        let address = resolve_address(&cheat.address, &attached)?;
        let pid = attached.pid;
        let bytes = value.to_le_bytes();
        let interval = Duration::from_millis(interval_ms.unwrap_or(DEFAULT_INTERVAL_MS));

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_worker = Arc::clone(&cancel);
        let join = std::thread::spawn(move || {
            while !cancel_worker.load(Ordering::Relaxed) {
                if write_bytes(pid, address, &bytes).is_err() {
                    // Target process gone or address unmapped — exit
                    // silently. Stale entry stays in the registry until
                    // `stop` is called or the registry is dropped.
                    break;
                }
                std::thread::sleep(interval);
            }
        });

        active.insert(
            key,
            FreezeHandle {
                cancel,
                _join: join,
            },
        );
        Ok(true)
    }

    /// Stop a freeze loop. Returns `true` if a loop was active and signalled
    /// to cancel, `false` if no loop existed for this key. Does not block on
    /// the worker thread exiting.
    pub fn stop(&self, key: &FreezeKey) -> bool {
        let mut active = self.active.lock().expect("freeze registry poisoned");
        let Some(handle) = active.remove(key) else {
            return false;
        };
        handle.cancel.store(true, Ordering::Relaxed);
        true
    }

    pub fn is_active(&self, key: &FreezeKey) -> bool {
        self.active
            .lock()
            .expect("freeze registry poisoned")
            .contains_key(key)
    }

    pub fn active_keys(&self) -> Vec<FreezeKey> {
        self.active
            .lock()
            .expect("freeze registry poisoned")
            .keys()
            .cloned()
            .collect()
    }
}

impl Drop for FreezeRegistry {
    fn drop(&mut self) {
        // Signal cancel to every worker. The OS reaps them within
        // `interval_ms`; we don't join to keep app shutdown snappy.
        let active = self.active.lock().expect("freeze registry poisoned");
        for handle in active.values() {
            handle.cancel.store(true, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AddressSpec, Cheat, CheatValue};

    fn freeze_table_with_address(
        address: u64,
        value: CheatValue,
        interval_ms: Option<u64>,
    ) -> CheatTable {
        CheatTable {
            app_id: 42,
            game_name: "test".into(),
            exe_pattern: own_exe_name(),
            cheats: vec![Cheat {
                id: "frozen".into(),
                name: "Frozen".into(),
                description: None,
                address: AddressSpec::Absolute { address },
                action: CheatAction::Freeze { value, interval_ms },
            }],
        }
    }

    fn write_once_table() -> CheatTable {
        CheatTable {
            app_id: 1,
            game_name: "test".into(),
            exe_pattern: own_exe_name(),
            cheats: vec![Cheat {
                id: "once".into(),
                name: "Once".into(),
                description: None,
                address: AddressSpec::Absolute { address: 0xDEAD },
                action: CheatAction::WriteOnce {
                    value: CheatValue::I32(1),
                },
            }],
        }
    }

    fn own_exe_name() -> String {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "cheat_core".into())
    }

    #[test]
    fn start_on_non_freeze_action_errors_with_action_mismatch() {
        let registry = FreezeRegistry::new();
        let table = write_once_table();
        match registry.start(&table, "once") {
            Err(CheatError::ActionMismatch {
                expected, actual, ..
            }) => {
                assert_eq!(expected, "Freeze");
                assert_eq!(actual, "WriteOnce");
            }
            other => panic!("expected ActionMismatch, got {other:?}"),
        }
        assert!(registry.active_keys().is_empty());
    }

    #[test]
    fn start_on_unknown_cheat_id_errors_with_cheat_not_found() {
        let registry = FreezeRegistry::new();
        let table = write_once_table();
        match registry.start(&table, "nonexistent") {
            Err(CheatError::CheatNotFound(id)) => assert_eq!(id, "nonexistent"),
            other => panic!("expected CheatNotFound, got {other:?}"),
        }
    }

    #[test]
    fn stop_on_unknown_key_returns_false() {
        let registry = FreezeRegistry::new();
        let key = FreezeKey {
            app_id: 0,
            cheat_id: "missing".into(),
        };
        assert!(!registry.stop(&key));
        assert!(!registry.is_active(&key));
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn start_then_stop_freezes_then_releases_value() {
        use std::sync::atomic::AtomicU32;
        let target = Box::new(AtomicU32::new(0));
        let address = target.as_ref() as *const AtomicU32 as u64;
        let table = freeze_table_with_address(address, CheatValue::U32(0xCAFE_BABE), Some(5));

        let registry = FreezeRegistry::new();
        assert!(registry.start(&table, "frozen").expect("start"));
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(
            target.load(Ordering::Relaxed),
            0xCAFE_BABE,
            "value should have been frozen"
        );

        let key = FreezeKey {
            app_id: 42,
            cheat_id: "frozen".into(),
        };
        assert!(registry.stop(&key));
        std::thread::sleep(Duration::from_millis(20));

        target.store(0xDEAD_BEEF, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(
            target.load(Ordering::Relaxed),
            0xDEAD_BEEF,
            "value should stay free after stop"
        );
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn start_twice_returns_false_second_time() {
        use std::sync::atomic::AtomicU32;
        let target = Box::new(AtomicU32::new(0));
        let address = target.as_ref() as *const AtomicU32 as u64;
        let table = freeze_table_with_address(address, CheatValue::U32(7), Some(50));

        let registry = FreezeRegistry::new();
        assert!(registry.start(&table, "frozen").expect("first start"));
        assert!(!registry.start(&table, "frozen").expect("second start"));
        assert_eq!(registry.active_keys().len(), 1);

        let key = FreezeKey {
            app_id: 42,
            cheat_id: "frozen".into(),
        };
        registry.stop(&key);
    }

    #[test]
    fn drop_signals_cancel_to_all_handles() {
        use std::sync::atomic::AtomicU32;
        let target = Box::new(AtomicU32::new(0));
        let address = target.as_ref() as *const AtomicU32 as u64;
        let table = freeze_table_with_address(address, CheatValue::U32(0), Some(10));

        let registry = FreezeRegistry::new();
        if registry.start(&table, "frozen").is_err() {
            // ptrace blocked / process self-attach disallowed in this env —
            // skip the worker-spawn path; the no-active drop is still
            // exercised by simply dropping below.
            drop(registry);
            return;
        }

        let cancel_snapshot = {
            let active = registry.active.lock().expect("lock");
            active
                .values()
                .next()
                .map(|h| Arc::clone(&h.cancel))
                .expect("one active handle")
        };

        drop(registry);
        assert!(cancel_snapshot.load(Ordering::Relaxed));
    }
}
