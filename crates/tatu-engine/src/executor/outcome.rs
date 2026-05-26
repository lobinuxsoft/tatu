//! [`EnableOutcome`] — the POD result of a successful
//! [`crate::Engine::enable`].
//!
//! Backend-agnostic. The Linux runtime in `cheat-runtime` wraps this
//! into its `ActiveCheat` (which owns ptrace-flavoured rollback);
//! the bridge serialises it across the CHRT wire as part of
//! `Response::EnableScript` in PR 7B so the tracker can build its
//! own `PersistedHook` from the same payload.

use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct EnableOutcome {
    /// Pairs of `(remote_addr, original_bytes)` in apply order — the
    /// undo log every rollback path walks in reverse.
    pub undo: Vec<(u64, Vec<u8>)>,
    /// Symbol → `(remote_addr, size)` for every `Statement::Alloc`
    /// that succeeded during pass 1.
    pub allocs: HashMap<String, (u64, usize)>,
    /// Snapshot of every symbol bound during enable — AOB scans,
    /// allocs, explicit `registersymbol` directives, label sites.
    /// The tracker's value-cheat reads consult this to resolve
    /// `[symbol]` against the running game.
    pub symbols: HashMap<String, u64>,
}
