//! Multi-module symbol resolution with caching — the missing pieces
//! from `ceserver/symbols.c` (~800 LOC) on top of the per-module
//! lookup helpers in `elfsym`.
//!
//! Ported 1:1 in spirit from CE's:
//! - `FindSymbol` (symbols.c:704) — module iteration + per-module
//!   scan loop → [`SymbolTable::resolve`].
//! - `enumerateModules` (referenced by FindSymbol but inlined in CE)
//!   → [`enumerate_modules`].
//! - The `module:symbol` / `module+0xoff` / bare `symbol` grammar CE
//!   accepts via the network protocol → [`SymbolSpec`].
//!
//! The `goblin` crate is used (as in `elfsym`) instead of porting the
//! hand-rolled `ELF32_scan` / `ELF64_scan` parsers — same conclusion
//! [[reference_cheat_engine_source]] called out.
//!
//! # Out of scope
//!
//! DWARF function names + line numbers (CE's
//! `FindFunctionByName` path) are deliberately deferred — `.CT`
//! tables use offsets from module bases, not DWARF function
//! identifiers. The use case can be added on top of `goblin::dwarf`
//! when an actual `.CT` consumer asks for it.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use goblin::elf::Elf;
use nix::unistd::Pid;

#[derive(Debug, thiserror::Error)]
pub enum SymbolError {
    #[error("io reading /proc/{pid}/maps or module on disk: {source}")]
    Io {
        pid: i32,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid symbol spec {spec:?}: {reason}")]
    InvalidSpec { spec: String, reason: &'static str },
    #[error("module {0:?} not loaded in target process")]
    ModuleNotLoaded(String),
    #[error("symbol {symbol:?} not found in module {module:?}")]
    SymbolNotFound { module: String, symbol: String },
    #[error("elf parse error in {path}: {source}")]
    Elf {
        path: PathBuf,
        #[source]
        source: goblin::error::Error,
    },
}

/// One distinct module loaded into the target process (a unique
/// backing file in `/proc/<pid>/maps`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// Full path on disk (or whatever `/proc/.../maps` exposes —
    /// could be `/usr/lib/libc.so.6` or e.g. `/tmp/anonymous-mmap-N`).
    pub path: String,
    /// First (lowest) mapped address among this module's segments —
    /// adjusted by the segment's file offset so it represents the
    /// "image base" the symbol's `st_value` is relative to.
    pub load_base: u64,
}

/// What [`SymbolTable::resolve`] understands. CE accepts the same
/// three forms via the network protocol; the upper layer (manifest
/// loader, ce-launcher) typically receives a string and parses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolSpec {
    /// `libfoo.so:SymName` — resolve `SymName` exported by the first
    /// loaded module whose path contains `libfoo.so`.
    QualifiedSymbol { module: String, symbol: String },
    /// `libfoo.so+0x1234` — load base of the matching module plus
    /// `offset`.
    ModuleOffset { module: String, offset: u64 },
    /// Bare `SymName` — searches every module in load order. CE picks
    /// the first hit; we do the same.
    BareSymbol(String),
}

impl SymbolSpec {
    /// Parse `s` into one of the three CE-compatible forms. Empty
    /// strings + whitespace-only are rejected; the rest of validation
    /// happens at resolve time.
    pub fn parse(s: &str) -> Result<Self, SymbolError> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(SymbolError::InvalidSpec {
                spec: s.to_string(),
                reason: "empty",
            });
        }
        // CE uses `+` for offset literals — split at the first `+`
        // that appears AFTER the module name (so paths with `+` in
        // them don't tear). The canonical form CE emits never has
        // `+` in the module name, so first-`+` is safe.
        if let Some((module, rest)) = trimmed.split_once('+') {
            let module = module.trim();
            let rest = rest.trim();
            if module.is_empty() {
                return Err(SymbolError::InvalidSpec {
                    spec: s.to_string(),
                    reason: "empty module before +",
                });
            }
            let offset = parse_hex_or_dec_u64(rest).ok_or(SymbolError::InvalidSpec {
                spec: s.to_string(),
                reason: "offset is not a number",
            })?;
            return Ok(SymbolSpec::ModuleOffset {
                module: module.to_string(),
                offset,
            });
        }
        if let Some((module, sym)) = trimmed.split_once(':') {
            let module = module.trim();
            let sym = sym.trim();
            if module.is_empty() || sym.is_empty() {
                return Err(SymbolError::InvalidSpec {
                    spec: s.to_string(),
                    reason: "empty module or symbol around :",
                });
            }
            return Ok(SymbolSpec::QualifiedSymbol {
                module: module.to_string(),
                symbol: sym.to_string(),
            });
        }
        Ok(SymbolSpec::BareSymbol(trimmed.to_string()))
    }
}

fn parse_hex_or_dec_u64(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok();
    }
    if s.chars().all(|c| c.is_ascii_hexdigit())
        && s.chars()
            .any(|c| c.is_ascii_hexdigit() && !c.is_ascii_digit())
    {
        // Bare hex without 0x prefix (e.g. "deadbeef") — CE accepts
        // this from CT files.
        return u64::from_str_radix(s, 16).ok();
    }
    s.parse::<u64>().ok()
}

