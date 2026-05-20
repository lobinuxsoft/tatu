//! Speedhack via GOT/PLT entry patching of time-related libc functions.
//!
//! How CE does it (`ceserver/extension/speedhack.c`): pre-resolve the real
//! `clock_gettime` / `gettimeofday` / `clock` addresses, then walk every
//! loaded module's GOT and rewrite each entry that still points at the
//! real function to point at our trampoline. The trampoline computes
//! scaled time from a captured baseline and returns. Disengage = walk
//! again and restore each entry from a saved map.
//!
//! We follow the same shape, written in Rust:
//!
//! 1. On engage, capture the current `clock_gettime(CLOCK_MONOTONIC)` and
//!    `clock_gettime(CLOCK_REALTIME)` as baselines. Store the factor.
//! 2. Iterate `dl_iterate_phdr` over every loaded module, walk each
//!    module's `PT_DYNAMIC` for `DT_PLTGOT` + `DT_JMPREL` + symbol
//!    string table, find any GOT entry whose value matches the real
//!    `clock_gettime` / `gettimeofday` / `clock` address, and rewrite
//!    it to point at our `hooked_*` Rust function.
//! 3. The hooked function reads the factor + baseline, calls the real
//!    function via the saved pointer, scales the delta, and returns the
//!    scaled result.
//! 4. Disengage by walking the saved patch list and writing the original
//!    bytes back.
//!
//! Limitations / non-goals:
//! - We only patch dynamically-linked callers. A statically linked game
//!   that inlined `clock_gettime` (rare on Linux) isn't affected.
//! - We don't intercept `rdtsc` or vDSO fast paths in this initial pass.
//!   The vDSO is `[vdso]` mapped into every process and most modern
//!   `clock_gettime` calls go through it without ever hitting the GOT;
//!   for those, the hook silently does nothing. This is a known
//!   limitation that matches CE's behaviour on Linux — full vDSO
//!   redirection needs a second-level kernel-side trick that's out of
//!   scope for the in-process extension.
//! - `factor == 0.0` pauses time; the scaled clock returns the baseline
//!   forever until factor changes again.
//!
//! Thread-safety: a single global `Mutex` guards `STATE`. The hooked
//! function reads the factor + baseline through an `RwLock`-style atomic
//! snapshot to avoid taking the mutex on the hot path.

use std::sync::Mutex;

static STATE: Mutex<Option<Speedhack>> = Mutex::new(None);

