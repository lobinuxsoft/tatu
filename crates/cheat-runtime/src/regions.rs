//! Region enumeration + page protection translation.
//!
//! Ported 1:1 from `cheat-engine/Cheat Engine/ceserver/api.c`:
//! - `windowsProtectionToLinux` (line 317)
//! - `linuxProtectionToWindows` (line 334)
//! - `ProtectionStringToType` (line 3494)
//! - `ProtectionStringToProtection` (line 3502)
//! - `AddToRegionList` (line 3534) — collapsed into Rust `Vec::push`
//! - `VirtualQueryExFull` (line 3558)
//! - `VirtualQueryEx` (line 3819)
//!
//! Why this lives separately from [`crate::maps`]: `maps.rs` is a pure
//! Linux-format parser (POSIX `r/w/x/s` flags, `/proc/<pid>/maps` line
//! shape). `regions.rs` exposes the CE / Win32-style API on top
//! (`PAGE_*` protection bitmask, `MEM_PRIVATE` vs `MEM_MAPPED` type,
//! single-region "query the page containing this address" lookup) plus
//! a process-local cache so repeated queries during an AOB scan or
//! pre-write protection check don't re-parse `/proc/<pid>/maps` on
//! every call.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use nix::unistd::Pid;

use crate::maps::{MemoryRegion, Perms, read_maps};

/// Windows page protection constants. Values match `winnt.h` so on-disk
/// records and any future bridge wire format stay byte-compatible with
/// CE.
pub const PAGE_NOACCESS: u32 = 0x01;
pub const PAGE_READONLY: u32 = 0x02;
pub const PAGE_READWRITE: u32 = 0x04;
pub const PAGE_EXECUTE: u32 = 0x10;
pub const PAGE_EXECUTE_READ: u32 = 0x20;
pub const PAGE_EXECUTE_READWRITE: u32 = 0x40;

/// Windows memory type constants. Same `winnt.h` values as above.
pub const MEM_PRIVATE: u32 = 0x20000;
pub const MEM_MAPPED: u32 = 0x40000;

/// Flags for [`enumerate_regions`]. Mirror CE's `VirtualQueryExFull`
/// flag bits so a future replay against CE-format traces compares
/// directly.
pub const VQE_PAGEDONLY: u32 = 1;
pub const VQE_DIRTYONLY: u32 = 2;
pub const VQE_NOSHARED: u32 = 4;

/// One region as returned by [`query_region`] / [`enumerate_regions`].
/// Field shape mirrors CE's `RegionInfo` so callers that learnt the CE
/// API translate without a glossary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionInfo {
    pub base_address: u64,
    pub size: u64,
    /// `PAGE_*` bitmask. `PAGE_NOACCESS` for the synthetic gap entry
    /// returned by [`query_region`] when the queried address falls in
    /// an unmapped gap before the next region.
    pub protection: u32,
    /// `MEM_PRIVATE` or `MEM_MAPPED`. Zero for the synthetic gap entry.
    pub type_: u32,
}

impl RegionInfo {
    /// True iff the region's protection bits grant read access.
    pub fn is_readable(&self) -> bool {
        matches!(
            self.protection,
            PAGE_READONLY | PAGE_READWRITE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE
        )
    }

    /// True iff the region's protection bits grant write access.
    pub fn is_writable(&self) -> bool {
        matches!(self.protection, PAGE_READWRITE | PAGE_EXECUTE_READWRITE)
    }

    /// True iff the region's protection bits grant execute access.
    pub fn is_executable(&self) -> bool {
        matches!(
            self.protection,
            PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE
        )
    }
}

/// `ProtectionStringToProtection` port — Linux `Perms` → `PAGE_*`.
pub fn perms_to_windows_protection(perms: &Perms) -> u32 {
    let x = perms.execute;
    let w = perms.write;
    if x {
        if w {
            PAGE_EXECUTE_READWRITE
        } else {
            PAGE_EXECUTE_READ
        }
    } else if w {
        PAGE_READWRITE
    } else {
        // CE collapses `r--` and `---` to PAGE_READONLY (read=0 case is
        // rare on Linux — it's mostly PROT_NONE pages which CE treats
        // as NOACCESS but only inside the synthetic gap path).
        PAGE_READONLY
    }
}

