//! AOB scan against a remote process — Win32 front-end on top of
//! [`tatu_mem::pattern::scan_range`]. Two entry points:
//!
//! - [`scan_module`] — bounds the search to one loaded module's
//!   in-memory range (typical for `"game.exe+1A2B"`-style signatures).
//! - [`scan_all_readable`] — sweeps every committed readable region
//!   reported by `VirtualQueryEx`. Slower but matches anonymous
//!   allocations / JIT regions.
//!
//! The pure scanning kernel + chunked-read loop lives in `tatu-mem`;
//! this module supplies the Win32-only pieces (module enumeration,
//! `VirtualQueryEx` region walk, [`tatu_mem::MemoryAccess`] adapter
//! via [`super::remote_mem::Win32Mem`]).

use std::mem;

use tatu_mem::pattern::Pattern;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
    PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE,
    PAGE_WRITECOPY, VirtualQueryEx,
};

use super::modules::{ModulesError, find_module};
use super::remote_mem::Win32Mem;

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("module {name:?} not loaded in target")]
    ModuleNotFound { name: String },
    #[error("module enumeration: {0}")]
    Modules(#[from] ModulesError),
}

pub fn scan_module(
    process: HANDLE,
    module_name: &str,
    pattern: &Pattern,
) -> Result<Vec<u64>, ScanError> {
    let module = find_module(process, module_name)?.ok_or_else(|| ScanError::ModuleNotFound {
        name: module_name.to_string(),
    })?;
    let mut mem = Win32Mem::new(process);
    Ok(tatu_mem::pattern::scan_range(
        &mut mem,
        module.base,
        module.size,
        pattern,
    ))
}

pub fn scan_all_readable(process: HANDLE, pattern: &Pattern) -> Vec<u64> {
    let mut out = Vec::new();
    let mut addr: u64 = 0;
    let mut mem = Win32Mem::new(process);
    loop {
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { mem::zeroed() };
        let written = unsafe {
            VirtualQueryEx(
                process,
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

        if info.State == MEM_COMMIT && is_readable_protection(info.Protect) {
            out.extend(tatu_mem::pattern::scan_range(
                &mut mem,
                region_base,
                region_size,
                pattern,
            ));
        }

        if region_end == 0 || region_end <= addr {
            break;
        }
        addr = region_end;
    }
    out
}

fn is_readable_protection(protect: u32) -> bool {
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
