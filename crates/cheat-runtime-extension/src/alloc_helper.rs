//! In-process allocation helper. Wraps the target's libc `malloc`/`free`
//! through a HashMap so the host can refer to allocations by address
//! without leaking the bookkeeping inside the cheat script.
//!
//! Why a HashMap instead of trusting the host to remember sizes: `free()`
//! only takes an address; we keep size around so future operations
//! (resize, dump, etc.) can be added without breaking the wire protocol.

use std::collections::HashMap;
use std::sync::Mutex;

static ALLOCS: Mutex<Option<HashMap<u64, usize>>> = Mutex::new(None);

fn map() -> std::sync::MutexGuard<'static, Option<HashMap<u64, usize>>> {
    let mut g = ALLOCS.lock().expect("alloc map poisoned");
    if g.is_none() {
        *g = Some(HashMap::new());
    }
    g
}

/// Allocate `size` bytes via libc `malloc`. Returns the absolute address
/// or `None` if `malloc` returned NULL. The allocation is tracked so
/// [`free`] can release it without the host having to send the size.
pub fn alloc(size: usize) -> Option<u64> {
    if size == 0 {
        return None;
    }
    // SAFETY: libc::malloc is async-signal-safe enough for our background
    // thread; we never call this from a signal handler.
    let ptr = unsafe { libc::malloc(size) };
    if ptr.is_null() {
        return None;
    }
    let addr = ptr as u64;
    if let Some(m) = map().as_mut() {
        m.insert(addr, size);
    }
    Some(addr)
}

/// Free a previously allocated address. No-op if `addr` is not tracked
/// (defensive: a bug in the host shouldn't crash the target).
pub fn free(addr: u64) {
    let was_tracked = {
        let mut g = map();
        let m = g.as_mut().expect("alloc map");
        m.remove(&addr).is_some()
    };
    if was_tracked {
        // SAFETY: address came from our prior malloc; tracked exactly once.
        unsafe { libc::free(addr as *mut libc::c_void) };
    }
}

/// Count currently-live allocations. Test helper, also useful for
/// debugging leaks during development.
pub fn live_count() -> usize {
    map().as_ref().map(|m| m.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_free_round_trip() {
        // Run in a serial-ish way so the global state doesn't get polluted
        // by parallel tests in this module.
        let pre = live_count();
        let addr = alloc(128).expect("alloc");
        assert!(addr != 0);
        assert_eq!(live_count(), pre + 1);
        free(addr);
        assert_eq!(live_count(), pre);
    }

    #[test]
    fn alloc_zero_returns_none() {
        assert!(alloc(0).is_none());
    }

    #[test]
    fn free_unknown_address_is_silent() {
        // Pretend the host sent garbage. Must not crash.
        free(0xdead_beef);
    }
}
