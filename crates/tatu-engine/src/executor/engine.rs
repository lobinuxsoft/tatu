//! The two-pass executor that walks a parsed [`crate::parser::Script`]
//! and applies it through a [`Backend`]. Produces an [`EnableOutcome`]
//! POD; the platform-specific wrapper (`cheat-runtime::ActiveCheat`
//! for Linux, the bridge's response payload for Win32) takes
//! ownership of the rollback from there.

use std::collections::HashMap;

use tatu_mem::pattern::Pattern;

use crate::asm;
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
                }
                | Statement::AobScan { symbol, pattern } => {
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
                Statement::SymbolSite { symbol, offset } => {
                    // The base symbol is bound earlier this pass (aobscanmodule
                    // / registersymbol). Unlike LabelSite we never *define* a
                    // symbol here — an unresolved base just leaves the cursor
                    // alone; write_pass surfaces UnknownSymbol if it never binds.
                    if let Some(base) = self.symbols.get(symbol).copied() {
                        cursor = Some(base.wrapping_add(*offset as u64));
                    }
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
                Statement::Reassemble(operand) => {
                    // Re-encoded length depends on live target bytes and the
                    // destination address (short→near branch promotion), so
                    // compute the real bytes at the current cursor and advance
                    // by their length. Skipped when no cursor is set — pass 2
                    // surfaces the orphan-write error.
                    if let Some(c) = cursor {
                        let bytes = self.reassemble_bytes(operand, c)?;
                        cursor = Some(c.wrapping_add(bytes.len() as u64));
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
                Statement::SymbolSite { symbol, offset } => {
                    let base = *self
                        .symbols
                        .get(symbol)
                        .ok_or_else(|| ExecError::UnknownSymbol(symbol.clone()))?;
                    cursor = Some(base.wrapping_add(*offset as u64));
                }
                Statement::Raw(line) => {
                    let Some(base) = cursor else {
                        return Err(ExecError::OrphanWrite(line.clone()));
                    };
                    let bytes = compile_raw(line, &self.symbols, &mut self.backend, base)?;
                    cursor = Some(self.emit(base, &bytes, line, outcome)?);
                }
                Statement::Reassemble(operand) => {
                    let Some(base) = cursor else {
                        return Err(ExecError::OrphanWrite(format!("reassemble({operand})")));
                    };
                    let bytes = self.reassemble_bytes(operand, base)?;
                    cursor = Some(self.emit(base, &bytes, operand, outcome)?);
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
                | Statement::AobScan { .. }
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

    /// Write `bytes` at `base`, recording the overwritten bytes in the undo
    /// log and returning the advanced cursor. Shared by the `Raw` and
    /// `Reassemble` arms of [`Self::write_pass`]. `label` is the source line
    /// (or a synthesised `reassemble(...)` tag) used only for the trace log.
    fn emit(
        &mut self,
        base: u64,
        bytes: &[u8],
        label: &str,
        outcome: &mut EnableOutcome,
    ) -> Result<u64, ExecError> {
        let original = self.backend.read(base, bytes.len())?;
        if std::env::var_os("CHEAT_RUNTIME_TRACE").is_some() {
            eprintln!(
                "[trace] @0x{:x} write {:>2}B {:02X?}  was {:02X?}  ← {}",
                base,
                bytes.len(),
                bytes,
                original,
                label
            );
        }
        self.backend.write(base, bytes)?;
        self.backend.flush_instruction_cache(base, bytes.len())?;
        outcome.undo.push((base, original));
        Ok(base + bytes.len() as u64)
    }

    /// Resolve a `reassemble(operand)` address, read the live instruction at
    /// it (x86-64 max 15 bytes), and re-encode it for execution at `dest`.
    fn reassemble_bytes(&mut self, operand: &str, dest: u64) -> Result<Vec<u8>, ExecError> {
        let src = asm::resolve_address(operand, &self.symbols)?;
        let code = self.backend.read(src, 15)?;
        Ok(asm::reassemble_instruction(&code, src, dest)?)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::BackendError;
    use crate::parser::parse;
    use std::collections::BTreeMap;
    use tatu_mem::MemoryAccess;

    /// Sparse byte-addressed memory mock. Unset bytes read as `0`; `alloc`
    /// hands out a codecave 1 MiB above the `near` hint (or a fixed base) so
    /// reassembled `rel32` branches and rip-relative operands stay in range.
    struct MockBackend {
        mem: BTreeMap<u64, u8>,
        next_alloc: u64,
    }

    impl MockBackend {
        fn new() -> Self {
            Self {
                mem: BTreeMap::new(),
                next_alloc: 0,
            }
        }
        fn put(&mut self, addr: u64, bytes: &[u8]) {
            for (i, b) in bytes.iter().enumerate() {
                self.mem.insert(addr + i as u64, *b);
            }
        }
    }

    impl MemoryAccess for MockBackend {
        type Error = BackendError;
        fn read(&mut self, addr: u64, len: usize) -> Result<Vec<u8>, BackendError> {
            Ok((0..len as u64)
                .map(|i| *self.mem.get(&(addr + i)).unwrap_or(&0))
                .collect())
        }
        fn read_partial(&mut self, addr: u64, len: usize) -> Vec<u8> {
            self.read(addr, len).unwrap()
        }
        fn write(&mut self, addr: u64, bytes: &[u8]) -> Result<(), BackendError> {
            self.put(addr, bytes);
            Ok(())
        }
    }

    impl Backend for MockBackend {
        fn alloc(&mut self, size: usize, near: Option<u64>) -> Result<u64, BackendError> {
            let base = near.unwrap_or(0x1_4000_0000).wrapping_add(0x10_0000);
            let addr = base + self.next_alloc;
            self.next_alloc += size as u64;
            Ok(addr)
        }
        fn dealloc(&mut self, _addr: u64, _size: usize) -> Result<(), BackendError> {
            Ok(())
        }
        fn readable_regions(&mut self) -> Result<Vec<ReadableRegion>, BackendError> {
            Ok(Vec::new())
        }
        fn attach(&mut self) -> bool {
            false
        }
        fn detach(&mut self) {}
    }

    /// End-to-end: a codecave that replays a displaced `jne rel8` via
    /// `reassemble`. Pass 1 must size the re-encoded instruction (6 bytes,
    /// rel8→rel32) so the label after it resolves correctly; pass 2 must write
    /// those bytes with the absolute branch target preserved. This mirrors the
    /// DD2 Fatal Fall Height codecave body.
    #[test]
    fn reassemble_two_pass_sizes_and_writes() {
        let mut backend = MockBackend::new();
        let hook = 0x1_4000_0000_u64;
        // hook+0: mov rax,[rdx+10] (4B); hook+4: jne $+8 (75 06).
        backend.put(hook, &[0x48, 0x8B, 0x42, 0x10, 0x75, 0x06]);
        let mut eng = Engine::new(backend);
        eng.bind_symbol("hook", hook);

        let script = parse(concat!(
            "[ENABLE]\n",
            "alloc(cave,0x100)\n",
            "label(after)\n",
            "cave:\n",
            "reassemble(hook+4)\n",
            "after:\n",
            "nop\n",
            "[DISABLE]\n",
        ))
        .unwrap();
        let outcome = eng.enable(&script).unwrap();

        let cave = outcome.symbols["cave"];
        let after = outcome.symbols["after"];
        // rel8 promoted to rel32 ⇒ 6 bytes ⇒ the label lands at cave+6.
        assert_eq!(after, cave + 6, "pass-1 must size the reassembled branch");

        let written = eng.backend_mut().read(cave, 6).unwrap();
        assert_eq!(&written[..2], &[0x0F, 0x85], "jne rel32 opcode");
        let rel = i32::from_le_bytes(written[2..6].try_into().unwrap()) as i64;
        let absolute = (cave as i64 + 6 + rel) as u64;
        // Original target: jne at hook+4, rel8 +6 ⇒ hook+4+2+6.
        assert_eq!(
            absolute,
            hook + 4 + 2 + 6,
            "branch target must survive relocation"
        );
        // The trailing `nop` proves the cursor advanced past the reassembly.
        assert_eq!(eng.backend_mut().read(cave + 6, 1).unwrap(), vec![0x90]);
    }
}