/// `ProtectionStringToType` port — Linux `Perms` → `MEM_*`.
pub fn perms_to_memory_type(perms: &Perms) -> u32 {
    if perms.shared {
        MEM_MAPPED
    } else {
        MEM_PRIVATE
    }
}

/// `linuxProtectionToWindows` port — PROT_* (mprotect-style) → `PAGE_*`.
/// Used by the (future) protection-mapping module when calling
/// remote mprotect via ptrace.
pub fn linux_prot_to_windows(prot: i32) -> u32 {
    let r = prot & libc_prot::PROT_READ != 0;
    let w = prot & libc_prot::PROT_WRITE != 0;
    let x = prot & libc_prot::PROT_EXEC != 0;
    match (r, w, x) {
        (true, true, true) => PAGE_EXECUTE_READWRITE,
        (true, false, true) => PAGE_EXECUTE_READ,
        (false, false, true) => PAGE_EXECUTE,
        (true, true, false) => PAGE_READWRITE,
        (true, false, false) => PAGE_READONLY,
        _ => PAGE_NOACCESS,
    }
}

/// `windowsProtectionToLinux` port — `PAGE_*` → PROT_* (mprotect bits).
pub fn windows_protection_to_linux(p: u32) -> i32 {
    match p {
        PAGE_EXECUTE_READWRITE => {
            libc_prot::PROT_READ | libc_prot::PROT_WRITE | libc_prot::PROT_EXEC
        }
        PAGE_EXECUTE_READ => libc_prot::PROT_READ | libc_prot::PROT_EXEC,
        PAGE_EXECUTE => libc_prot::PROT_EXEC,
        PAGE_READWRITE => libc_prot::PROT_READ | libc_prot::PROT_WRITE,
        PAGE_READONLY => libc_prot::PROT_READ,
        _ => 0,
    }
}

mod libc_prot {
    pub const PROT_READ: i32 = 0x1;
    pub const PROT_WRITE: i32 = 0x2;
    pub const PROT_EXEC: i32 = 0x4;
}

/// `VirtualQueryEx` port — find the region covering `addr`. Returns
/// the synthetic gap entry (protection=PAGE_NOACCESS, type=0) when
/// `addr` falls in an unmapped hole before the next mapped region,
/// matching CE's behaviour for VirtualQuery semantics. Returns `None`
/// only when `addr` is past the last mapped region (CE returns 0).
pub fn query_region(pid: Pid, addr: u64) -> Option<RegionInfo> {
    let regions = read_maps(pid).ok()?;
    query_region_in(&regions, addr)
}

/// Pure helper for [`query_region`] — separated so callers that already
/// have a maps snapshot (e.g. inside [`enumerate_regions`]) don't pay
/// the syscall + parse cost twice.
pub fn query_region_in(regions: &[MemoryRegion], addr: u64) -> Option<RegionInfo> {
    let page_addr = addr & !0xfff;
    for r in regions {
        if r.end > page_addr {
            if page_addr >= r.start {
                return Some(RegionInfo {
                    base_address: page_addr,
                    size: r.end - page_addr,
                    protection: perms_to_windows_protection(&r.perms),
                    type_: perms_to_memory_type(&r.perms),
                });
            } else {
                // Unmapped gap before this region — CE returns a
                // NOACCESS stub describing the gap span.
                return Some(RegionInfo {
                    base_address: page_addr,
                    size: r.start - page_addr,
                    protection: PAGE_NOACCESS,
                    type_: 0,
                });
            }
        }
    }
    None
}

/// `VirtualQueryExFull` port — enumerate every region with optional
/// CE-style filters. Doesn't go through `/proc/<pid>/smaps` +
/// `/proc/<pid>/pagemap` like CE's pagedonly/dirtyonly modes do (those
/// require root or CAP_SYS_PTRACE and aren't relevant for the AOB
/// scanner path that this issue's consumers care about) — the
/// VQE_PAGEDONLY / VQE_DIRTYONLY bits are accepted but ignored with a
/// doc comment so we don't break call sites if/when we add the
/// pagemap path later.
pub fn enumerate_regions(pid: Pid, flags: u32) -> Vec<RegionInfo> {
    let regions = match read_maps(pid) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    enumerate_regions_in(&regions, flags)
}

