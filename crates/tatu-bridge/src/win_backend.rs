//! [`tatu_engine::Backend`] implementation for the Win32 in-prefix
//! bridge. Composes the Phase 4 primitives (`remote_mem` for r/w,
//! `alloc` for codecave alloc/free, `patch` for the i-cache flush,
//! plus a fresh `VirtualQueryEx` walk for region enumeration) into
//! the same trait `cheat-runtime`'s `LinuxBackend` implements, so
//! the autoassembler executor in `tatu-engine` runs unchanged
//! against either side.

use std::mem;
use std::os::raw::c_void;
use std::path::PathBuf;

use tatu_engine::backend::{Backend, BackendError, ReadableRegion, RegionPerms};
use tatu_mem::MemoryAccess;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
    PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE,
    PAGE_WRITECOPY, VirtualQueryEx,
};

use super::alloc as remote_alloc;
use super::patch;
use super::remote_mem::Win32Mem;

unsafe extern "system" {
    fn FlushInstructionCache(hProcess: HANDLE, lpBaseAddress: *const c_void, dwSize: usize) -> i32;
}

/// Backend impl for the in-prefix Win32 bridge. Owns the open
/// process handle (already obtained by `connect_mode` before serving
/// kicks in) plus the target PID — passed through to the patch path
/// for thread enumeration.
pub struct Win32Backend {
    process: HANDLE,
    target_pid: u32,
    mem: Win32Mem,
}

impl Win32Backend {
    pub fn new(process: HANDLE, target_pid: u32) -> Self {
        Self {
            process,
            target_pid,
            mem: Win32Mem::new(process),
        }
    }

    pub fn process(&self) -> HANDLE {
        self.process
    }

    pub fn target_pid(&self) -> u32 {
        self.target_pid
    }
}

impl MemoryAccess for Win32Backend {
    type Error = BackendError;

    fn read(&mut self, addr: u64, len: usize) -> Result<Vec<u8>, BackendError> {
        self.mem.read(addr, len).map_err(BackendError::new)
    }

    fn read_partial(&mut self, addr: u64, len: usize) -> Vec<u8> {
        self.mem.read_partial(addr, len)
    }

    fn write(&mut self, addr: u64, bytes: &[u8]) -> Result<(), BackendError> {
        // Always go through patch_bytes: VirtualProtectEx lift →
        // WriteProcessMemory → FlushInstructionCache → restore,
        // bracketed by SuspendThread/ResumeThread on every thread
        // of the target. The AA engine writes to a mix of .text
        // (read-only by default) and codecaves (RWX from
        // VirtualAllocEx) — patch_bytes is the only path that
        // handles .text writes correctly cross-process on x86_64.
        //
        // Raw WriteProcessMemory can appear to succeed (Wine /
        // Windows lift PAGE_EXECUTE_READ for handles with
        // PROCESS_VM_WRITE) but leaves the target's i-cache stale
        // and racy against live threads in the patched range,
        // producing the "everything wrote OK but the game crashed"
        // failure mode we just hit on EM.
        patch::patch_bytes(self.process, self.target_pid, addr, bytes, true)
            .map_err(BackendError::new)
    }
}

impl Backend for Win32Backend {
    fn alloc(&mut self, size: usize, near_hint: Option<u64>) -> Result<u64, BackendError> {
        // Executable codecaves — autoassembler scripts always need
        // PAGE_EXECUTE_READWRITE so a `jmp newmem` lands in a region
        // the CPU can execute.
        remote_alloc::alloc_remote(self.process, near_hint, size as u64, true)
            .map_err(BackendError::new)
    }

    fn dealloc(&mut self, addr: u64, _size: usize) -> Result<(), BackendError> {
        // VirtualFreeEx + MEM_RELEASE ignores `size`; the kernel
        // frees the full region the original alloc reserved.
        remote_alloc::free_remote(self.process, addr).map_err(BackendError::new)
    }

    fn readable_regions(&mut self) -> Result<Vec<ReadableRegion>, BackendError> {
        let mut out = Vec::new();
        let mut addr: u64 = 0;
        loop {
            let mut info: MEMORY_BASIC_INFORMATION = unsafe { mem::zeroed() };
            let written = unsafe {
                VirtualQueryEx(
                    self.process,
                    addr as *const _,
                    &mut info,
                    mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            };
            if written == 0 {
                break;
            }
            let region_base = info.BaseAddress as u64;
            let region_size = info.RegionSize as u64;
            let region_end = region_base.saturating_add(region_size);

            if info.State == MEM_COMMIT && is_readable(info.Protect) {
                out.push(ReadableRegion {
                    start: region_base,
                    end: region_end,
                    perms: RegionPerms {
                        read: true,
                        write: is_writable(info.Protect),
                        execute: is_executable(info.Protect),
                    },
                    // Win32 doesn't surface a path here cheaply —
                    // mapped-file regions could be looked up via
                    // GetMappedFileNameW but the scanner doesn't
                    // filter on path so leave empty.
                    path: PathBuf::new(),
                });
            }

            if region_end == 0 || region_end <= addr {
                break;
            }
            addr = region_end;
        }
        Ok(out)
    }

    fn attach(&mut self) -> bool {
        // OpenProcess already granted PROCESS_VM_OPERATION /
        // PROCESS_VM_READ / PROCESS_VM_WRITE in connect_mode — no
        // separate attach handshake needed on Win32. Threads stay
        // running; the patcher's per-call SuspendThread (Phase 4
        // patch.rs) is what gives us atomicity around individual
        // writes.
        false
    }

    fn detach(&mut self) {
        // Symmetrical no-op.
    }

    fn flush_instruction_cache(&mut self, addr: u64, len: usize) -> Result<(), BackendError> {
        if len == 0 {
            return Ok(());
        }
        let ok = unsafe { FlushInstructionCache(self.process, addr as *const c_void, len) };
        if ok == 0 {
            return Err(BackendError::new(std::io::Error::last_os_error()));
        }
        Ok(())
    }
}

fn is_readable(protect: u32) -> bool {
    if protect & PAGE_GUARD != 0 || protect == PAGE_NOACCESS {
        return false;
    }
    matches!(
        protect & 0xFF,
        PAGE_READONLY
            | PAGE_READWRITE
            | PAGE_WRITECOPY
            | PAGE_EXECUTE
            | PAGE_EXECUTE_READ
            | PAGE_EXECUTE_READWRITE
            | PAGE_EXECUTE_WRITECOPY
    )
}

fn is_writable(protect: u32) -> bool {
    matches!(
        protect & 0xFF,
        PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY
    )
}

fn is_executable(protect: u32) -> bool {
    matches!(
        protect & 0xFF,
        PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY
    )
}