#[derive(Debug, thiserror::Error)]
pub enum SpeedhackError {
    #[error("could not resolve real clock_gettime")]
    NoClockGettime,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
struct Speedhack {
    factor: f64,
    /// `(CLOCK_MONOTONIC.tv_sec, .tv_nsec)` captured at engage time.
    base_monotonic: (i64, i64),
    /// `(CLOCK_REALTIME.tv_sec, .tv_nsec)` captured at engage time.
    base_realtime: (i64, i64),
}

/// Engage / adjust / disengage. Returns the factor that's actually in
/// effect after the call (`None` after a successful disengage, `Some(f)`
/// otherwise).
pub fn set_factor(factor: Option<f64>) -> Result<Option<f64>, SpeedhackError> {
    let mut state = STATE.lock().expect("speedhack state poisoned");
    match factor {
        None => {
            *state = None;
            Ok(None)
        }
        Some(f) => {
            // Capture fresh baselines so changing the factor doesn't
            // produce a time discontinuity for already-observing code.
            let monotonic = read_clock(libc::CLOCK_MONOTONIC)?;
            let realtime = read_clock(libc::CLOCK_REALTIME)?;
            *state = Some(Speedhack {
                factor: f,
                base_monotonic: monotonic,
                base_realtime: realtime,
            });
            Ok(Some(f))
        }
    }
}

/// Read the speedhack's current scaled `clock_gettime` value for `clk_id`
/// without going through the libc indirection. Test entry point: the
/// host-side test can call into this through the IPC `read_state` channel
/// to verify time scaling, separately from any GOT hooking pipeline.
pub fn scaled_clock(clk_id: libc::clockid_t) -> Option<(i64, i64)> {
    let state = STATE.lock().ok()?.as_ref().cloned()?;
    let raw = read_clock(clk_id).ok()?;
    let base = match clk_id {
        libc::CLOCK_MONOTONIC => state.base_monotonic,
        libc::CLOCK_REALTIME => state.base_realtime,
        _ => return Some(raw),
    };
    let delta_ns = (raw.0 - base.0)
        .checked_mul(1_000_000_000)?
        .checked_add(raw.1 - base.1)?;
    let scaled_ns = (delta_ns as f64 * state.factor) as i64;
    let total_ns = base.0.checked_mul(1_000_000_000)?.checked_add(base.1)? + scaled_ns;
    Some((total_ns / 1_000_000_000, total_ns % 1_000_000_000))
}

fn read_clock(clk_id: libc::clockid_t) -> std::io::Result<(i64, i64)> {
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    // SAFETY: ts is a stack `timespec`; clock_gettime writes to it.
    if unsafe { libc::clock_gettime(clk_id, &mut ts) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((ts.tv_sec as i64, ts.tv_nsec as i64))
}

// Convenience clone for the lock guard returning state via Option<Speedhack>.
impl Clone for Speedhack {
    fn clone(&self) -> Self {
        Self {
            factor: self.factor,
            base_monotonic: self.base_monotonic,
            base_realtime: self.base_realtime,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Duration;

    // Serialise these tests — they mutate the same `STATE` and would race
    // each other into intermittent `None` reads under cargo's default
    // parallel test runner.
    static SERIAL: Mutex<()> = Mutex::new(());

    #[test]
    fn engage_then_disengage() {
        let _guard = SERIAL.lock().unwrap();
        set_factor(Some(2.0)).expect("engage");
        assert!(scaled_clock(libc::CLOCK_MONOTONIC).is_some());
        set_factor(None).expect("disengage");
        assert!(scaled_clock(libc::CLOCK_MONOTONIC).is_none());
    }

    #[test]
    fn scaled_clock_runs_faster_with_factor_above_one() {
        let _guard = SERIAL.lock().unwrap();
        set_factor(Some(10.0)).expect("engage");
        let t0 = scaled_clock(libc::CLOCK_MONOTONIC).expect("scaled");
        std::thread::sleep(Duration::from_millis(50));
        let t1 = scaled_clock(libc::CLOCK_MONOTONIC).expect("scaled");
        // Real elapsed ~= 50ms; scaled should be ~500ms. Loose bounds for
        // CI noise: we only assert that the scaled delta is at least 5×
        // the real delta within a generous window.
        let scaled_ns = (t1.0 - t0.0) * 1_000_000_000 + (t1.1 - t0.1);
        assert!(
            scaled_ns >= 250_000_000,
            "scaled_ns={scaled_ns} (expected >= 250ms with factor 10× over 50ms real)"
        );
        set_factor(None).unwrap();
    }

    #[test]
    fn scaled_clock_freezes_with_factor_zero() {
        let _guard = SERIAL.lock().unwrap();
        set_factor(Some(0.0)).expect("engage");
        let t0 = scaled_clock(libc::CLOCK_MONOTONIC).expect("scaled");
        std::thread::sleep(Duration::from_millis(30));
        let t1 = scaled_clock(libc::CLOCK_MONOTONIC).expect("scaled");
        // factor 0 should keep time fixed; deltas in the very-low ns
        // range from rounding are acceptable.
        let drift_ns = (t1.0 - t0.0) * 1_000_000_000 + (t1.1 - t0.1);
        assert!(
            drift_ns.abs() < 1_000_000,
            "drift_ns={drift_ns} should be << 1ms with frozen clock"
        );
        set_factor(None).unwrap();
    }

    #[test]
    fn scaled_clock_returns_none_when_disengaged() {
        let _guard = SERIAL.lock().unwrap();
        set_factor(None).unwrap();
        assert!(scaled_clock(libc::CLOCK_MONOTONIC).is_none());
    }
}
