//! AOB scan against a remote process. Pulls chunks of memory via
//! `ReadProcessMemory` and runs them through [`aob::Pattern::scan`].
//! Two entry points:
//!
//! - [`scan_module`] — bounds the search to one loaded module's
//!   in-memory range (typical for `"game.exe+1A2B"`-style signatures).
//! - [`scan_all_readable`] — sweeps every committed readable region
//!   reported by `VirtualQueryEx`. Slower but matches anonymous
//!   allocations / JIT regions.
//!
//! Pages crossing the chunk boundary are handled via an overlap of
//! `pattern.len() - 1` bytes between consecutive reads, so a match
//! that straddles the boundary is still found exactly once.

use std::mem;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE,
    PAGE_EXECUTE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY,
    PAGE_WRITECOPY, VirtualQueryEx,
};

use super::aob::{Pattern, scan_chunk_size};
use super::modules::{ModulesError, find_module};
use super::remote_mem::read_remote_partial;

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
    Ok(scan_range(process, module.base, module.size, pattern))
}

pub fn scan_all_readable(process: HANDLE, pattern: &Pattern) -> Vec<u64> {
    let mut out = Vec::new();
    let mut addr: u64 = 0;
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
            out.extend(scan_range(process, region_base, region_size, pattern));
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

fn scan_range(process: HANDLE, base: u64, size: u64, pattern: &Pattern) -> Vec<u64> {
    let mut out = Vec::new();
    if size < pattern.len() as u64 {
        return out;
    }
    let chunk = scan_chunk_size();
    let overlap = pattern.len().saturating_sub(1);
    let mut offset: u64 = 0;
    while offset < size {
        let remaining = size - offset;
        let want = (chunk as u64).min(remaining) as usize;
        let bytes = read_remote_partial(process, base + offset, want);
        if bytes.is_empty() {
            // Unmapped region inside the range — skip past it.
            offset = offset.saturating_add(want as u64);
            continue;
        }
        for hit in pattern.scan(&bytes) {
            out.push(base + offset + hit as u64);
        }
        if bytes.len() < want {
            // Short read — advance past whatever we got, no overlap
            // possible because the tail wasn't there.
            offset += bytes.len() as u64;
            continue;
        }
        if remaining <= chunk as u64 {
            break;
        }
        offset += (chunk - overlap) as u64;
    }
    out.sort_unstable();
    out.dedup();
    out
}