/// Pure helper for [`enumerate_regions`].
pub fn enumerate_regions_in(regions: &[MemoryRegion], flags: u32) -> Vec<RegionInfo> {
    let no_shared = flags & VQE_NOSHARED != 0;
    regions
        .iter()
        .filter(|r| !(no_shared && r.perms.shared))
        .map(|r| RegionInfo {
            base_address: r.start,
            size: r.end - r.start,
            protection: perms_to_windows_protection(&r.perms),
            type_: perms_to_memory_type(&r.perms),
        })
        .collect()
}

/// Process-local cache so callers that repeatedly query regions
/// (typically an AOB scan that walks every readable region of a
/// process) don't re-read `/proc/<pid>/maps` on every call. CE's
/// AddToRegionList grows a per-pid heap-allocated array with the same
/// intent.
///
/// TTL-based invalidation: in practice the maps file changes rarely
/// during a scan, but games can spawn DLLs / map files dynamically.
/// 1-second default keeps the cache useful without holding stale data
/// across multi-second pauses (level load, alt-tab).
pub struct RegionCache {
    cache: Mutex<HashMap<i32, CacheEntry>>,
    ttl: Duration,
}

struct CacheEntry {
    cached_at: Instant,
    regions: Vec<RegionInfo>,
}

impl RegionCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// Returns a snapshot of the regions for `pid`. Refreshes the
    /// cache if the entry is missing or older than `ttl`.
    pub fn regions_for(&self, pid: Pid) -> Vec<RegionInfo> {
        let key = pid.as_raw();
        let mut guard = match self.cache.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(entry) = guard.get(&key)
            && entry.cached_at.elapsed() < self.ttl
        {
            return entry.regions.clone();
        }
        let fresh = enumerate_regions(pid, 0);
        guard.insert(
            key,
            CacheEntry {
                cached_at: Instant::now(),
                regions: fresh.clone(),
            },
        );
        fresh
    }

    /// Force-drop the cached entry for `pid`. Call this after the
    /// caller has done something that changes the address space
    /// (mmap_remote / mprotect_remote) and a stale snapshot would
    /// return wrong protection bits.
    pub fn invalidate(&self, pid: Pid) {
        if let Ok(mut g) = self.cache.lock() {
            g.remove(&pid.as_raw());
        }
    }

    /// Drop the entire cache. Test seam + tracker reset.
    pub fn clear(&self) {
        if let Ok(mut g) = self.cache.lock() {
            g.clear();
        }
    }
}

