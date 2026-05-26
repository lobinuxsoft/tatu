//! Resolve ELF symbol / module addresses inside a target Linux process.
//!
//! Ported in spirit from Cheat Engine `ceserver/symbols.c:704` (`FindSymbol`),
//! which walks the target's module list and parses each module's on-disk
//! ELF dynamic symbol table. We use the `goblin` crate (MIT, drop-in for
//! CE's `ELF64_scan` / `ELF32_scan`) instead of re-porting the parser.
//!
//! Three primitives, all keyed by an in-memory `/proc/<pid>/maps` scan:
//!
//! - [`find_module_base`] — the load base of the first module whose path
//!   matches the given substring (case-sensitive). Useful when a manifest
//!   wants the address of a known library to base AOB scans off, or to
//!   `bind_symbol` for a fixed-offset constant inside that module.
//! - [`find_module_symbol`] — resolve `<module>::<symbol>` to an absolute
//!   address by adding the load base to the symbol's `st_value`.
//! - [`find_libc_symbol`] — convenience wrapper for the common case
//!   (`find_module_symbol(pid, "libc", name)`); kept as a free function so
//!   the [`crate::alloc`] module's ptrace dance doesn't need to know about
//!   the module-pattern API.
//!
//! Each distinct on-disk path is parsed at most once per call — a
//! multi-segment libc shows up many times in `/proc/<pid>/maps` but the
//! symbol table is the same file. The match function is substring-based
//! (CE's `FindSymbol` does the same) so callers can pass either a full
//! path fragment (`/usr/lib64/libc.so.6`), a base name (`libc.so.6`), or a
//! library name without version (`libc`).

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use goblin::elf::Elf;
use nix::unistd::Pid;

#[derive(Debug, thiserror::Error)]
pub enum ElfSymError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("module matching {pattern:?} not found in /proc/{pid}/maps")]
    ModuleNotFound { pattern: String, pid: i32 },
    #[error("symbol {symbol:?} not found in any module matching {module:?} for pid {pid}")]
    SymbolNotFound {
        module: String,
        symbol: String,
        pid: i32,
    },
    #[error("elf parse error in {path}: {source}")]
    Elf {
        path: PathBuf,
        #[source]
        source: goblin::error::Error,
    },
}

