//! Remote allocator — `VirtualAllocEx` / `VirtualFreeEx` with a
//! near-hint scanning path. Used by the autoassembler to allocate
//! codecaves close enough to a hooked instruction that a 5-byte
//! `jmp rel32` reaches them.
//!
//! ## Why we can't just pass the hint to VirtualAllocEx
//!
//! Win32's `lpAddress` is documented as a preferred starting address.
//! Under Wine it's even softer — the kernel may completely ignore it
//! and place the allocation wherever convenient, which on x86_64
//! often lands several GiB away from the game's `.text`. A `jmp` from
//! the hooked instruction to that far codecave compiles as the
//! 14-byte `jmp [rip+disp32]` indirect form instead of `jmp rel32`,
//! overflowing the typical 10-byte slot the AA script reserved at
//! the hook site and corrupting the next instruction.
//!
//! The Linux backend gets around this with `mmap(MAP_32BIT)`, which
//! pins the mapping to the low 2 GiB and works when the game has a
//! standard low ImageBase. The equivalent for Win32 cross-process is
//! a manual descending/ascending scan in 64 KiB chunks (Win32's
//! allocation granularity) around the hint until VirtualAllocEx
//! succeeds — which is what `alloc_near` below does. Pure null hint
//! is kept as a final fallback so callers without a hint still get
//! some allocation rather than failing outright.

use std::os::raw::c_void;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE, PAGE_READWRITE, VirtualAllocEx,
    VirtualFreeEx,
};

#[derive(Debug, thiserror::Error)]
pub enum AllocError {
    #[error("VirtualAllocEx({size}, exec={executable}) returned NULL (os error {os_error})")]
    Alloc {
        size: u64,
        executable: bool,
        os_error: i32,
    },
    #[error("VirtualFreeEx({addr:#x}) returned 0 (os error {os_error})")]
    Free { addr: u64, os_error: i32 },
}

/// Win32 allocation granularity. `VirtualAllocEx` aligns reservations
/// to this — pass a non-multiple `lpAddress` and the kernel rounds
/// down anyway.
const ALLOC_GRANULARITY: u64 = 0x10000; // 64 KiB

/// Maximum scan distance from a hint when looking for a near
/// allocation. Slightly under 2 GiB so rel32 hops still fit.
const NEAR_SCAN_RANGE: u64 = 0x7FFF_0000; // ~2 GiB - 64 KiB

pub fn alloc_remote(
    process: HANDLE,
    hint: Option<u64>,
    size: u64,
    executable: bool,
) -> Result<u64, AllocError> {
    let protect = if executable {
        PAGE_EXECUTE_READWRITE
    } else {
        PAGE_READWRITE
    };
    let size_usize = size as usize;

    let try_alloc = |addr: *const c_void| -> *mut c_void {
        unsafe { VirtualAllocEx(process, addr, size_usize, MEM_COMMIT | MEM_RESERVE, protect) }
    };

    let raw = match hint {
        Some(h) => {
            // Round the hint down to allocation granularity so the
            // first probe lands on a valid VirtualAllocEx address.
            let hint_aligned = h & !(ALLOC_GRANULARITY - 1);
            let direct = try_alloc(hint_aligned as *const c_void);
            if !direct.is_null() {
                direct
            } else {
                // Hint was busy. Scan outward in 64 KiB steps,
                // alternating below and above the hint, until
                // VirtualAllocEx returns a slot. Stop short of the
                // ±2 GiB rel32 reach so a `jmp` from the hook site
                // to the resulting codecave still fits in 5 bytes.
                scan_near(try_alloc, hint_aligned).unwrap_or_else(|| try_alloc(std::ptr::null()))
            }
        }
        None => try_alloc(std::ptr::null()),
    };

    if raw.is_null() {
        return Err(AllocError::Alloc {
            size,
            executable,
            os_error: std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        });
    }
    Ok(raw as u64)
}

fn scan_near(try_alloc: impl Fn(*const c_void) -> *mut c_void, hint: u64) -> Option<*mut c_void> {
    let mut delta = ALLOC_GRANULARITY;
    while delta < NEAR_SCAN_RANGE {
        // Try below the hint first — game modules typically load
        // around 0x140000000 with most allocations climbing upward,
        // so the immediate "below" range is usually free first.
        if let Some(below) = hint.checked_sub(delta)
            && below >= ALLOC_GRANULARITY
        {
            let p = try_alloc(below as *const c_void);
            if !p.is_null() {
                return Some(p);
            }
        }
        if let Some(above) = hint.checked_add(delta) {
            let p = try_alloc(above as *const c_void);
            if !p.is_null() {
                return Some(p);
            }
        }
        delta += ALLOC_GRANULARITY;
    }
    None
}

pub fn free_remote(process: HANDLE, addr: u64) -> Result<(), AllocError> {
    // MEM_RELEASE requires the size argument to be 0 — Win32 frees the
    // entire region originally reserved.
    let ok = unsafe { VirtualFreeEx(process, addr as *mut c_void, 0, MEM_RELEASE) };
    if ok == 0 {
        return Err(AllocError::Free {
            addr,
            os_error: std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        });
    }
    Ok(())
}
