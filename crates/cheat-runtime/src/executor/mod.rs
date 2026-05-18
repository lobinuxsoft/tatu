//! Executor that walks a parsed [`Script`] and applies it to a live process.
//!
//! Scope (matches issue #64's out-of-scope list):
//! - **Supported**: `aobscanmodule`, `registersymbol`, `unregistersymbol`,
//!   `label`, label sites (`name:`), and write directives following a label
//!   site (`db`, `dq`, `nop N`, `readmem(symbol, len)`).
//! - **Unsupported (returns [`ExecError::Unsupported`])**: `alloc`, `dealloc`,
//!   inline assembly mnemonics (`push`, `mov`, `jmp`, …), and any other
//!   `Statement::Raw` line we don't recognise.
//!
//! Atomicity: [`Engine::enable`] keeps an undo log of every byte sequence
//! it overwrites. If any later statement fails, every previously applied
//! write is reverted before returning the error — there is no partial state.
//!
//! [`ActiveCheat::disable`] reverts the same writes in reverse order. After
//! disable the target process's memory is byte-for-byte identical to what
//! it was before [`Engine::enable`] was called.

mod length;
mod raw_compiler;

use std::collections::HashMap;

use nix::unistd::Pid;

use crate::alloc::{self, AllocError};
use crate::asm::AsmError;
use crate::maps::{MemoryRegion, read_maps};
use crate::memory::{self, RuntimeError};
use crate::parser::{Script, Statement};
use crate::scanner::{self, Pattern};

