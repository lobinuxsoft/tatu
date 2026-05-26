//! The two-pass executor that walks a parsed [`crate::parser::Script`]
//! and applies it through a [`Backend`]. Produces an [`EnableOutcome`]
//! POD; the platform-specific wrapper (`cheat-runtime::ActiveCheat`
//! for Linux, the bridge's response payload for Win32) takes
//! ownership of the rollback from there.

use std::collections::HashMap;

use tatu_mem::pattern::Pattern;

use crate::backend::{Backend, ReadableRegion};
use crate::parser::{NameList, Script, Statement};

use super::EnableOutcome;
use super::error::ExecError;
use super::length::estimate_raw_length;
use super::raw_compiler::compile_raw;

/// Execution context. Owns the [`Backend`] for the duration of an
/// [`Self::enable`] cycle plus the symbol table built up by the
/// `aobscanmodule` / `registersymbol` / `alloc` directives.
pub struct Engine<B: Backend> {
    backend: B,
    symbols: HashMap<String, u64>,
}

impl<B: Backend> Engine<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            symbols: HashMap::new(),
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    /// Consume the engine and return its owned backend — handy for
    /// callers that need to keep talking to the target after the
    /// enable cycle (e.g. value-cheat read/write threads).
    pub fn into_backend(self) -> B {
        self.backend
    }

    pub fn symbols(&self) -> &HashMap<String, u64> {
        &self.symbols
    }

    /// Programmatically bind a symbol to an address. Useful for tests,
    /// for loaders that resolve symbols outside the AOB-scan path
    /// (e.g. from a manifest), and for layered scripts that share a
    /// common scan table.
    pub fn bind_symbol(&mut self, name: impl Into<String>, addr: u64) {
        self.symbols.insert(name.into(), addr);
    }

    /// Walk `script.enable` and apply every supported statement.
    ///
    /// Two-pass evaluation:
    /// - Pass 1: side-effecting symbol providers (`aobscanmodule`,
    ///   `alloc`) plus a virtual cursor that estimates the byte
    ///   length of every `Statement::Raw` so forward labels resolve.
    /// - Pass 2: emits the bytes with the now-complete symbol table.
    ///
    /// On any error, every prior write is rolled back via the same
    /// backend before returning.
    pub fn enable(&mut self, script: &Script) -> Result<EnableOutcome, ExecError> {
        if script.lua_only {
            // Surface as a typed error rather than silently succeeding —
            // the UI distinguishes "this entry is broken" from "this
            // entry's enable path is in CE's Lua interpreter, which
            // tatu doesn't ship".
            return Err(ExecError::LuaNotSupported);
        }
        let mut outcome = EnableOutcome::default();

        if let Err(e) = self.pre_resolve_symbols(script, &mut outcome) {
            // Roll back whatever the pass-1 side effects already wrote
            // (alloc-only at this stage; no Raw writes happen in pass 1).
            self.rollback(&mut outcome);
            return Err(e);
        }

        // One attach for the whole batch — the backend decides whether
        // attach is meaningful (ptrace SIGSTOP on Linux, no-op on Win32
        // because OpenProcess already grants R/W).
        let attached = self.backend.attach();
        let write_result = self.write_pass(script, &mut outcome);
        self.backend.detach();

        if let Err(e) = write_result {
            self.rollback(&mut outcome);
            return Err(e);
        }
        // attached just bookkeeping — useful when the backend wants
        // to assert the post-batch state. Logged here for debug builds.
        let _ = attached;

        outcome.symbols = self.symbols.clone();
        Ok(outcome)
    }

    /// Pass 1: bind every symbol the script references before pass 2 writes.
    fn pre_resolve_symbols(
        &mut self,
        script: &Script,
        outcome: &mut EnableOutcome,
    ) -> Result<(), ExecError> {
        let mut cursor: Option<u64> = None;
        for stmt in &script.enable {
            match stmt {
                Statement::AobScanModule {
                    symbol, pattern, ..
                } => {
                    let addr = self.scan_unique(pattern, symbol)?;
                    self.symbols.insert(symbol.clone(), addr);
                }
                Statement::Alloc { symbol, size, near } => {
                    let hint = near.as_ref().and_then(|n| {
                        self.symbols
                            .get(n)
                            .copied()
                            .or_else(|| parse_numeric_token(n))
                    });
                    let addr = self.backend.alloc(*size as usize, hint)?;
                    self.symbols.insert(symbol.clone(), addr);
                    outcome
                        .allocs
                        .insert(symbol.clone(), (addr, *size as usize));
                }
                Statement::GlobalAlloc { symbol, size } => {
                    // CE's `globalalloc` is process-global; tatu treats it
                    // identically to `alloc` because per-toggle rollback
                    // owns the lifetime anyway. No `near` hint by spec.
                    let addr = self.backend.alloc(*size as usize, None)?;
                    self.symbols.insert(symbol.clone(), addr);
                    outcome
                        .allocs
                        .insert(symbol.clone(), (addr, *size as usize));
                }
                Statement::Define { name, value } => {
                    // Defines that resolve to a numeric literal go straight
                    // into the symbol table; non-numeric values are deferred
                    // until the asm compiler asks for them via `Define`
                    // lookup. Symbol-table insert keeps `register_symbol`
                    // happy and matches CE's behaviour for the common
                    // module-relative-offset pattern.
                    if let Some(addr) = parse_numeric_token(value) {
                        self.symbols.insert(name.clone(), addr);
                    }
                }
                Statement::LabelSite(name) => {
                    if let Some(addr) = self.symbols.get(name).copied() {
                        cursor = Some(addr);
                    } else if let Some(c) = cursor {
                        self.symbols.insert(name.clone(), c);
                    }
                }
                Statement::AbsoluteSite(addr) => {
                    cursor = Some(*addr);
                }
                Statement::Raw(line) => {
                    if let Some(c) = cursor.as_mut() {
                        let len =
                            estimate_raw_length(line, &self.symbols, *c).ok_or_else(|| {
                                ExecError::Unsupported(format!(
                                    "cannot estimate length for pass 1: {line:?}"
                                ))
                            })?;
                        *c = c.wrapping_add(len as u64);
                    }
                }
                Statement::RegisterSymbol(_)
                | Statement::UnregisterSymbol(_)
                | Statement::Label(_)
                | Statement::Directive(_)
                | Statement::Dealloc(_) => {}
            }
        }
        Ok(())
    }

    /// Pass 2: with every symbol now bound, walk again and emit the bytes.
    fn write_pass(
        &mut self,
        script: &Script,
        outcome: &mut EnableOutcome,
    ) -> Result<(), ExecError> {
        let mut cursor: Option<u64> = None;
        for stmt in &script.enable {
            match stmt {
                Statement::LabelSite(name) => {
                    let addr = *self
                        .symbols
                        .get(name)
                        .ok_or_else(|| ExecError::UnknownSymbol(name.clone()))?;
                    cursor = Some(addr);
                }
                Statement::AbsoluteSite(addr) => {
                    cursor = Some(*addr);
                }
                Statement::Raw(line) => {
                    let Some(base) = cursor else {
                        return Err(ExecError::OrphanWrite(line.clone()));
                    };
                    let bytes = compile_raw(line, &self.symbols, &mut self.backend, base)?;
                    let original = self.backend.read(base, bytes.len())?;
                    if std::env::var_os("CHEAT_RUNTIME_TRACE").is_some() {
                        eprintln!(
                            "[trace] @0x{:x} write {:>2}B {:02X?}  was {:02X?}  ← {}",
                            base,
                            bytes.len(),
                            bytes,
                            original,
                            line
                        );
                    }
                    self.backend.write(base, &bytes)?;
                    self.backend.flush_instruction_cache(base, bytes.len())?;
                    outcome.undo.push((base, original));
                    cursor = Some(base + bytes.len() as u64);
                }
                Statement::Dealloc(list) => match list {
                    NameList::Wildcard => {
                        // `dealloc(*)` inside an `[ENABLE]` block: release
                        // every codecave this script has allocated so far.
                        // Rare but legal (some scripts pre-clean before
                        // reallocating).
                        for (name, (addr, size)) in outcome.allocs.drain() {
                            self.backend.dealloc(addr, size)?;
                            self.symbols.remove(&name);
                        }
                    }
                    NameList::Names(names) => {
                        for symbol in names {
                            // No-op on unknown names: CE's `dealloc` is
                            // lenient when a name was already freed by a
                            // companion script. Erroring here would block
                            // legitimate idempotent disables.
                            if let Some((addr, size)) = outcome.allocs.remove(symbol) {
                                self.backend.dealloc(addr, size)?;
                                self.symbols.remove(symbol);
                            }
                        }
                    }
                },
                // Symbol providers already ran in pass 1.
                Statement::AobScanModule { .. }
                | Statement::Alloc { .. }
                | Statement::GlobalAlloc { .. }
                | Statement::Define { .. }
                | Statement::RegisterSymbol(_)
                | Statement::UnregisterSymbol(_)
                | Statement::Label(_)
                | Statement::Directive(_) => {}
            }
        }
        Ok(())
    }

    /// Internal rollback. Delegates to the public
    /// [`super::rollback`] free function so a caller holding only an
    /// [`EnableOutcome`] (no live [`Engine`]) can reuse the same
    /// codepath.
    fn rollback(&mut self, outcome: &mut EnableOutcome) {
        super::rollback(&mut self.backend, outcome);
    }

    fn scan_unique(&mut self, pattern_text: &str, symbol: &str) -> Result<u64, ExecError> {
        let pat = Pattern::parse(pattern_text)?;
        let regions = self.backend.readable_regions()?;
        let mut hits: Vec<u64> = Vec::new();
        for r in regions.iter().filter(|r| is_scannable(r)) {
            // The Linux backend may return regions that show `r` in
            // /proc/<pid>/maps but produce `EFAULT` when read (kernel
            // shadow stacks, lazy file mappings whose inode is gone, …).
            // Pattern::scan via MemoryAccess.read_partial absorbs those
            // — the backend's permissive read returns empty for any
            // syscall error.
            let found = tatu_mem::pattern::scan_range(&mut self.backend, r.start, r.size(), &pat);
            hits.extend(found);
            if hits.len() > 1 {
                break;
            }
        }
        match hits.len() {
            0 => Err(ExecError::PatternNotFound {
                symbol: symbol.to_string(),
            }),
            1 => Ok(hits[0]),
            n => Err(ExecError::PatternAmbiguous {
                symbol: symbol.to_string(),
                count: n,
            }),
        }
    }
}

fn is_scannable(r: &ReadableRegion) -> bool {
    // Skip kernel-mapped virtual regions Linux refuses to let
    // `process_vm_readv` touch. The Win32 backend never reports these
    // so this filter is a no-op there.
    let path = r.path.to_string_lossy();
    r.perms.read && path != "[vvar]" && path != "[vsyscall]" && path != "[vdso]"
}

fn parse_numeric_token(token: &str) -> Option<u64> {
    let t = token.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok();
    }
    if let Some(hex) = t.strip_prefix('$') {
        return u64::from_str_radix(hex, 16).ok();
    }
    t.parse::<u64>().ok()
}
