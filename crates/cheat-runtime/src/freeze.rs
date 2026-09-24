//! Continuous re-apply of a typed value at a fixed address ("freeze").
//!
//! Cheat Engine's *freeze* checkbox keeps a value pinned by re-writing it on a
//! short timer. The new runtime's [`crate::Engine`] is one-shot
//! (enable → undo log → disable), so it does not by itself fight a game that
//! overwrites the cheat target every frame. [`FreezeRegistry`] fills that gap:
//! given a [`FreezeTarget`] and an interval, it owns a worker thread that
//! loops the write until the loop is cancelled, the target exits, or the
//! registry is dropped.
//!
//! Two backends:
//! - [`FreezeTarget::Linux`] writes via `process_vm_writev` (the same path the
//!   Linux [`crate::Engine`] uses for its toggles).
//! - [`FreezeTarget::Bridge`] dials `tatu-bridge --connect` over its TCP port
//!   inside the wineprefix and sends a CHRT `WriteChainValue` per tick.
//!   Connect-write-disconnect every tick keeps the bridge's single-client
//!   serve loop free between ticks for other tracker traffic.
//!
//! Keys are arbitrary owned strings so callers can decide their own naming
//! scheme (manifest feature UUID, legacy `(app_id, cheat_id)`, ad-hoc test
//! tag). Calling [`FreezeRegistry::start`] with a key already in flight is a
//! no-op that returns `Ok(false)` — the caller is treated as a duplicate
//! click, not an error.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use nix::unistd::Pid;

use crate::memory::write_bytes;

pub const DEFAULT_INTERVAL_MS: u64 = 16;

pub type FreezeKey = String;

#[derive(Debug, thiserror::Error)]
pub enum FreezeError {
    #[error("freeze registry mutex poisoned")]
    Poisoned,
}

/// What the freeze worker writes on every tick. Linux ptrace only
/// post-pivot #128 — the wine-side Bridge variant was dropped along
/// with the bridge architecture.
pub enum FreezeTarget {
    /// Native Linux ptrace runtime. Loops `process_vm_writev` against
    /// `pid` at `address` with `bytes`. Direct, no protocol overhead.
    Linux {
        pid: Pid,
        address: u64,
        bytes: Vec<u8>,
    },
}

pub struct FreezeHandle {
    cancel: Arc<AtomicBool>,
    // Owned so the worker can be joined on Drop if a caller ever wants
    // synchronous shutdown — the public API does not block on join; the OS
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

    /// Spawn a freeze loop. Returns `Ok(true)` if a new loop was started,
    /// `Ok(false)` if one was already active under this key (duplicate click).
    pub fn start(
        &self,
        key: impl Into<FreezeKey>,
        target: FreezeTarget,
        interval_ms: Option<u64>,
    ) -> Result<bool, FreezeError> {
        let key = key.into();
        let mut active = self.active.lock().map_err(|_| FreezeError::Poisoned)?;
        if active.contains_key(&key) {
            return Ok(false);
        }

        let interval = Duration::from_millis(interval_ms.unwrap_or(DEFAULT_INTERVAL_MS));
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_worker = Arc::clone(&cancel);
        let join = std::thread::spawn(move || run_freeze(target, cancel_worker, interval));

        active.insert(
            key,
            FreezeHandle {
                cancel,
                _join: join,
            },
        );
        Ok(true)
    }

    /// Signal cancel and remove the entry. Returns `true` if a loop was
    /// active, `false` otherwise. Does not block on the worker exiting.
    pub fn stop(&self, key: &str) -> Result<bool, FreezeError> {
        let mut active = self.active.lock().map_err(|_| FreezeError::Poisoned)?;
        let Some(handle) = active.remove(key) else {
            return Ok(false);
        };
        handle.cancel.store(true, Ordering::Relaxed);
        Ok(true)
    }

    pub fn is_active(&self, key: &str) -> Result<bool, FreezeError> {
        Ok(self
            .active
            .lock()
            .map_err(|_| FreezeError::Poisoned)?
            .contains_key(key))
    }

    pub fn active_keys(&self) -> Result<Vec<FreezeKey>, FreezeError> {
        Ok(self
            .active
            .lock()
            .map_err(|_| FreezeError::Poisoned)?
            .keys()
            .cloned()
            .collect())
    }
}

impl Drop for FreezeRegistry {
    fn drop(&mut self) {
        // Best-effort cancel on shutdown. Mutex poisoning here is rare and
        // not actionable from Drop, so we silently swallow it.
        if let Ok(active) = self.active.lock() {
            for handle in active.values() {
                handle.cancel.store(true, Ordering::Relaxed);
            }
        }
    }
}

fn run_freeze(target: FreezeTarget, cancel: Arc<AtomicBool>, interval: Duration) {
    let FreezeTarget::Linux {
        pid,
        address,
        bytes,
    } = target;
    while !cancel.load(Ordering::Relaxed) {
        if write_bytes(pid, address, &bytes).is_err() {
            // Target gone or address unmapped — exit silently. The
            // stale registry entry survives until stop() is called.
            break;
        }
        std::thread::sleep(interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_on_unknown_key_returns_false() {
        let registry = FreezeRegistry::new();
        assert!(!registry.stop("missing").unwrap());
        assert!(!registry.is_active("missing").unwrap());
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn start_then_stop_freezes_then_releases_value() {
        use std::sync::atomic::AtomicU32;
        let target = Box::new(AtomicU32::new(0));
        let address = target.as_ref() as *const AtomicU32 as u64;
        let bytes = 0xCAFE_BABE_u32.to_le_bytes().to_vec();

        let registry = FreezeRegistry::new();
        assert!(
            registry
                .start(
                    "k",
                    FreezeTarget::Linux {
                        pid: Pid::this(),
                        address,
                        bytes,
                    },
                    Some(5),
                )
                .unwrap()
        );
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(target.load(Ordering::Relaxed), 0xCAFE_BABE);

        assert!(registry.stop("k").unwrap());
        std::thread::sleep(Duration::from_millis(20));

        target.store(0xDEAD_BEEF, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(target.load(Ordering::Relaxed), 0xDEAD_BEEF);
    }

    #[test]
    fn drop_signals_cancel_to_all_handles() {
        use std::sync::atomic::AtomicU32;
        // Leak the target so the detached worker thread cannot outlive the
        // backing memory if `process_vm_writev` is permitted on self in this
        // env. Without the leak, the worker may write into freed heap after
        // the test scope drops, segfaulting the test binary on exit.
        let target: &'static AtomicU32 = Box::leak(Box::new(AtomicU32::new(0)));
        let address = target as *const AtomicU32 as u64;
        let bytes = vec![0u8; 4];

        let registry = FreezeRegistry::new();
        if registry
            .start(
                "k",
                FreezeTarget::Linux {
                    pid: Pid::this(),
                    address,
                    bytes,
                },
                Some(10),
            )
            .is_err()
            || !registry.is_active("k").unwrap()
        {
            drop(registry);
            return;
        }

        let cancel_snapshot = {
            let active = registry.active.lock().unwrap();
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