use self::length::estimate_raw_length;
use self::raw_compiler::compile_raw;

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("pattern parse error in aobscanmodule: {0}")]
    Pattern(#[from] scanner::ParseError),
    #[error("memory io: {0}")]
    Memory(#[from] RuntimeError),
    #[error("remote alloc: {0}")]
    Alloc(#[from] AllocError),
    #[error("asm compile: {0}")]
    Asm(#[from] AsmError),
    #[error("aobscanmodule({symbol}): no match in any executable region")]
    PatternNotFound { symbol: String },
    #[error("aobscanmodule({symbol}): {count} matches found, pattern must be unique")]
    PatternAmbiguous { symbol: String, count: usize },
    #[error("unknown symbol {0:?}")]
    UnknownSymbol(String),
    #[error("dealloc({0:?}) before matching alloc — symbol not in active region table")]
    DeallocUnknown(String),
    #[error("write outside any label site: {0:?}")]
    OrphanWrite(String),
    #[error("unsupported statement: {0}")]
    Unsupported(String),
}

/// A live execution context bound to a target PID. Holds the symbol table
/// built up by `aobscanmodule` resolutions and any other label registrations.
pub struct Engine {
    pid: Pid,
    symbols: HashMap<String, u64>,
}

/// A successfully enabled cheat. Owns the undo log; calling [`Self::disable`]
/// (or dropping after a deliberate `forget`) reverts every byte the executor
/// wrote during ENABLE.
#[derive(Debug)]
#[must_use = "ActiveCheat owns the undo log; call .disable() or it will roll back on drop"]
pub struct ActiveCheat {
    pid: Pid,
    undo: Vec<(u64, Vec<u8>)>,
    /// Pairs of `(remote_address, size)` that were allocated via `Statement::Alloc`
    /// and need to be released with `munmap` when the cheat is disabled. Keyed
    /// by the AA symbol name so `Statement::Dealloc(symbol)` can find them.
    allocs: HashMap<String, (u64, usize)>,
    /// Snapshot of every symbol the Engine bound during ENABLE — both AOB-scan
    /// results and allocs. Kept on the live cheat so downstream features
    /// (pointer-chain `Value` reads in particular) can look up
    /// `base_address` / `shop` / etc. while the master toggle is active. Goes
    /// stale on `.disable()`; the registry should drop the entry then.
    symbols: HashMap<String, u64>,
    disabled: bool,
}

impl ActiveCheat {
    /// Read-only view of the symbol table this cheat established. The map
    /// is a snapshot taken at ENABLE time — it doesn't update if the game
    /// re-locates code afterwards (rare for AOB-scan-bound symbols, which
    /// pin to fixed module offsets) but a re-enable will refresh it.
    pub fn symbols(&self) -> &HashMap<String, u64> {
        &self.symbols
    }

    pub fn pid(&self) -> Pid {
        self.pid
    }
}

impl Engine {
    pub fn new(pid: Pid) -> Self {
        Self {
            pid,
            symbols: HashMap::new(),
        }
    }

    pub fn pid(&self) -> Pid {
        self.pid
    }

    pub fn symbols(&self) -> &HashMap<String, u64> {
        &self.symbols
    }

    /// Programmatically bind a symbol to an address. Useful for tests, for
    /// loaders that resolve symbols outside the AOB-scan path (e.g. from a
    /// manifest), and for layered scripts that share a common scan table.
    pub fn bind_symbol(&mut self, name: impl Into<String>, addr: u64) {
        self.symbols.insert(name.into(), addr);
    }

    /// Walk `script.enable` and apply every supported statement.
    ///
    /// Two-pass evaluation. Pass 1 runs the side-effecting symbol providers
    /// (`aobscanmodule`, `alloc`) and walks a virtual cursor through the rest
    /// of the script to bind every `LabelSite` to its address — that's what
    /// makes forward references like `jmp return` work when `return:` is
    /// declared further down. Pass 2 walks again and emits the actual bytes
    /// with the now-complete symbol table.
    ///
    /// On any error, every side effect already applied (writes + allocs) is
    /// reverted before returning. The returned [`ActiveCheat`] can later be
    /// `.disable()`d to undo the writes on demand.
    pub fn enable(&mut self, script: &Script) -> Result<ActiveCheat, ExecError> {
        let mut active = ActiveCheat {
            pid: self.pid,
            undo: Vec::new(),
            allocs: HashMap::new(),
            symbols: HashMap::new(),
            disabled: false,
        };

        if let Err(e) = self.pre_resolve_symbols(script, &mut active) {
            rollback(&mut active);
            return Err(e);
        }
        if let Err(e) = self.write_pass(script, &mut active) {
            rollback(&mut active);
            return Err(e);
        }
        active.symbols = self.symbols.clone();
        Ok(active)
    }

    /// Pass 1: bind every symbol the script references before pass 2 writes.
    fn pre_resolve_symbols(
        &mut self,
        script: &Script,
        active: &mut ActiveCheat,
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
                    let addr = alloc::alloc_remote(self.pid, *size as usize, None, hint)?;
                    self.symbols.insert(symbol.clone(), addr);
                    active.allocs.insert(symbol.clone(), (addr, *size as usize));
                }
                Statement::LabelSite(name) => {
                    if let Some(addr) = self.symbols.get(name).copied() {
                        cursor = Some(addr);
                    } else if let Some(c) = cursor {
                        // Forward label declared inside an alloc / aobscan region.
                        self.symbols.insert(name.clone(), c);
                    }
                    // Else: still unbound; pass 2 will emit OrphanWrite / UnknownSymbol.
                }
                Statement::AbsoluteSite(addr) => {
                    cursor = Some(*addr);
                }
                Statement::Raw(line) => {
                    if let Some(c) = cursor.as_mut() {
                        let len = estimate_raw_length(line, &self.symbols).ok_or_else(|| {
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
    fn write_pass(&mut self, script: &Script, active: &mut ActiveCheat) -> Result<(), ExecError> {
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
                    let bytes = compile_raw(line, &self.symbols, self.pid, base)?;
                    let original = memory::read_bytes(self.pid, base, bytes.len())?;
                    memory::write_bytes(self.pid, base, &bytes)?;
                    active.undo.push((base, original));
                    cursor = Some(base + bytes.len() as u64);
                }
                Statement::Dealloc(symbol) => {
                    let (addr, size) = active
                        .allocs
                        .remove(symbol)
                        .ok_or_else(|| ExecError::DeallocUnknown(symbol.clone()))?;
                    alloc::dealloc_remote(self.pid, addr, size)?;
                    self.symbols.remove(symbol);
                }
                // Symbol providers already ran in pass 1.
                Statement::AobScanModule { .. }
                | Statement::Alloc { .. }
                | Statement::RegisterSymbol(_)
                | Statement::UnregisterSymbol(_)
                | Statement::Label(_)
                | Statement::Directive(_) => {}
            }
        }
        Ok(())
    }

    fn scan_unique(&self, pattern_text: &str, symbol: &str) -> Result<u64, ExecError> {
        let pat = Pattern::parse(pattern_text)?;
        let regions = read_maps(self.pid).map_err(|e| ExecError::Memory(RuntimeError::Io(e)))?;
        let mut hits: Vec<u64> = Vec::new();
        for r in regions.iter().filter(|r| is_scannable(r)) {
            // Some regions show `r` in /proc/<pid>/maps but `process_vm_readv`
            // still returns EFAULT (kernel-managed shadow stacks, lazy file
            // mappings whose backing inode is gone, etc.). CE silently skips
            // these and so do we — a single unreadable region must not abort
            // the whole scan.
            let found = match scanner::scan_in_process(self.pid, r, &pat) {
                Ok(v) => v,
                Err(RuntimeError::Nix(nix::errno::Errno::EFAULT)) => continue,
                Err(e) => return Err(ExecError::Memory(e)),
            };
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

fn is_scannable(r: &MemoryRegion) -> bool {
    // Accept every readable region except the kernel-mapped virtual ranges,
    // which the kernel forbids `process_vm_readv` from touching. This matches
    // CE's plain `aobscan` semantics; resolving `$process` to *just the main
    // module* (the stricter `aobscanmodule` semantics) is deferred until the
    // executor learns to identify which mapping is the main exe.
    let path = r.path.to_string_lossy();
    r.perms.read && path != "[vvar]" && path != "[vsyscall]" && path != "[vdso]"
}

impl ActiveCheat {
    pub fn writes(&self) -> usize {
        self.undo.len()
    }

    pub fn disable(mut self) -> Result<(), ExecError> {
        if self.disabled {
            return Ok(());
        }
        rollback(&mut self);
        self.disabled = true;
        Ok(())
    }
}

impl Drop for ActiveCheat {
    fn drop(&mut self) {
        if !self.disabled && !self.undo.is_empty() {
            // Best-effort revert; we cannot return an error from Drop.
            rollback(self);
        }
    }
}

fn rollback(active: &mut ActiveCheat) {
    while let Some((addr, bytes)) = active.undo.pop() {
        let _ = memory::write_bytes(active.pid, addr, &bytes);
    }
    // Release any codecaves we allocated. Best-effort: a failure here would
    // leak ~pages in the target, not a correctness issue for the host. The
    // map is drained so a second rollback is a no-op.
    for (_, (addr, size)) in active.allocs.drain() {
        let _ = alloc::dealloc_remote(active.pid, addr, size);
    }
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
mod tests;
