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
use crate::persisted_hook::{PersistedAlloc, PersistedHook, PersistedWrite};
use crate::scanner::{self, Pattern};
use crate::threads::ThreadPauseError;

use nix::sys::ptrace;
use nix::sys::signal::Signal;
use nix::sys::wait::{WaitStatus, waitpid};

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
    #[error("thread pause: {0}")]
    ThreadPause(#[from] ThreadPauseError),
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

    /// Capture the undo state into a serialisable record so a future
    /// process can replay the rollback even if this `ActiveCheat`'s
    /// owning runtime has been torn down. Caller fills in `app_id`,
    /// `feature_uuid`, `exe`, and `started_at` from its own context.
    pub fn to_persisted(
        &self,
        app_id: String,
        feature_uuid: String,
        exe: String,
        started_at: Option<String>,
    ) -> PersistedHook {
        PersistedHook {
            app_id,
            feature_uuid,
            pid: self.pid.as_raw(),
            exe,
            started_at,
            writes: self
                .undo
                .iter()
                .map(|(addr, bytes)| PersistedWrite {
                    addr: *addr,
                    original: bytes.clone(),
                })
                .collect(),
            allocs: self
                .allocs
                .iter()
                .map(|(symbol, (addr, size))| PersistedAlloc {
                    symbol: symbol.clone(),
                    addr: *addr,
                    size: *size,
                })
                .collect(),
        }
    }

    /// Build an `ActiveCheat` from a persisted record. The returned
    /// cheat carries no live symbol table (recovery doesn't need it) and
    /// is immediately ready for [`Self::disable`] to walk the undo log.
    pub fn from_persisted(record: &PersistedHook) -> Self {
        let undo = record
            .writes
            .iter()
            .map(|w| (w.addr, w.original.clone()))
            .collect();
        let allocs = record
            .allocs
            .iter()
            .map(|a| (a.symbol.clone(), (a.addr, a.size)))
            .collect();
        Self {
            pid: record.pid_typed(),
            undo,
            allocs,
            symbols: HashMap::new(),
            disabled: false,
        }
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

impl ActiveCheat {
    pub fn writes(&self) -> usize {
        self.undo.len()
    }

    pub fn disable(mut self) -> Result<(), ExecError> {
        if self.disabled {
            return Ok(());
        }
        let result = rollback(&mut self);
        self.disabled = true;
        result
    }
}

impl Drop for ActiveCheat {
    fn drop(&mut self) {
        if !self.disabled && !self.undo.is_empty() {
            // Best-effort revert; we cannot return an error from Drop, but
            // we surface anything that fails so the user sees half-applied
            // hooks instead of a silent leak.
            if let Err(e) = rollback(self) {
                eprintln!(
                    "[cheat-runtime] WARNING: best-effort rollback on drop failed: {e} — the game may still carry trampoline bytes from this cheat. Re-launching the game restores the original .text."
                );
            }
        }
    }
}

/// Attach to the main thread of `pid` and wait for it to stop on SIGSTOP.
/// Returns `true` on success — the caller must `ptrace::detach` later.
/// Returns `false` if the attach is impossible (self-pid, EPERM, ESRCH);
/// the caller falls back to a non-attached write path in that case.
fn attach_main_thread(pid: Pid) -> bool {
    if pid == Pid::this() {
        return false;
    }
    if ptrace::attach(pid).is_err() {
        return false;
    }
    loop {
        match waitpid(pid, None) {
            Ok(WaitStatus::Stopped(_, Signal::SIGSTOP)) => return true,
            Ok(WaitStatus::Stopped(_, sig)) => {
                // Spurious signal arrived first — forward and keep waiting
                // for our SIGSTOP.
                if ptrace::cont(pid, sig).is_err() {
                    return false;
                }
            }
            Ok(_) | Err(_) => {
                let _ = ptrace::detach(pid, None);
                return false;
            }
        }
    }
}

/// Restore every byte the active cheat overwrote and release every
/// codecave it allocated. Mirrors the ENABLE write-pass structure: one
/// ptrace attach for the whole undo batch, POKEDATA writes under that
/// pause, detach once at the end.
///
/// Errors no longer get swallowed — the first restore failure aborts and
/// returns, leaving the remaining undo entries on the stack so a caller
/// can decide whether to retry. The dealloc loop runs regardless because
/// dealloc has its own attach lifecycle (CE-style remote `munmap`).
fn rollback(active: &mut ActiveCheat) -> Result<(), ExecError> {
    let attached = attach_main_thread(active.pid);
    let restore_result = restore_all_writes(active, attached);
    if attached {
        let _ = ptrace::detach(active.pid, None);
    }
    // Always drain allocs even if a restore failed — leaving codecaves
    // mapped in the game would compound the problem, and `dealloc_remote`
    // owns its own ptrace attach so we don't need ours.
    let mut dealloc_err: Option<ExecError> = None;
    for (_, (addr, size)) in active.allocs.drain() {
        if let Err(e) = alloc::dealloc_remote(active.pid, addr, size) {
            // Keep the first error; subsequent ones are likely the same
            // root cause (e.g. game already exited).
            if dealloc_err.is_none() {
                dealloc_err = Some(ExecError::Alloc(e));
            }
        }
    }
    restore_result
        .or_else(|e| Err(e))
        .and_then(|()| match dealloc_err {
            Some(e) => Err(e),
            None => Ok(()),
        })
}

fn restore_all_writes(active: &mut ActiveCheat, attached: bool) -> Result<(), ExecError> {
    while let Some((addr, bytes)) = active.undo.pop() {
        let result = if attached {
            memory::write_bytes_attached(active.pid, addr, &bytes)
        } else {
            memory::write_bytes(active.pid, addr, &bytes)
        };
        if let Err(e) = result {
            // Push the entry back so a retry can resume from here.
            active.undo.push((addr, bytes));
            return Err(ExecError::Memory(e));
        }
    }
    Ok(())
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