/// Enumerate every module mapped into `pid` — one entry per unique
/// backing file path. The load base of each entry is the lowest
/// mapped address minus its file offset (so adding `st_value` from
/// the ELF gives the absolute runtime address). Mirror CE's inline
/// module enumeration inside `FindSymbol` (symbols.c:704).
pub fn enumerate_modules(pid: Pid) -> Result<Vec<Module>, SymbolError> {
    let maps = fs::read_to_string(format!("/proc/{}/maps", pid.as_raw())).map_err(|source| {
        SymbolError::Io {
            pid: pid.as_raw(),
            source,
        }
    })?;
    let mut by_path: HashMap<String, u64> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for line in maps.lines() {
        let Some((start, file_offset, path)) = parse_maps_line(line) else {
            continue;
        };
        let base = start.saturating_sub(file_offset);
        if let Some(existing) = by_path.get_mut(&path) {
            if base < *existing {
                *existing = base;
            }
            continue;
        }
        order.push(path.clone());
        by_path.insert(path, base);
    }
    Ok(order
        .into_iter()
        .map(|path| Module {
            load_base: by_path[&path],
            path,
        })
        .collect())
}

fn parse_maps_line(line: &str) -> Option<(u64, u64, String)> {
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
    Some((start, file_offset, path))
}

/// Per-process symbol cache. Reads `/proc/<pid>/maps` once at
/// construction, parses each unique module's ELF on first lookup,
/// memoizes the symbol map afterwards. The CE equivalent (`FindSymbol`
/// in symbols.c:704) re-walks the maps + re-parses ELFs on every
/// call; CE pays this cost because its protocol is request/response
/// and short-lived. We're a long-lived in-process runtime, so the
/// cache is the obvious extension.
#[derive(Debug)]
pub struct SymbolTable {
    modules: Vec<Module>,
    /// Per-module symbol map, populated lazily on first lookup.
    cache: Mutex<HashMap<String, ModuleSymbols>>,
}

#[derive(Debug)]
struct ModuleSymbols {
    /// `symbol -> absolute address` (load_base + st_value).
    map: HashMap<String, u64>,
}

