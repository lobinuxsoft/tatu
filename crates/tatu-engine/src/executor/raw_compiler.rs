//! Compile a single `Statement::Raw` line into the bytes that will be
//! written at the executor's cursor.

use std::collections::HashMap;

use crate::asm;
use crate::backend::{Backend, BackendError};

use super::ExecError;

/// Compile a raw assembler line to the bytes that should be written at the
/// current cursor. Supports `db`, `dq`, `nop N`, `readmem(symbol, len)`, plus
/// the asm subset covered by [`crate::asm::compile_line`] (`jmp`, `call`,
/// `ret`). `base` is the absolute target address — required for rip-relative
/// encodings. `readmem` needs the backend so it can fetch live bytes from
/// the target.
pub(super) fn compile_raw<B: Backend>(
    line: &str,
    symbols: &HashMap<String, u64>,
    backend: &mut B,
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
        return readmem_bytes(args, symbols, backend);
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

pub(super) fn readmem_bytes<B: Backend>(
    args: &str,
    symbols: &HashMap<String, u64>,
    backend: &mut B,
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
    let bytes = backend
        .read(addr, len)
        .map_err(|e: BackendError| ExecError::Backend(e))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    //! Backend-free parsing tests. End-to-end tests that need a real
    //! `compile_raw<B>` invocation live in `cheat-runtime`'s executor
    //! tests with `LinuxBackend` plugged in (and will land alongside
    //! the bridge in PR 7B with `Win32Backend`).
    use super::*;

    #[test]
    fn parse_byte_list_works() {
        assert_eq!(
            parse_byte_list("DE AD BE EF"),
            Some(vec![0xde, 0xad, 0xbe, 0xef])
        );
        assert_eq!(parse_byte_list("CA fe"), Some(vec![0xca, 0xfe]));
        assert_eq!(parse_byte_list("DE A"), None);
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
}
