//! Thin safe-ish wrapper around `ReadProcessMemory` /
//! `WriteProcessMemory`. The bridge owns a `HANDLE` to the target
//! process opened with `PROCESS_VM_OPERATION | PROCESS_VM_READ |
//! PROCESS_VM_WRITE`; every Phase 4 primitive (AOB scanner, code
//! patcher, codecave allocator, pointer-chain walker) goes through
//! these helpers so the unsafe surface stays in one file.
//!
//! Linux mirror: `cheat_runtime::memory::{read_bytes, write_bytes,
//! read_bytes_partial}` over `process_vm_readv` / `process_vm_writev`.

use std::os::raw::c_void;

use windows_sys::Win32::Foundation::HANDLE;

use super::win::{ReadProcessMemory, WriteProcessMemory};

#[derive(Debug, thiserror::Error)]
pub enum RemoteMemError {
    #[error("ReadProcessMemory({addr:#x}, {len}) returned 0 (last os error: {os_error})")]
    Read {
        addr: u64,
        len: usize,
        os_error: i32,
    },
    #[error("WriteProcessMemory({addr:#x}, {len}) returned 0 (last os error: {os_error})")]
    Write {
        addr: u64,
        len: usize,
        os_error: i32,
    },
    #[error("short read at {addr:#x}: requested {requested}, got {got}")]
    ShortRead {
        addr: u64,
        requested: usize,
        got: usize,
    },
    #[error("short write at {addr:#x}: requested {requested}, wrote {wrote}")]
    ShortWrite {
        addr: u64,
        requested: usize,
        wrote: usize,
    },
}

/// Read exactly `len` bytes from `addr` in the remote process. Short
/// reads are an error — when the bridge needs a permissive variant for
/// region-scanning, use [`read_remote_partial`] instead.
pub fn read_remote(process: HANDLE, addr: u64, len: usize) -> Result<Vec<u8>, RemoteMemError> {
    if len == 0 {
        return Ok(Vec::new());
    }
    let mut buf = vec![0u8; len];
    let mut got = 0usize;
    let ok = unsafe {
        ReadProcessMemory(
            process,
            addr as *const c_void,
            buf.as_mut_ptr() as *mut c_void,
            len,
            &mut got,
        )
    };
    if ok == 0 {
        return Err(RemoteMemError::Read {
            addr,
            len,
            os_error: std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        });
    }
    if got != len {
        return Err(RemoteMemError::ShortRead {
            addr,
            requested: len,
            got,
        });
    }
    Ok(buf)
}

/// Permissive read: truncates to whatever the kernel transferred.
/// Returns `Ok(empty)` if the call fails OR returns 0 bytes (caller
/// distinguishes via the returned length). Useful when scanning across
/// regions of possibly-unmapped memory.
pub fn read_remote_partial(process: HANDLE, addr: u64, len: usize) -> Vec<u8> {
    if len == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u8; len];
    let mut got = 0usize;
    let ok = unsafe {
        ReadProcessMemory(
            process,
            addr as *const c_void,
            buf.as_mut_ptr() as *mut c_void,
            len,
            &mut got,
        )
    };
    if ok == 0 {
        return Vec::new();
    }
    buf.truncate(got);
    buf
}

pub fn write_remote(process: HANDLE, addr: u64, bytes: &[u8]) -> Result<(), RemoteMemError> {
    if bytes.is_empty() {
        return Ok(());
    }
    let mut wrote = 0usize;
    let ok = unsafe {
        WriteProcessMemory(
            process,
            addr as *mut c_void,
            bytes.as_ptr() as *const c_void,
            bytes.len(),
            &mut wrote,
        )
    };
    if ok == 0 {
        return Err(RemoteMemError::Write {
            addr,
            len: bytes.len(),
            os_error: std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        });
    }
    if wrote != bytes.len() {
        return Err(RemoteMemError::ShortWrite {
            addr,
            requested: bytes.len(),
            wrote,
        });
    }
    Ok(())
}

/// Read 8 bytes at `addr` and decode as little-endian `u64`.
/// Helper for pointer-chain walking.
pub fn read_u64(process: HANDLE, addr: u64) -> Result<u64, RemoteMemError> {
    let bytes = read_remote(process, addr, 8)?;
    Ok(u64::from_le_bytes(bytes.as_slice().try_into().unwrap()))
}
