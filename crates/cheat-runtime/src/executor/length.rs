//! Pass-1 length estimation for `Statement::Raw` lines.

use std::collections::HashMap;

use crate::asm;

/// Predict the byte length a `Statement::Raw` line will produce in pass 2.
///
/// Used by [`super::Engine::pre_resolve_symbols`] to advance the virtual
/// cursor without committing writes. Length is exact for the constant-encoding
/// directives (`db`, `dq`, `nop N`, `readmem(s, N)`); for the asm subset
/// covered by [`crate::asm`] the estimator delegates to a speculative
/// compile that substitutes `0` for any still-unresolved symbol, so a
/// forward reference like `jmp return` (where `return:` is declared later)
/// returns the correct 5 bytes before the symbol is bound. Returns `None`
/// for anything we cannot size; pass 2 surfaces the clear `Unsupported`
/// error.
pub(super) fn estimate_raw_length(line: &str, symbols: &HashMap<String, u64>) -> Option<usize> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
