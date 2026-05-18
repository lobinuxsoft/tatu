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

use std::collections::HashMap;

use nix::unistd::Pid;

use crate::alloc::{self, AllocError};
use crate::asm::{self, AsmError};
use crate::maps::{MemoryRegion, read_maps};
use crate::memory::{self, RuntimeError};
use crate::parser::{Script, Statement};
use crate::scanner::{self, Pattern};

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
    disabled: bool,
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

/// Compile a raw assembler line to the bytes that should be written at the
/// current cursor. Supports `db`, `dq`, `nop N`, `readmem(symbol, len)`, plus
/// the asm subset covered by [`crate::asm::compile_line`] (`jmp`, `call`,
/// `ret`). `base` is the absolute target address — required for rip-relative
/// encodings.
fn compile_raw(
    line: &str,
    symbols: &HashMap<String, u64>,
    pid: Pid,
    base: u64,
) -> Result<Vec<u8>, ExecError> {
    let trimmed = line.trim();
    if let Some(rest) = trimmed
        .strip_prefix("db ")
        .or_else(|| trimmed.strip_prefix("db\t"))
    {
        return parse_byte_list(rest)
            .ok_or_else(|| ExecError::Unsupported(format!("db with bad bytes: {line:?}")));
    }
    if let Some(rest) = trimmed
        .strip_prefix("dq ")
        .or_else(|| trimmed.strip_prefix("dq\t"))
    {
        return parse_dq(rest, symbols)
            .ok_or_else(|| ExecError::Unsupported(format!("dq with bad operand: {line:?}")));
    }
    if let Some(rest) = trimmed
        .strip_prefix("nop ")
        .or_else(|| trimmed.strip_prefix("nop\t"))
    {
        return rest
            .trim()
            .parse::<usize>()
            .ok()
            .map(|n| vec![0x90; n])
            .ok_or_else(|| ExecError::Unsupported(format!("nop with bad count: {line:?}")));
    }
    if trimmed == "nop" {
        return Ok(vec![0x90]);
    }
    if let Some(args) = trimmed
        .strip_prefix("readmem(")
        .and_then(|s| s.strip_suffix(')'))
    {
        return readmem_bytes(args, symbols, pid);
    }
    // Flatten `AsmError::Unsupported` (a known-mnemonic-but-unsupported-form
    // signal, e.g. `mov dword ptr [r13+13C], (float)100` until Phase B v2.2)
    // into the executor's own Unsupported variant so callers / tests only
    // need to match one shape regardless of which layer rejected the line.
    match asm::compile_line(trimmed, symbols, base) {
        Ok(Some(bytes)) => Ok(bytes),
        Ok(None) => Err(ExecError::Unsupported(format!(
            "asm/raw not supported: {line:?}"
        ))),
        Err(asm::AsmError::Unsupported(detail)) => Err(ExecError::Unsupported(detail)),
        Err(e) => Err(ExecError::Asm(e)),
    }
}

