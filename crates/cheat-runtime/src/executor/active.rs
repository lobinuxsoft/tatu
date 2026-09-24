//! A successfully enabled cheat plus its rollback path on `Drop`.
//!
//! `ActiveCheat` owns the [`tatu_engine::EnableOutcome`] returned by
//! an enable cycle (or by `from_persisted` during crash recovery)
//! plus the originating `Pid`. Calling [`Self::disable`] builds a
//! fresh [`LinuxBackend`] for the same PID and replays the undo log
//! through [`tatu_engine::rollback`].

use std::collections::HashMap;

use nix::unistd::Pid;
use tatu_engine::EnableOutcome;

use crate::executor::ExecError;
use crate::linux_backend::LinuxBackend;
use crate::persisted_hook::{PersistedAlloc, PersistedHook, PersistedWrite};

#[derive(Debug)]
#[must_use = "ActiveCheat owns the undo log; call .disable() or it will roll back on drop"]
pub struct ActiveCheat {
    pub(super) pid: Pid,
    pub(super) outcome: EnableOutcome,
    pub(super) disabled: bool,
}

impl ActiveCheat {
    pub fn from_outcome(pid: Pid, outcome: EnableOutcome) -> Self {
        Self {
            pid,
            outcome,
            disabled: false,
        }
    }

    pub fn symbols(&self) -> &HashMap<String, u64> {
        &self.outcome.symbols
    }

    pub fn pid(&self) -> Pid {
        self.pid
    }

    pub fn writes(&self) -> usize {
        self.outcome.undo.len()
    }

    /// Snapshot to JSON for crash-recovery storage. Caller supplies
    /// `app_id` / `feature_uuid` / `exe` / timestamp from its own
    /// context.
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
                .outcome
                .undo
                .iter()
                .map(|(addr, bytes)| PersistedWrite {
                    addr: *addr,
                    original: bytes.clone(),
                })
                .collect(),
            allocs: self
                .outcome
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

    /// Reconstruct an `ActiveCheat` from a crash-recovery record.
    /// The symbol table is intentionally empty — recovery doesn't
    /// need it, and replays only need the undo + alloc pairs.
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
            outcome: EnableOutcome {
                undo,
                allocs,
                symbols: HashMap::new(),
            },
            disabled: false,
        }
    }

    /// Walk the undo log in reverse and release every codecave the
    /// cheat allocated. Builds a fresh [`LinuxBackend`] internally
    /// so the engine that produced the outcome doesn't have to stay
    /// alive between enable and disable.
    pub fn disable(mut self) -> Result<(), ExecError> {
        if self.disabled {
            return Ok(());
        }
        let mut backend = LinuxBackend::new(self.pid);
        tatu_engine::rollback(&mut backend, &mut self.outcome);
        self.disabled = true;
        Ok(())
    }
}

impl Drop for ActiveCheat {
    fn drop(&mut self) {
        if !self.disabled && !self.outcome.undo.is_empty() {
            // Best-effort revert; we cannot return an error from Drop, but
            // a partial replay still beats leaving the trampoline live.
            let mut backend = LinuxBackend::new(self.pid);
            tatu_engine::rollback(&mut backend, &mut self.outcome);
        }
    }
}
