//! Compile a single `Statement::Raw` line into the bytes that will be
//! written at the executor's cursor.

use std::collections::HashMap;

use nix::unistd::Pid;

use crate::asm;
use crate::memory;

use super::ExecError;

/// Compile a raw assembler line to the bytes that should be written at the
/// current cursor. Supports `db`, `dq`, `nop N`, `readmem(symbol, len)`, plus
/// the asm subset covered by [`crate::asm::compile_line`] (`jmp`, `call`,
/// `ret`). `base` is the absolute target address — required for rip-relative
/// encodings.
pub(super) fn compile_raw(
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

pub(super) fn parse_byte_list(rest: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for tok in rest.split_whitespace() {
        if tok.len() != 2 || !tok.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        out.push(u8::from_str_radix(tok, 16).ok()?);
    }
    if out.is_empty() { None } else { Some(out) }
}

pub(super) fn parse_dq(operand: &str, symbols: &HashMap<String, u64>) -> Option<Vec<u8>> {
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

pub(super) fn readmem_bytes(
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
}
