//! AOB scanner — Linux ptrace front-end on top of [`tatu_mem::pattern`].
//!
//! The pattern parser + masked-byte scan kernel live in `tatu-mem` so
//! the Win32 bridge can share them; this module re-exports them and
//! adds the only Linux-specific piece: [`scan_in_process`], which
//! drives [`tatu_mem::pattern::scan_range`] over a [`MemoryRegion`]
//! using [`crate::memory_access::ProcessVmMem`].

use nix::unistd::Pid;
pub use tatu_mem::pattern::{Pattern, ParseError, SCAN_CHUNK_SIZE};

use crate::maps::MemoryRegion;
use crate::memory::RuntimeError;
use crate::memory_access::ProcessVmMem;

/// Backwards-compatible function form of [`Pattern::scan`]: kept so
/// existing call sites (`cheat_runtime::scan(haystack, &pattern)`)
/// keep compiling after the dedup.
pub fn scan(haystack: &[u8], pattern: &Pattern) -> Vec<usize> {
    pattern.scan(haystack)
}

/// Scan a live process region for `pattern`, returning absolute
/// addresses where the pattern matches. Reads via `process_vm_readv`
/// in 4 MiB chunks (with `pattern.len() - 1` byte overlap so hits
/// straddling chunk boundaries are still matched without
/// double-counting).
///
/// The `Result` wrapper is kept for source compatibility; the
/// `tatu-mem` scan engine treats short-reads / unmapped sub-pages as
/// "no match here" rather than fatal errors (the CE behaviour), so
/// the `Err` arm is never produced today. Returning `Result` lets us
/// surface backend errors later without touching call sites.
pub fn scan_in_process(
    pid: Pid,
    region: &MemoryRegion,
    pattern: &Pattern,
) -> Result<Vec<u64>, RuntimeError> {
    let mut mem = ProcessVmMem::new(pid);
    Ok(tatu_mem::pattern::scan_range(
        &mut mem,
        region.start,
        region.size(),
        pattern,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maps::Perms;
    use std::path::PathBuf;
    use std::time::Instant;

    fn region_over(buf: &[u8]) -> MemoryRegion {
        MemoryRegion {
            start: buf.as_ptr() as u64,
            end: buf.as_ptr() as u64 + buf.len() as u64,
            perms: Perms {
                read: true,
                write: true,
                execute: false,
                shared: false,
            },
            offset: 0,
            path: PathBuf::new(),
        }
    }

    #[test]
    fn scan_in_process_against_own_buffer() {
        let mut buf = vec![0u8; 1024];
        buf[100..103].copy_from_slice(&[0xde, 0xad, 0xbe]);
        buf[500..503].copy_from_slice(&[0xde, 0xff, 0xbe]);
        let region = region_over(&buf);
        let p = Pattern::parse("DE ?? BE").unwrap();

        let hits = scan_in_process(Pid::this(), &region, &p).unwrap();
        let base = buf.as_ptr() as u64;
        assert_eq!(hits, vec![base + 100, base + 500]);
    }

    #[test]
    fn scan_in_process_finds_pattern_across_chunk_boundary() {
        // 9 MB buffer with the needle straddling the 4 MiB chunk
        // boundary — exercises the overlap logic in
        // tatu_mem::pattern::scan_range.
        let mut buf = vec![0u8; 9 * 1024 * 1024];
        let needle = b"\x11\x22\x33\x44\x55\x66\x77\x88";
        let split = SCAN_CHUNK_SIZE - 3;
        buf[split..split + needle.len()].copy_from_slice(needle);
        let region = region_over(&buf);
        let p = Pattern::parse("11 22 33 44 55 66 77 88").unwrap();

        let hits = scan_in_process(Pid::this(), &region, &p).unwrap();
        assert_eq!(hits.len(), 1, "expected exactly one match across boundary");
        assert_eq!(hits[0], buf.as_ptr() as u64 + split as u64);
    }

    #[test]
    fn scan_100mb_under_500ms() {
        // Performance regression guard against the in-memory
        // (non-syscall) hot path. The remote-IO variant is exercised
        // by the two tests above; this one validates that swapping
        // the algorithm engine to tatu-mem didn't tank the kernel.
        let mut haystack = vec![0u8; 100 * 1024 * 1024];
        let where_at = 50 * 1024 * 1024;
        haystack[where_at..where_at + 6].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let p = Pattern::parse("AA BB ?? DD EE FF").unwrap();

        let started = Instant::now();
        let hits = scan(&haystack, &p);
        let elapsed = started.elapsed();
        eprintln!("scan 100 MiB ({:?}): {} hits", elapsed, hits.len());

        assert_eq!(hits, vec![where_at]);
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "scan was too slow: {elapsed:?}"
        );
    }
}