/// Predict the byte length a `Statement::Raw` line will produce in pass 2.
///
/// Used by [`Engine::pre_resolve_symbols`] to advance the virtual cursor
/// without committing writes. Length is exact for the constant-encoding
/// directives (`db`, `dq`, `nop N`, `readmem(s, N)`); for the asm subset
/// covered by [`crate::asm`] the estimator delegates to a speculative
/// compile that substitutes `0` for any still-unresolved symbol, so a
/// forward reference like `jmp return` (where `return:` is declared later)
/// returns the correct 5 bytes before the symbol is bound. Returns `None`
/// for anything we cannot size; pass 2 surfaces the clear `Unsupported`
/// error.
fn estimate_raw_length(line: &str, symbols: &HashMap<String, u64>) -> Option<usize> {
    let trimmed = line.trim();
    if let Some(rest) = trimmed
        .strip_prefix("db ")
        .or_else(|| trimmed.strip_prefix("db\t"))
    {
        return Some(rest.split_whitespace().count());
    }
    if trimmed.starts_with("dq ") || trimmed.starts_with("dq\t") {
        return Some(8);
    }
    if let Some(rest) = trimmed
        .strip_prefix("nop ")
        .or_else(|| trimmed.strip_prefix("nop\t"))
    {
        return rest.trim().parse::<usize>().ok();
    }
    if trimmed == "nop" {
        return Some(1);
    }
    if let Some(args) = trimmed
        .strip_prefix("readmem(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let parts: Vec<&str> = args.split(',').map(str::trim).collect();
        if parts.len() == 2 {
            return parts[1].parse::<usize>().ok();
        }
        return None;
    }

    // Asm line: speculatively compile. Unresolved symbols (forward refs)
    // get a temporary placeholder address. We use `0x10000` (not zero) so
    // iced-x86 does not pick a short-form `rel8` encoding for a `jmp 0`
    // self-loop — pass 2's real address will be a far page-aligned vaddr,
    // which always wants `rel32`. Choosing the placeholder to match the
    // real form is what keeps pass-1 length predictions accurate.
    const FAR_PLACEHOLDER: u64 = 0x10000;
    let mut perm = symbols.clone();
    for _ in 0..16 {
        match asm::compile_line(trimmed, &perm, 0) {
            Ok(Some(bytes)) => return Some(bytes.len()),
            Ok(None) => return None,
            Err(asm::AsmError::UnknownSymbol(name)) => {
                perm.insert(name, FAR_PLACEHOLDER);
            }
            Err(_) => return None,
        }
    }
    None
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

fn parse_byte_list(rest: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for tok in rest.split_whitespace() {
        if tok.len() != 2 || !tok.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        out.push(u8::from_str_radix(tok, 16).ok()?);
    }
    if out.is_empty() { None } else { Some(out) }
}

fn parse_dq(operand: &str, symbols: &HashMap<String, u64>) -> Option<Vec<u8>> {
    let t = operand.trim();
    if let Some(addr) = symbols.get(t) {
        return Some(addr.to_le_bytes().to_vec());
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return Some(u64::from_str_radix(hex, 16).ok()?.to_le_bytes().to_vec());
    }
    if let Some(hex) = t.strip_prefix('$') {
        return Some(u64::from_str_radix(hex, 16).ok()?.to_le_bytes().to_vec());
    }
    Some(t.parse::<u64>().ok()?.to_le_bytes().to_vec())
}

fn readmem_bytes(
    args: &str,
    symbols: &HashMap<String, u64>,
    pid: Pid,
) -> Result<Vec<u8>, ExecError> {
    let parts: Vec<&str> = args.split(',').map(str::trim).collect();
    if parts.len() != 2 {
        return Err(ExecError::Unsupported(format!(
            "readmem expects 2 args, got {}",
            parts.len()
        )));
    }
    let addr = *symbols
        .get(parts[0])
        .ok_or_else(|| ExecError::UnknownSymbol(parts[0].to_string()))?;
    let len: usize = parts[1]
        .parse()
        .map_err(|_| ExecError::Unsupported(format!("readmem with bad len: {:?}", parts[1])))?;
    let bytes = memory::read_bytes(pid, addr, len)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn engine_for_self() -> Engine {
        Engine::new(Pid::this())
    }

    #[test]
    fn enable_with_only_noops_creates_empty_active() {
        let script =
            parse("[ENABLE]\nregistersymbol(foo)\nlabel(bar)\n{$lua}\n[DISABLE]\n").unwrap();
        let mut eng = engine_for_self();
        // We can't actually scan/write without a real symbol; this only checks
        // that noop statements pass through cleanly.
        let active = eng.enable(&script).expect("noop enable should succeed");
        assert_eq!(active.writes(), 0);
        active.disable().unwrap();
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn alloc_enable_then_disable_round_trips_codecave() {
        use nix::sys::signal::{Signal, kill};
        use nix::sys::wait::waitpid;
        use nix::unistd::{ForkResult, fork};
        use std::time::Duration;

        // Linux refuses ptrace-self. Fork a sleeping child and run the
        // executor against it.
        match unsafe { fork() }.expect("fork") {
            ForkResult::Child => {
                std::thread::sleep(Duration::from_secs(5));
                std::process::exit(0);
            }
            ForkResult::Parent { child } => {
                std::thread::sleep(Duration::from_millis(150));

                let script =
                    parse("[ENABLE]\nalloc(codecave,4096)\n[DISABLE]\ndealloc(codecave)\n")
                        .unwrap();
                let mut eng = Engine::new(child);
                let active = eng.enable(&script).expect("alloc must succeed");
                let codecave_addr = *eng.symbols().get("codecave").expect("codecave bound");
                assert!(codecave_addr != 0);
                assert!(codecave_addr & 0xfff == 0);
                active.disable().unwrap();

                let _ = kill(child, Signal::SIGKILL);
                let _ = waitpid(child, None);
            }
        }
    }

    #[test]
    fn dealloc_of_unknown_symbol_errors() {
        let script = parse("[ENABLE]\ndealloc(missing)\n[DISABLE]\n").unwrap();
        let mut eng = engine_for_self();
        let err = eng.enable(&script).unwrap_err();
        assert!(matches!(err, ExecError::DeallocUnknown(name) if name == "missing"));
    }

    #[test]
    fn orphan_raw_write_errors() {
        let script = parse("[ENABLE]\ndb DE AD BE EF\n[DISABLE]\n").unwrap();
        let mut eng = engine_for_self();
        let err = eng.enable(&script).unwrap_err();
        assert!(matches!(
            err,
            ExecError::Unsupported(_) | ExecError::OrphanWrite(_)
        ));
    }

    #[test]
    fn parse_byte_list_works() {
        assert_eq!(
            parse_byte_list("DE AD BE EF"),
            Some(vec![0xde, 0xad, 0xbe, 0xef])
        );
        assert_eq!(parse_byte_list("CA fe"), Some(vec![0xca, 0xfe]));
        assert_eq!(parse_byte_list("DE A"), None); // odd-length token
        assert_eq!(parse_byte_list("ZZ"), None);
        assert_eq!(parse_byte_list(""), None);
    }

    #[test]
    fn parse_dq_decimal_hex_and_symbol() {
        let mut syms = HashMap::new();
        syms.insert("foo".to_string(), 0x1234_5678_9abc_def0_u64);
        assert_eq!(parse_dq("0", &syms), Some(0u64.to_le_bytes().to_vec()));
        assert_eq!(
            parse_dq("0xdeadbeef", &syms),
            Some(0xdeadbeefu64.to_le_bytes().to_vec())
        );
        assert_eq!(
            parse_dq("foo", &syms),
            Some(0x1234_5678_9abc_def0_u64.to_le_bytes().to_vec())
        );
        assert_eq!(
            parse_dq("$ABC", &syms),
            Some(0xABCu64.to_le_bytes().to_vec())
        );
    }

    #[test]
    fn compile_raw_db_dq_nop_succeeds() {
        let syms = HashMap::new();
        let pid = Pid::this();
        assert_eq!(
            compile_raw("db 90 90 90", &syms, pid, 0).unwrap(),
            vec![0x90; 3]
        );
        assert_eq!(compile_raw("dq 0", &syms, pid, 0).unwrap(), vec![0u8; 8]);
        assert_eq!(compile_raw("nop 5", &syms, pid, 0).unwrap(), vec![0x90; 5]);
        assert_eq!(compile_raw("nop", &syms, pid, 0).unwrap(), vec![0x90]);
    }

    #[test]
    fn compile_raw_jmp_via_asm_module() {
        let syms = HashMap::new();
        let bytes = compile_raw("jmp 0x1000", &syms, Pid::this(), 0x500).unwrap();
        assert_eq!(bytes, vec![0xE9, 0xFB, 0x0A, 0x00, 0x00]);
    }

    #[test]
    fn compile_raw_rejects_unsupported_asm() {
        let syms = HashMap::new();
        let pid = Pid::this();
        // Anything outside the Phase B subset (`imul`, AVX, MMX, etc.) must
        // surface Unsupported so the user sees what's missing.
        assert!(matches!(
            compile_raw("imul rax, rbx, 4", &syms, pid, 0),
            Err(ExecError::Unsupported(_))
        ));
        assert!(matches!(
            compile_raw("vmovups ymm0, ymm1", &syms, pid, 0),
            Err(ExecError::Unsupported(_))
        ));
    }

    /// End-to-end: orchestration of LabelSite + `db` overwrite + DISABLE
    /// rollback. Pre-binds the symbol address (the `aobscanmodule` path is
    /// already covered by the scanner crate's own tests; here we exercise
    /// the executor's enable/disable orchestration in isolation, which is
    /// reliable even when the test process's heap holds other copies of
    /// arbitrary byte patterns).
    #[test]
    fn enable_then_disable_roundtrips_a_byte_overwrite() {
        let mut victim = [0u8; 64];
        let original: [u8; 16] = [
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE,
            0xFF, 0x00,
        ];
        victim[16..32].copy_from_slice(&original);
        let target_addr = victim.as_ptr() as u64 + 16;
        let orig_hex = original
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ");

        let script_src = format!(
            "[ENABLE]\n\
             registersymbol(victim)\n\
             victim:\n\
             db 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00\n\
             [DISABLE]\n\
             victim:\n\
             db {orig_hex}\n\
             unregistersymbol(victim)\n",
        );
        let script = parse(&script_src).unwrap();

        let mut eng = Engine::new(Pid::this());
        eng.bind_symbol("victim", target_addr);

        let active = eng.enable(&script).expect("enable must succeed");
        assert_eq!(active.writes(), 1);

        // After ENABLE the bytes are zeroed.
        assert_eq!(&victim[16..32], &[0u8; 16]);

        active.disable().unwrap();

        // After DISABLE the original bytes are restored.
        assert_eq!(&victim[16..32], &original);
    }

    /// AbsoluteSite parity with LabelSite: the migrator emits numeric label
    /// sites (CE's `0xADDR:` form) and the executor must apply writes the
    /// same way as it does for symbolic sites resolved via aobscanmodule.
    #[test]
    fn absolute_site_roundtrips_a_byte_overwrite() {
        let mut victim = [0u8; 16];
        let original = [0xCA, 0xFE, 0xBA, 0xBE];
        victim[4..8].copy_from_slice(&original);
        let target_addr = victim.as_ptr() as u64 + 4;

        let script_src = format!("[ENABLE]\n0x{target_addr:X}:\ndb 11 22 33 44\n[DISABLE]\n");
        let script = parse(&script_src).unwrap();

        let mut eng = Engine::new(Pid::this());
        let active = eng
            .enable(&script)
            .expect("absolute-site enable must succeed");
        assert_eq!(active.writes(), 1);
        assert_eq!(&victim[4..8], &[0x11, 0x22, 0x33, 0x44]);

        active.disable().unwrap();
        assert_eq!(&victim[4..8], &original);
    }

    /// Atomicity: a failing later statement must roll back the writes that
    /// the earlier statements applied.
    #[test]
    fn failed_statement_rolls_back_prior_writes() {
        let mut victim = [0u8; 32];
        let original: [u8; 8] = [0xCA, 0xFE, 0xBA, 0xBE, 0xDE, 0xAD, 0xBE, 0xEF];
        victim[8..16].copy_from_slice(&original);
        let target_addr = victim.as_ptr() as u64 + 8;

        // First write zeros, then trigger an Unsupported asm line — must rollback.
        // `imul rax, rbx, 4` is outside the Phase B subset.
        let script_src = "[ENABLE]\n\
             registersymbol(victim)\n\
             victim:\n\
             db 00 00 00 00 00 00 00 00\n\
             imul rax, rbx, 4\n\
             [DISABLE]\n";
        let script = parse(script_src).unwrap();

        let mut eng = Engine::new(Pid::this());
        eng.bind_symbol("victim", target_addr);

        let err = eng.enable(&script).unwrap_err();
        assert!(matches!(err, ExecError::Unsupported(_)));
        // Crucially, the previously-applied write was reverted.
        assert_eq!(&victim[8..16], &original);
    }

    #[test]
    fn estimate_raw_length_covers_db_dq_nop_readmem_and_asm() {
        let empty: HashMap<String, u64> = HashMap::new();
        assert_eq!(estimate_raw_length("db 90 90 90", &empty), Some(3));
        assert_eq!(estimate_raw_length("dq 0", &empty), Some(8));
        assert_eq!(estimate_raw_length("nop 5", &empty), Some(5));
        assert_eq!(estimate_raw_length("nop", &empty), Some(1));
        assert_eq!(estimate_raw_length("readmem(orig, 8)", &empty), Some(8));
        assert_eq!(estimate_raw_length("jmp codecave", &empty), Some(5));
        assert_eq!(estimate_raw_length("call helper", &empty), Some(5));
        assert_eq!(estimate_raw_length("ret", &empty), Some(1));
        // Phase B v2.1 mnemonics — now sized exactly via speculative compile.
        assert_eq!(estimate_raw_length("push ebx", &empty), Some(1));
        assert_eq!(estimate_raw_length("push r13d", &empty), Some(2));
        assert_eq!(estimate_raw_length("pop rax", &empty), Some(1));
        assert_eq!(estimate_raw_length("mov rax, rbx", &empty), Some(3));
        assert_eq!(estimate_raw_length("mov eax, 1", &empty), Some(5));
        assert_eq!(estimate_raw_length("jne target", &empty), Some(6));
        // Phase B v2.2: memory operands estimate via speculative compile.
        // `cmp byte ptr [foo], 1` resolves `foo` to the placeholder and
        // produces an 8-byte encoding (iced picks `cmp r/m8, imm8` with a
        // SIB-less disp32 and explicit 64-bit absolute via the placeholder).
        assert_eq!(
            estimate_raw_length("cmp byte ptr [foo], 1", &empty),
            Some(8)
        );
        // `imul rax, rbx, 4` is outside the Phase B subset — None.
        assert_eq!(estimate_raw_length("imul rax, rbx, 4", &empty), None);
        // `lea rax, [rbx+8]` is now supported (4 bytes: 48 8D 43 08).
        assert_eq!(estimate_raw_length("lea rax, [rbx+8]", &empty), Some(4));
    }

    /// Pass 1 must bind a forward `LabelSite` to the cursor it computes from
    /// previous Raw lengths. This is the lever that makes `jmp return` work
    /// before `return:` is reached at write time.
    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn full_injection_round_trip_against_forked_child() {
        use nix::sys::signal::{Signal, kill};
        use nix::sys::wait::waitpid;
        use nix::unistd::{ForkResult, fork};
        use std::time::Duration;

        // Unique pattern + 8 bytes that will be overwritten by the hook.
        // The pattern is the *anchor* aobscan finds; the 8 bytes immediately
        // after it are what `jmp codecave + nop 3` (5+3 = 8 bytes) replaces.
        let pattern = [0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE];
        let original_hook = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
        // Box::leak so the buffer survives the fork's child lifetime without
        // touching the parent's stack frame.
        let mut buf = vec![0u8; 256];
        buf[0..8].copy_from_slice(&pattern);
        buf[8..16].copy_from_slice(&original_hook);
        let leaked: &'static [u8] = Box::leak(buf.into_boxed_slice());
        let pattern_addr = leaked.as_ptr() as u64;
        let hook_addr = pattern_addr + 8;

        match unsafe { fork() }.expect("fork") {
            ForkResult::Child => {
                // Hold the page alive long enough for the parent to ptrace
                // + run the inject + dealloc + verify.
                std::thread::sleep(Duration::from_secs(10));
                std::process::exit(0);
            }
            ForkResult::Parent { child } => {
                std::thread::sleep(Duration::from_millis(150));

                let pattern_str = pattern
                    .iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let orig_hex = original_hook
                    .iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let script_src = format!(
                    "[ENABLE]\n\
                     aobscanmodule(hook_origin, $process, {pattern_str})\n\
                     alloc(codecave, 0x100, hook_origin)\n\
                     codecave:\n\
                     db {orig_hex}\n\
                     jmp hook_return\n\
                     hook:\n\
                     jmp codecave\n\
                     nop 3\n\
                     hook_return:\n\
                     [DISABLE]\n\
                     dealloc(codecave)\n",
                );
                let script = parse(&script_src).expect("script parses");

                let mut eng = Engine::new(child);
                // `hook` references the 8-byte hook region. AobScan finds
                // `hook_origin` = pattern_addr; we pre-bind `hook` =
                // pattern_addr + 8 so the second LabelSite can resolve to it.
                eng.bind_symbol("hook", hook_addr);

                let active = eng.enable(&script).expect("full inject must succeed");
                let _ = pattern_addr; // silence dead_code on the debug capture

                // After enable, the child's memory at `hook_addr` is the
                // jmp + nop pad we wrote. Read it back and verify.
                let hooked = crate::memory::read_bytes(child, hook_addr, 8).expect("read hook");
                assert_eq!(hooked[0], 0xE9, "first hook byte is jmp rel32 opcode");
                assert_eq!(&hooked[5..8], &[0x90, 0x90, 0x90], "nop 3 fill");

                // Disable rolls back the writes AND deallocs the codecave.
                active.disable().expect("disable must succeed");
                let restored =
                    crate::memory::read_bytes(child, hook_addr, 8).expect("read restored");
                assert_eq!(restored, original_hook, "hook bytes byte-for-byte restored");

                let _ = kill(child, Signal::SIGKILL);
                let _ = waitpid(child, None);
            }
        }
    }
}