impl SymbolTable {
    /// Build a fresh symbol table by snapshotting `/proc/<pid>/maps`.
    /// Module parsing is lazy — calling `for_pid` is cheap.
    pub fn for_pid(pid: Pid) -> Result<Self, SymbolError> {
        let modules = enumerate_modules(pid)?;
        Ok(Self {
            modules,
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// Borrow the snapshotted module list.
    pub fn modules(&self) -> &[Module] {
        &self.modules
    }

    /// Resolve a parsed [`SymbolSpec`] to an absolute address.
    pub fn resolve(&self, spec: &SymbolSpec) -> Result<u64, SymbolError> {
        match spec {
            SymbolSpec::ModuleOffset { module, offset } => {
                let m = self.find_module(module)?;
                Ok(m.load_base + offset)
            }
            SymbolSpec::QualifiedSymbol { module, symbol } => {
                let m = self.find_module(module)?.clone();
                let syms = self.ensure_module_loaded(&m)?;
                let addr =
                    syms.map
                        .get(symbol)
                        .copied()
                        .ok_or_else(|| SymbolError::SymbolNotFound {
                            module: module.clone(),
                            symbol: symbol.clone(),
                        })?;
                Ok(addr)
            }
            SymbolSpec::BareSymbol(symbol) => {
                let modules: Vec<Module> = self.modules.clone();
                for m in &modules {
                    let syms = self.ensure_module_loaded(m)?;
                    if let Some(&addr) = syms.map.get(symbol) {
                        return Ok(addr);
                    }
                }
                Err(SymbolError::SymbolNotFound {
                    module: "<any>".into(),
                    symbol: symbol.clone(),
                })
            }
        }
    }

    /// Convenience: parse + resolve in one call.
    pub fn resolve_str(&self, s: &str) -> Result<u64, SymbolError> {
        let spec = SymbolSpec::parse(s)?;
        self.resolve(&spec)
    }

    fn find_module(&self, pattern: &str) -> Result<&Module, SymbolError> {
        self.modules
            .iter()
            .find(|m| m.path.contains(pattern))
            .ok_or_else(|| SymbolError::ModuleNotLoaded(pattern.to_string()))
    }

    fn ensure_module_loaded(&self, m: &Module) -> Result<ModuleSymbolsRef<'_>, SymbolError> {
        let mut cache = self.cache.lock().expect("symbol cache poisoned");
        if !cache.contains_key(&m.path) {
            let bytes = fs::read(&m.path).map_err(|source| SymbolError::Io { pid: 0, source })?;
            let elf = Elf::parse(&bytes).map_err(|source| SymbolError::Elf {
                path: m.path.clone().into(),
                source,
            })?;
            let mut map: HashMap<String, u64> = HashMap::new();
            // .dynsym first (CE prefers it — dynamic linker is the
            // source of truth for runtime resolution).
            for sym in elf.dynsyms.iter() {
                if sym.st_value == 0 {
                    continue;
                }
                if let Some(name) = elf.dynstrtab.get_at(sym.st_name)
                    && !name.is_empty()
                {
                    map.entry(name.to_string())
                        .or_insert(m.load_base + sym.st_value);
                }
            }
            // .symtab (static symbols, stripped from production
            // binaries but present in -g builds + lots of system libs).
            for sym in elf.syms.iter() {
                if sym.st_value == 0 {
                    continue;
                }
                if let Some(name) = elf.strtab.get_at(sym.st_name)
                    && !name.is_empty()
                {
                    map.entry(name.to_string())
                        .or_insert(m.load_base + sym.st_value);
                }
            }
            cache.insert(m.path.clone(), ModuleSymbols { map });
        }
        Ok(ModuleSymbolsRef {
            cache,
            path: m.path.clone(),
        })
    }
}

/// Smart guard returned by `ensure_module_loaded` so the caller can
/// look up `symbol` without holding the lock manually.
struct ModuleSymbolsRef<'a> {
    cache: std::sync::MutexGuard<'a, HashMap<String, ModuleSymbols>>,
    path: String,
}

impl std::ops::Deref for ModuleSymbolsRef<'_> {
    type Target = ModuleSymbols;
    fn deref(&self) -> &Self::Target {
        &self.cache[&self.path]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_spec_bare_symbol() {
        let s = SymbolSpec::parse("malloc").unwrap();
        assert_eq!(s, SymbolSpec::BareSymbol("malloc".into()));
    }

    #[test]
    fn parse_spec_qualified() {
        let s = SymbolSpec::parse("libc.so.6:malloc").unwrap();
        assert_eq!(
            s,
            SymbolSpec::QualifiedSymbol {
                module: "libc.so.6".into(),
                symbol: "malloc".into()
            }
        );
    }

    #[test]
    fn parse_spec_module_offset_hex_with_prefix() {
        let s = SymbolSpec::parse("game.exe+0x12345").unwrap();
        assert_eq!(
            s,
            SymbolSpec::ModuleOffset {
                module: "game.exe".into(),
                offset: 0x12345
            }
        );
    }

    #[test]
    fn parse_spec_module_offset_decimal() {
        let s = SymbolSpec::parse("foo.so+1024").unwrap();
        assert_eq!(
            s,
            SymbolSpec::ModuleOffset {
                module: "foo.so".into(),
                offset: 1024
            }
        );
    }

    #[test]
    fn parse_spec_empty_rejected() {
        assert!(matches!(
            SymbolSpec::parse("   ").unwrap_err(),
            SymbolError::InvalidSpec {
                reason: "empty",
                ..
            }
        ));
    }

    #[test]
    fn parse_spec_malformed_offset_rejected() {
        let err = SymbolSpec::parse("foo.so+notanumber").unwrap_err();
        assert!(matches!(err, SymbolError::InvalidSpec { .. }));
    }

    #[test]
    fn parse_spec_empty_around_colon_rejected() {
        assert!(SymbolSpec::parse(":malloc").is_err());
        assert!(SymbolSpec::parse("libc:").is_err());
    }

    #[test]
    fn parse_hex_with_0x() {
        assert_eq!(parse_hex_or_dec_u64("0xDEAD"), Some(0xDEAD));
        assert_eq!(parse_hex_or_dec_u64("0X42"), Some(0x42));
    }

    #[test]
    fn parse_hex_bare_when_unambiguous() {
        // Has at least one a-f → assumed hex.
        assert_eq!(parse_hex_or_dec_u64("deadbeef"), Some(0xdeadbeef));
    }

    #[test]
    fn parse_decimal_when_no_letters() {
        // All digits → decimal.
        assert_eq!(parse_hex_or_dec_u64("1234"), Some(1234));
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0; reads /proc/self/maps"]
    fn enumerate_modules_self_contains_libc() {
        let mods = enumerate_modules(Pid::this()).expect("enumerate");
        assert!(
            mods.iter().any(|m| m.path.contains("libc")),
            "libc not in modules: {:?}",
            mods.iter().map(|m| &m.path).collect::<Vec<_>>()
        );
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0"]
    fn symbol_table_resolves_libc_malloc_to_dlsym_address() {
        let table = SymbolTable::for_pid(Pid::this()).expect("table");
        let addr = table
            .resolve_str("libc.so:malloc")
            .expect("resolve libc.so:malloc");
        let expected = nix::libc::malloc as *const () as u64;
        assert_eq!(addr, expected);
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0"]
    fn symbol_table_module_offset_returns_base_plus_offset() {
        let table = SymbolTable::for_pid(Pid::this()).expect("table");
        let resolved = table.resolve_str("libc.so+0x100").expect("module+off");
        let base_only = table.resolve_str("libc.so+0").expect("module+0");
        assert_eq!(resolved, base_only + 0x100);
    }
}
