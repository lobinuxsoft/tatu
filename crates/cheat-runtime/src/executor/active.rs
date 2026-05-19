//! A successfully enabled cheat plus its rollback path on `Drop`.

use std::collections::HashMap;

use nix::unistd::Pid;

use crate::persisted_hook::{PersistedAlloc, PersistedHook, PersistedWrite};

use super::error::ExecError;
use super::rollback::rollback;

/// A successfully enabled cheat. Owns the undo log; calling [`Self::disable`]
/// (or dropping after a deliberate `forget`) reverts every byte the executor
/// wrote during ENABLE.
#[derive(Debug)]
#[must_use = "ActiveCheat owns the undo log; call .disable() or it will roll back on drop"]
pub struct ActiveCheat {
    pub(super) pid: Pid,
    pub(super) undo: Vec<(u64, Vec<u8>)>,
    /// Pairs of `(remote_address, size)` that were allocated via `Statement::Alloc`
    /// and need to be released with `munmap` when the cheat is disabled. Keyed
    /// by the AA symbol name so `Statement::Dealloc(symbol)` can find them.
    pub(super) allocs: HashMap<String, (u64, usize)>,
    /// Snapshot of every symbol the Engine bound during ENABLE — both AOB-scan
    /// results and allocs. Kept on the live cheat so downstream features
    /// (pointer-chain `Value` reads in particular) can look up
    /// `base_address` / `shop` / etc. while the master toggle is active. Goes
    /// stale on `.disable()`; the registry should drop the entry then.
    pub(super) symbols: HashMap<String, u64>,
    pub(super) disabled: bool,
}

impl ActiveCheat {
    pub(super) fn new(pid: Pid) -> Self {
        Self {
            pid,
            undo: Vec::new(),
            allocs: HashMap::new(),
            symbols: HashMap::new(),
            disabled: false,
        }
    }

    /// Read-only view of the symbol table this cheat established. The map
    /// is a snapshot taken at ENABLE time — it doesn't update if the game
    /// re-locates code afterwards (rare for AOB-scan-bound symbols, which
    /// pin to fixed module offsets) but a re-enable will refresh it.
    pub fn symbols(&self) -> &HashMap<String, u64> {
        &self.symbols
    }

    pub fn pid(&self) -> Pid {
        self.pid
    }

    pub fn writes(&self) -> usize {
        self.undo.len()
    }

    /// Capture the undo state into a serialisable record so a future
    /// process can replay the rollback even if this `ActiveCheat`'s
    /// owning runtime has been torn down. Caller fills in `app_id`,
    /// `feature_uuid`, `exe`, and `started_at` from its own context.
    pub fn to_persisted(
        &self,
        app_id: String,
        feature_uuid: String,
        exe: String,
        started_at: Option<String>,
    ) -> PersistedHook {
        PersistedHook {
            app_id,
            feature_uuid,
            pid: self.pid.as_raw(),
            exe,
            started_at,
            writes: self
                .undo
                .iter()
                .map(|(addr, bytes)| PersistedWrite {
                    addr: *addr,
                    original: bytes.clone(),
                })
                .collect(),
            allocs: self
                .allocs
                .iter()
                .map(|(symbol, (addr, size))| PersistedAlloc {
                    symbol: symbol.clone(),
                    addr: *addr,
                    size: *size,
                })
                .collect(),
        }
    }

    /// Build an `ActiveCheat` from a persisted record. The returned
    /// cheat carries no live symbol table (recovery doesn't need it) and
    /// is immediately ready for [`Self::disable`] to walk the undo log.
    pub fn from_persisted(record: &PersistedHook) -> Self {
        let undo = record
            .writes
            .iter()
            .map(|w| (w.addr, w.original.clone()))
            .collect();
        let allocs = record
            .allocs
            .iter()
            .map(|a| (a.symbol.clone(), (a.addr, a.size)))
            .collect();
        Self {
            pid: record.pid_typed(),
            undo,
            allocs,
            symbols: HashMap::new(),
            disabled: false,
        }
    }

    pub fn disable(mut self) -> Result<(), ExecError> {
        if self.disabled {
            return Ok(());
        }
        let result = rollback(&mut self);
        self.disabled = true;
        result
    }
}

impl Drop for ActiveCheat {
    fn drop(&mut self) {
        if !self.disabled && !self.undo.is_empty() {
            // Best-effort revert; we cannot return an error from Drop, but
            // we surface anything that fails so the user sees half-applied
            // hooks instead of a silent leak.
            if let Err(e) = rollback(self) {
                eprintln!(
                    "[cheat-runtime] WARNING: best-effort rollback on drop failed: {e} — the game may still carry trampoline bytes from this cheat. Re-launching the game restores the original .text."
                );
            }
        }
    }
}
