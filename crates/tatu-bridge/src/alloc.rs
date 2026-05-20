//! Remote allocator — thin wrapper around `VirtualAllocEx` /
//! `VirtualFreeEx` with a near-hint fast path. Used by the autoassembler
//! to allocate codecaves close enough to a hooked instruction that a
//! 5-byte `jmp rel32` reaches them.
//!
//! Win32 honours `lpAddress` hints far more reliably than Linux's
//! `mmap` (which only takes them as advisory). When the hinted address
//! is unavailable we fall back to `null` so the kernel picks any free
//! region — better than failing the whole request.

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
    let size = size as usize;

    let try_alloc = |addr: *const c_void| unsafe {
        VirtualAllocEx(process, addr, size, MEM_COMMIT | MEM_RESERVE, protect)
    };

    let raw = match hint {
        Some(h) => {
            let hinted = try_alloc(h as *const c_void);
            if hinted.is_null() {
                // Fall back to any-address allocation; the caller asked
                // for "near" h but a definite allocation beats nothing.
                try_alloc(std::ptr::null())
            } else {
                hinted
            }
        }
        None => try_alloc(std::ptr::null()),
    };

    if raw.is_null() {
        return Err(AllocError::Alloc {
            size: size as u64,
            executable,
            os_error: std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        });
    }
    Ok(raw as u64)
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
