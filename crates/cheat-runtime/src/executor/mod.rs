//! Linux runtime wrapper around the generic [`tatu_engine::Engine`].
//!
//! The two-pass executor itself lives in `tatu-engine` and is generic
//! over any [`tatu_engine::Backend`]. This module wires that generic
//! engine to the [`LinuxBackend`](crate::linux_backend::LinuxBackend)
//! adapter so existing call sites (`Engine::new(pid)`,
//! `engine.enable(&script)` returning [`ActiveCheat`]) keep their
//! shape after PR 7A2 of #106.
//!
//! New code that needs cross-backend reuse should call
//! `tatu_engine::Engine::<B>::new(backend)` directly and consume
//! [`tatu_engine::EnableOutcome`] instead of [`ActiveCheat`] — the
//! bridge backend in PR 7B does exactly that.
//!
//! ## Submodule layout
//!
//! - [`active`] — [`ActiveCheat`], the Linux-side wrapper that owns
//!   the undo log + ptrace-backed rollback.
//! - [`tests`] — integration tests against `Pid::this()` + forked
//!   children.

mod active;

pub use active::ActiveCheat;
pub use tatu_engine::ExecError;

use nix::unistd::Pid;
use std::collections::HashMap;

use crate::linux_backend::LinuxBackend;
use crate::parser::Script;

/// Linux-facing executor: a thin wrapper around
/// [`tatu_engine::Engine`] specialised to [`LinuxBackend`].
/// Preserves the historical surface (`Engine::new(pid)`,
/// `engine.enable(script) -> ActiveCheat`).
pub struct Engine {
    inner: tatu_engine::Engine<LinuxBackend>,
}

impl Engine {
    pub fn new(pid: Pid) -> Self {
        Self {
            inner: tatu_engine::Engine::new(LinuxBackend::new(pid)),
        }
    }

    pub fn pid(&self) -> Pid {
        self.inner.backend().pid()
    }

    pub fn symbols(&self) -> &HashMap<String, u64> {
        self.inner.symbols()
    }

    pub fn bind_symbol(&mut self, name: impl Into<String>, addr: u64) {
        self.inner.bind_symbol(name, addr);
    }

    /// Drive an enable cycle and wrap the resulting
    /// [`tatu_engine::EnableOutcome`] in an [`ActiveCheat`] keyed on
    /// the original PID for backwards compatibility.
    pub fn enable(&mut self, script: &Script) -> Result<ActiveCheat, ExecError> {
        let outcome = self.inner.enable(script)?;
        Ok(ActiveCheat::from_outcome(self.pid(), outcome))
    }
}

#[cfg(test)]
mod tests;
