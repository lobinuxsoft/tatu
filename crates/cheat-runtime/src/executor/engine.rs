//! The two-pass executor that walks a parsed [`Script`] and applies it to a
//! live process. Owns the symbol table; produces an [`ActiveCheat`] on
//! success, rolls back atomically on failure.

use std::collections::HashMap;

use nix::sys::ptrace;
use nix::unistd::Pid;

use crate::alloc;
use crate::maps::{MemoryRegion, read_maps};
use crate::memory::{self, RuntimeError};
use crate::parser::{Script, Statement};
use crate::scanner::{self, Pattern};

use super::active::ActiveCheat;
use super::error::ExecError;
use super::length::estimate_raw_length;
use super::raw_compiler::compile_raw;
use super::rollback::{attach_main_thread, rollback};

/// A live execution context bound to a target PID. Holds the symbol table
/// built up by `aobscanmodule` resolutions and any other label registrations.
pub struct Engine {
    pid: Pid,
    symbols: HashMap<String, u64>,
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
        let mut active = ActiveCheat::new(self.pid);

        if let Err(e) = self.pre_resolve_symbols(script, &mut active) {
            let _ = rollback(&mut active);
            return Err(e);
        }
        // Attach to the main thread ONCE for the duration of the write
        // pass. This mirrors CE Linux's autoassembler.pas:4116 dance:
        // `ntsuspendProcess(processhandle)` / ceserver pauseProcess wraps
        // the entire batch of writes, so the target never observes a
        // half-applied trampoline (newmem populated but pBase still
        // pristine, or pBase patched while newmem is half-written).
        //
        // Inside the attach window we use PTRACE_POKEDATA for every byte,
        // which bypasses page protections — no `mprotect` round-trip for
        // .text writes, and the writes are atomic relative to the paused
        // main thread.
        //
        // Self-PID (tests) skips the attach: Linux refuses ptrace-on-self
        // with EPERM and we fall back to process_vm_writev for in-process
        // smoke tests.
        let attached = attach_main_thread(self.pid);
        let write_result = self.write_pass(script, &mut active, attached);
        if attached {
            let _ = ptrace::detach(self.pid, None);
        }
        if let Err(e) = write_result {
            let _ = rollback(&mut active);
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
                    // Explicit `near` propagates as the mmap hint. Without
                    // it `alloc_remote` falls back to MAP_32BIT so
                    // disp32-encoded `mov [imm], reg` stays legal.
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
    /// `attached == true` means the caller has the main thread ptrace-stopped
    /// and writes go through PTRACE_POKEDATA (no page-perm dance, atomic
    /// against the paused thread). `attached == false` falls back to
    /// process_vm_writev for self-pid test scenarios.
    fn write_pass(
        &mut self,
        script: &Script,
        active: &mut ActiveCheat,
        attached: bool,
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
                    let bytes = compile_raw(line, &self.symbols, self.pid, base)?;
                    let original = memory::read_bytes(self.pid, base, bytes.len())?;
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
                    if attached {
                        memory::write_bytes_attached(self.pid, base, &bytes)?;
                    } else {
                        memory::write_bytes(self.pid, base, &bytes)?;
                    }
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