impl Default for RegionCache {
    fn default() -> Self {
        Self::new(Duration::from_secs(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn region(start: u64, end: u64, perms: Perms) -> MemoryRegion {
        MemoryRegion {
            start,
            end,
            perms,
            offset: 0,
            path: PathBuf::new(),
        }
    }

    fn perms(rwxs: &str) -> Perms {
        let b = rwxs.as_bytes();
        Perms {
            read: b.first() == Some(&b'r'),
            write: b.get(1) == Some(&b'w'),
            execute: b.get(2) == Some(&b'x'),
            shared: b.get(3) == Some(&b's'),
        }
    }

    #[test]
    fn perms_to_protection_covers_every_combo() {
        assert_eq!(
            perms_to_windows_protection(&perms("r-xp")),
            PAGE_EXECUTE_READ
        );
        assert_eq!(
            perms_to_windows_protection(&perms("rwxp")),
            PAGE_EXECUTE_READWRITE
        );
        assert_eq!(perms_to_windows_protection(&perms("rw-p")), PAGE_READWRITE);
        assert_eq!(perms_to_windows_protection(&perms("r--p")), PAGE_READONLY);
    }

    #[test]
    fn perms_to_type_distinguishes_shared() {
        assert_eq!(perms_to_memory_type(&perms("r--p")), MEM_PRIVATE);
        assert_eq!(perms_to_memory_type(&perms("rw-s")), MEM_MAPPED);
    }

    #[test]
    fn linux_prot_round_trips_via_windows() {
        let r = libc_prot::PROT_READ;
        let rw = libc_prot::PROT_READ | libc_prot::PROT_WRITE;
        let rx = libc_prot::PROT_READ | libc_prot::PROT_EXEC;
        let rwx = libc_prot::PROT_READ | libc_prot::PROT_WRITE | libc_prot::PROT_EXEC;
        assert_eq!(windows_protection_to_linux(linux_prot_to_windows(r)), r);
        assert_eq!(windows_protection_to_linux(linux_prot_to_windows(rw)), rw);
        assert_eq!(windows_protection_to_linux(linux_prot_to_windows(rx)), rx);
        assert_eq!(windows_protection_to_linux(linux_prot_to_windows(rwx)), rwx);
    }

    #[test]
    fn query_region_inside_mapped_range_returns_full_extent() {
        let regions = vec![
            region(0x1000, 0x3000, perms("r-xp")),
            region(0x5000, 0x6000, perms("rw-p")),
        ];
        let r = query_region_in(&regions, 0x2500).expect("inside first region");
        assert_eq!(r.base_address, 0x2000);
        assert_eq!(r.size, 0x1000);
        assert_eq!(r.protection, PAGE_EXECUTE_READ);
        assert_eq!(r.type_, MEM_PRIVATE);
    }

    #[test]
    fn query_region_in_gap_returns_noaccess_stub() {
        let regions = vec![
            region(0x1000, 0x2000, perms("r-xp")),
            region(0x5000, 0x6000, perms("rw-p")),
        ];
        let r = query_region_in(&regions, 0x3000).expect("in the gap");
        assert_eq!(r.base_address, 0x3000);
        assert_eq!(r.size, 0x2000, "gap span ends at the next region start");
        assert_eq!(r.protection, PAGE_NOACCESS);
        assert_eq!(r.type_, 0);
    }

    #[test]
    fn query_region_past_last_returns_none() {
        let regions = vec![region(0x1000, 0x2000, perms("r--p"))];
        assert!(query_region_in(&regions, 0x5000).is_none());
    }

    #[test]
    fn enumerate_with_no_shared_skips_shared_mappings() {
        let regions = vec![
            region(0x1000, 0x2000, perms("rw-p")),
            region(0x2000, 0x3000, perms("r--s")),
            region(0x3000, 0x4000, perms("r-xp")),
        ];
        let out = enumerate_regions_in(&regions, VQE_NOSHARED);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|r| r.protection != PAGE_NOACCESS));
        assert_eq!(out[0].base_address, 0x1000);
        assert_eq!(out[1].base_address, 0x3000);
    }

    #[test]
    fn enumerate_without_flags_returns_every_region() {
        let regions = vec![
            region(0x1000, 0x2000, perms("rw-p")),
            region(0x2000, 0x3000, perms("r--s")),
        ];
        let out = enumerate_regions_in(&regions, 0);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn is_readable_writable_executable_flags_consistent() {
        let exec = RegionInfo {
            base_address: 0,
            size: 0x1000,
            protection: PAGE_EXECUTE_READ,
            type_: MEM_PRIVATE,
        };
        assert!(exec.is_readable());
        assert!(!exec.is_writable());
        assert!(exec.is_executable());

        let rw = RegionInfo {
            base_address: 0,
            size: 0x1000,
            protection: PAGE_READWRITE,
            type_: MEM_PRIVATE,
        };
        assert!(rw.is_readable());
        assert!(rw.is_writable());
        assert!(!rw.is_executable());

        let noaccess = RegionInfo {
            base_address: 0,
            size: 0x1000,
            protection: PAGE_NOACCESS,
            type_: 0,
        };
        assert!(!noaccess.is_readable());
        assert!(!noaccess.is_writable());
        assert!(!noaccess.is_executable());
    }

    #[test]
    fn cache_returns_same_snapshot_within_ttl() {
        let cache = RegionCache::new(Duration::from_secs(60));
        let pid = Pid::this();
        let first = cache.regions_for(pid);
        let second = cache.regions_for(pid);
        assert_eq!(first, second);
        assert!(!first.is_empty(), "/proc/self/maps must have entries");
    }

    #[test]
    fn cache_invalidate_drops_entry() {
        let cache = RegionCache::new(Duration::from_secs(60));
        let pid = Pid::this();
        let _ = cache.regions_for(pid);
        cache.invalidate(pid);
        // Next call repopulates; just confirm it doesn't panic + returns
        // something non-empty.
        assert!(!cache.regions_for(pid).is_empty());
    }
}
