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
    /// On any error, every write already applied is reverted before
    /// returning. The returned [`ActiveCheat`] can later be `.disable()`d
    /// to undo the writes on demand.
    pub fn enable(&mut self, script: &Script) -> Result<ActiveCheat, ExecError> {
        let mut active = ActiveCheat {
            pid: self.pid,
            undo: Vec::new(),
            allocs: HashMap::new(),
            disabled: false,
        };
        let mut cursor: Option<u64> = None;

        for stmt in &script.enable {
            if let Err(e) = self.apply_one(stmt, &mut cursor, &mut active) {
                rollback(&mut active);
                return Err(e);
            }
        }
        Ok(active)
    }

    fn apply_one(
        &mut self,
        stmt: &Statement,
        cursor: &mut Option<u64>,
        active: &mut ActiveCheat,
    ) -> Result<(), ExecError> {
        match stmt {
            Statement::AobScanModule {
                symbol, pattern, ..
            } => {
                let addr = self.scan_unique(pattern, symbol)?;
                self.symbols.insert(symbol.clone(), addr);
                Ok(())
            }
            Statement::RegisterSymbol(_)
            | Statement::UnregisterSymbol(_)
            | Statement::Label(_)
            | Statement::Directive(_) => Ok(()),
            Statement::LabelSite(name) => {
                let addr = *self
                    .symbols
                    .get(name)
                    .ok_or_else(|| ExecError::UnknownSymbol(name.clone()))?;
                *cursor = Some(addr);
                Ok(())
            }
            Statement::AbsoluteSite(addr) => {
                *cursor = Some(*addr);
                Ok(())
            }
            Statement::Raw(line) => {
                let Some(base) = *cursor else {
                    return Err(ExecError::OrphanWrite(line.clone()));
                };
                let bytes = compile_raw(line, &self.symbols, self.pid, base)?;
                let original = memory::read_bytes(self.pid, base, bytes.len())?;
                memory::write_bytes(self.pid, base, &bytes)?;
                active.undo.push((base, original));
                *cursor = Some(base + bytes.len() as u64);
                Ok(())
            }
            Statement::Alloc { symbol, size } => {
                let addr = alloc::alloc_remote(self.pid, *size as usize, None, None)?;
                self.symbols.insert(symbol.clone(), addr);
                active.allocs.insert(symbol.clone(), (addr, *size as usize));
                Ok(())
            }
            Statement::Dealloc(symbol) => {
                let (addr, size) = active
                    .allocs
                    .remove(symbol)
                    .ok_or_else(|| ExecError::DeallocUnknown(symbol.clone()))?;
                alloc::dealloc_remote(self.pid, addr, size)?;
                self.symbols.remove(symbol);
                Ok(())
            }
        }
    }

    fn scan_unique(&self, pattern_text: &str, symbol: &str) -> Result<u64, ExecError> {
        let pat = Pattern::parse(pattern_text)?;
        let regions = read_maps(self.pid).map_err(|e| ExecError::Memory(RuntimeError::Io(e)))?;
        let mut hits: Vec<u64> = Vec::new();
        for r in regions.iter().filter(|r| is_scannable(r)) {
            let mut found = scanner::scan_in_process(self.pid, r, &pat)?;
            hits.append(&mut found);
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
    if let Some(bytes) = asm::compile_line(trimmed, symbols, base)? {
        return Ok(bytes);
    }
    Err(ExecError::Unsupported(format!(
        "asm/raw not supported: {line:?}"
    )))
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
        // `mov` / `push` aren't covered by Phase B v1; they should still
        // fall through to Unsupported until Phase B v2 broadens coverage.
        assert!(matches!(
            compile_raw("push ebx", &syms, pid, 0),
            Err(ExecError::Unsupported(_))
        ));
        assert!(matches!(
            compile_raw("mov dword ptr [r13+13C],(float)100", &syms, pid, 0),
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
        let script_src = "[ENABLE]\n\
             registersymbol(victim)\n\
             victim:\n\
             db 00 00 00 00 00 00 00 00\n\
             push ebx\n\
             [DISABLE]\n";
        let script = parse(script_src).unwrap();

        let mut eng = Engine::new(Pid::this());
        eng.bind_symbol("victim", target_addr);

        let err = eng.enable(&script).unwrap_err();
        assert!(matches!(err, ExecError::Unsupported(_)));
        // Crucially, the previously-applied write was reverted.
        assert_eq!(&victim[8..16], &original);
    }
}
