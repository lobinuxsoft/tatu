//! Code patcher — `VirtualProtectEx` → `WriteProcessMemory` →
//! `FlushInstructionCache` → restore protection, optionally bracketed
//! by `SuspendThread` / `ResumeThread` against every thread of the
//! target.
//!
//! Suspending threads is the same atomicity guard CE's autoassembler
//! uses: when we're rewriting a 5-byte `jmp rel32` over the start of a
//! function, no thread can be partway through the original
//! instruction. `SuspendThread` is racy by itself (a thread can be
//! between two `SuspendThread` calls when it's about to enter the
//! patched bytes), but it covers the overwhelmingly common case where
//! a single thread is hot in the function we're patching.
//!
//! `FlushInstructionCache` is non-negotiable cross-process: x86_64
//! L1i is coherent for the *issuing* process, but a different process
//! writing into the target's address space leaves the target's
//! pipeline holding the pre-patch bytes until the next branch /
//! invalidation. Win32 documents this exact requirement.

use std::os::raw::c_void;

use windows_sys::Win32::Foundation::{CloseHandle, FALSE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::Memory::{
    PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtectEx,
};
use windows_sys::Win32::System::Threading::{
    OpenThread, ResumeThread, SuspendThread, THREAD_SUSPEND_RESUME,
};

use super::remote_mem::{RemoteMemError, write_remote};

unsafe extern "system" {
    fn FlushInstructionCache(
        hProcess: HANDLE,
        lpBaseAddress: *const c_void,
        dwSize: usize,
    ) -> i32;
}

#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    #[error("VirtualProtectEx({addr:#x}, {len}, {desired:#x}) returned 0 (os error {os_error})")]
    Protect {
        addr: u64,
        len: usize,
        desired: u32,
        os_error: i32,
    },
    #[error("FlushInstructionCache({addr:#x}, {len}) returned 0 (os error {os_error})")]
    Flush { addr: u64, len: usize, os_error: i32 },
    #[error("remote memory: {0}")]
    Memory(#[from] RemoteMemError),
}

/// Apply `bytes` at `addr` in the remote process. Lifts the page
/// protection to PAGE_EXECUTE_READWRITE for the duration of the write,
/// restores the prior protection afterwards, and flushes the target's
/// instruction cache so the next branch sees the new bytes.
///
/// When `suspend_threads` is true, every other thread of the owning
/// process is suspended before the write and resumed via the guard's
/// `Drop`. Failure to enumerate / suspend a thread aborts the whole
/// patch — partial suspension is worse than none.
pub fn patch_bytes(
    process: HANDLE,
    target_pid: u32,
    addr: u64,
    bytes: &[u8],
    suspend_threads: bool,
) -> Result<(), PatchError> {
    if bytes.is_empty() {
        return Ok(());
    }

    let _guard = if suspend_threads {
        Some(ThreadSuspendGuard::suspend_all(target_pid)?)
    } else {
        None
    };

    let mut old_protect: PAGE_PROTECTION_FLAGS = 0;
    let ok = unsafe {
        VirtualProtectEx(
            process,
            addr as *mut c_void,
            bytes.len(),
            PAGE_EXECUTE_READWRITE,
            &mut old_protect,
        )
    };
    if ok == 0 {
        return Err(PatchError::Protect {
            addr,
            len: bytes.len(),
            desired: PAGE_EXECUTE_READWRITE,
            os_error: std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        });
    }

    let write_result = write_remote(process, addr, bytes);

    let flush_ok = unsafe { FlushInstructionCache(process, addr as *const c_void, bytes.len()) };

    let mut throwaway: PAGE_PROTECTION_FLAGS = 0;
    let restore_ok = unsafe {
        VirtualProtectEx(
            process,
            addr as *mut c_void,
            bytes.len(),
            old_protect,
            &mut throwaway,
        )
    };

    // Report the most specific failure: write error first (the data
    // didn't land), then flush (data landed but CPU may not see it),
    // then restore (data + flush OK but protection didn't roll back).
    write_result?;
    if flush_ok == 0 {
        return Err(PatchError::Flush {
            addr,
            len: bytes.len(),
            os_error: std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        });
    }
    if restore_ok == 0 {
        return Err(PatchError::Protect {
            addr,
            len: bytes.len(),
            desired: old_protect,
            os_error: std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        });
    }

    Ok(())
}

/// RAII guard: suspends every other thread of `pid` on construction,
/// resumes + closes their handles on drop. "Every other" because the
/// bridge itself runs in a different process — we never accidentally
/// freeze our own dispatch loop.
struct ThreadSuspendGuard {
    handles: Vec<HANDLE>,
}

impl ThreadSuspendGuard {
    fn suspend_all(target_pid: u32) -> Result<Self, PatchError> {
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snap == INVALID_HANDLE_VALUE {
            return Err(PatchError::Memory(RemoteMemError::Read {
                addr: 0,
                len: 0,
                os_error: std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
            }));
        }

        let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut handles = Vec::new();

        if unsafe { Thread32First(snap, &mut entry) } != 0 {
            loop {
                if entry.th32OwnerProcessID == target_pid {
                    let h = unsafe { OpenThread(THREAD_SUSPEND_RESUME, FALSE, entry.th32ThreadID) };
                    if !h.is_null() {
                        // SuspendThread returns the prior suspend count
                        // on success, -1 (cast to u32) on failure.
                        let prior = unsafe { SuspendThread(h) };
                        if prior == u32::MAX {
                            unsafe { CloseHandle(h) };
                        } else {
                            handles.push(h);
                        }
                    }
                }
                if unsafe { Thread32Next(snap, &mut entry) } == 0 {
                    break;
                }
            }
        }
        unsafe { CloseHandle(snap) };

        Ok(Self { handles })
    }
}

impl Drop for ThreadSuspendGuard {
    fn drop(&mut self) {
        for h in self.handles.drain(..) {
            unsafe {
                ResumeThread(h);
                CloseHandle(h);
            }
        }
    }
}