/// Resolve a symbol exported from any module whose `/proc/<pid>/maps` path
/// contains `module_pattern`. The first matching `(module, exported symbol)`
/// pair wins; pass a more specific `module_pattern` (e.g. `libc.so.6`
/// instead of `libc`) when the target has multiple libcs loaded.
pub fn find_module_symbol(
    pid: Pid,
    module_pattern: &str,
    symbol: &str,
) -> Result<u64, ElfSymError> {
    let entries = collect_module_entries(pid, module_pattern)?;
    if entries.is_empty() {
        return Err(ElfSymError::ModuleNotFound {
            pattern: module_pattern.to_string(),
            pid: pid.as_raw(),
        });
    }
    for entry in entries {
        let bytes = match fs::read(&entry.path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let elf = Elf::parse(&bytes).map_err(|source| ElfSymError::Elf {
            path: entry.path.clone().into(),
            source,
        })?;
        let load_base = entry.load_base();
        for sym in elf.dynsyms.iter() {
            if sym.st_value == 0 || !sym.is_function() {
                continue;
            }
            let Some(sym_name) = elf.dynstrtab.get_at(sym.st_name) else {
                continue;
            };
            if sym_name == symbol {
                return Ok(load_base + sym.st_value);
            }
        }
    }
    Err(ElfSymError::SymbolNotFound {
        module: module_pattern.to_string(),
        symbol: symbol.to_string(),
        pid: pid.as_raw(),
    })
}

/// Return the load base of the first module in `/proc/<pid>/maps` whose
/// path contains `module_pattern`. Useful for "load + known offset"
/// resolution where the offset is hand-curated.
pub fn find_module_base(pid: Pid, module_pattern: &str) -> Result<u64, ElfSymError> {
    let entries = collect_module_entries(pid, module_pattern)?;
    entries
        .into_iter()
        .next()
        .map(|e| e.load_base())
        .ok_or_else(|| ElfSymError::ModuleNotFound {
            pattern: module_pattern.to_string(),
            pid: pid.as_raw(),
        })
}

/// Convenience for the [`crate::alloc`] hot path: resolve a libc export.
/// Equivalent to `find_module_symbol(pid, "libc", name)` but kept as a
/// free function so existing call sites stay short.
pub fn find_libc_symbol(pid: Pid, name: &str) -> Result<u64, ElfSymError> {
    find_module_symbol(pid, "libc", name)
}

#[derive(Debug)]
struct ModuleEntry {
    start: u64,
    file_offset: u64,
    path: String,
}

impl ModuleEntry {
    fn load_base(&self) -> u64 {
        self.start.saturating_sub(self.file_offset)
    }
}

fn collect_module_entries(pid: Pid, pattern: &str) -> Result<Vec<ModuleEntry>, ElfSymError> {
    let maps = fs::read_to_string(format!("/proc/{}/maps", pid.as_raw()))?;
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for line in maps.lines() {
        let Some(entry) = parse_maps_line(line) else {
            continue;
        };
        if !entry.path.contains(pattern) {
            continue;
        }
        if !seen.insert(entry.path.clone()) {
            continue;
        }
        out.push(entry);
    }
    Ok(out)
}

fn parse_maps_line(line: &str) -> Option<ModuleEntry> {
    let mut parts = line.split_whitespace();
    let range = parts.next()?;
    let _perms = parts.next()?;
    let offset = parts.next()?;
    let _dev = parts.next()?;
    let _inode = parts.next()?;
    let path = parts.collect::<Vec<_>>().join(" ");
    if path.is_empty() || path.starts_with('[') {
        return None;
    }
    let start = u64::from_str_radix(range.split('-').next()?, 16).ok()?;
    let file_offset = u64::from_str_radix(offset, 16).ok()?;
    Some(ModuleEntry {
        start,
        file_offset,
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_maps_line_extracts_libc() {
        let line = "7f1234567000-7f123456a000 r-xp 00000000 fd:01 12345  /usr/lib64/libc.so.6";
        let entry = parse_maps_line(line).expect("parse");
        assert_eq!(entry.start, 0x7f1234567000);
        assert_eq!(entry.file_offset, 0);
        assert_eq!(entry.path, "/usr/lib64/libc.so.6");
        assert_eq!(entry.load_base(), 0x7f1234567000);
    }

    #[test]
    fn parse_maps_line_skips_anonymous_and_pseudo() {
        assert!(parse_maps_line("7f0000000000-7f0000001000 rw-p 00000000 00:00 0").is_none());
        assert!(
            parse_maps_line("7ffff7fdc000-7ffff7ffd000 r-xp 00000000 00:00 0  [vdso]").is_none()
        );
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn find_libc_symbol_locates_mmap_in_self() {
        let addr = find_libc_symbol(Pid::this(), "mmap").expect("mmap in self libc");
        let expected = nix::libc::mmap as *const () as u64;
        assert_eq!(addr, expected);
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn find_module_base_locates_libc_in_self() {
        let base = find_module_base(Pid::this(), "libc.so").expect("libc base in self");
        // The actual libc base in this process is the start address of the
        // first PT_LOAD with offset 0. We don't know the exact value but it
        // must be non-zero and page-aligned.
        assert!(base != 0);
        assert_eq!(base & 0xfff, 0);
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn unknown_module_returns_module_not_found() {
        let err =
            find_module_symbol(Pid::this(), "definitely-not-a-real-lib-xyzzy", "foo").unwrap_err();
        assert!(matches!(err, ElfSymError::ModuleNotFound { .. }));
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn unknown_symbol_returns_symbol_not_found() {
        let err = find_module_symbol(Pid::this(), "libc", "definitely_not_a_libc_export_xyzzy")
            .unwrap_err();
        assert!(matches!(err, ElfSymError::SymbolNotFound { .. }));
    }
}
