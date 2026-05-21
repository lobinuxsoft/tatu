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
pub(super) fn estimate_raw_length(
    line: &str,
    symbols: &HashMap<String, u64>,
    base: u64,
) -> Option<usize> {
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

    // Asm line: speculatively compile with the REAL cursor as the base
    // address. This is critical for control-flow encodings — iced-x86
    // picks the 5-byte `jmp rel32` form only when the target is within
    // ±2 GiB of `base`; with base=0 a near target like 0x143...ea0 looks
    // 5 GiB away and iced falls back to a 14-byte `jmp [rip+disp32]`,
    // inflating pass-1 length and stomping the cursor for any LabelSite
    // that lands after the trampoline.
    //
    // Unresolved symbols (forward refs declared later in the script) get
    // a placeholder near `base` so their speculative encoding matches the
    // real one — pass 2 substitutes the bound address and produces the
    // same byte count.
    let placeholder = base.wrapping_add(0x10000);
    let mut perm = symbols.clone();
    for _ in 0..16 {
        match asm::compile_line(trimmed, &perm, base) {
            Ok(Some(bytes)) => return Some(bytes.len()),
            Ok(None) => return None,
            Err(asm::AsmError::UnknownSymbol(name)) => {
                perm.insert(name, placeholder);
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
        assert_eq!(estimate_raw_length("db 90 90 90", &empty, 0), Some(3));
        assert_eq!(estimate_raw_length("dq 0", &empty, 0), Some(8));
        assert_eq!(estimate_raw_length("nop 5", &empty, 0), Some(5));
        assert_eq!(estimate_raw_length("nop", &empty, 0), Some(1));
        assert_eq!(estimate_raw_length("readmem(orig, 8)", &empty, 0), Some(8));
        assert_eq!(estimate_raw_length("jmp codecave", &empty, 0), Some(5));
        assert_eq!(estimate_raw_length("call helper", &empty, 0), Some(5));
        assert_eq!(estimate_raw_length("ret", &empty, 0), Some(1));
        assert_eq!(estimate_raw_length("push ebx", &empty, 0), Some(1));
        assert_eq!(estimate_raw_length("push r13d", &empty, 0), Some(2));
        assert_eq!(estimate_raw_length("pop rax", &empty, 0), Some(1));
        assert_eq!(estimate_raw_length("mov rax, rbx", &empty, 0), Some(3));
        assert_eq!(estimate_raw_length("mov eax, 1", &empty, 0), Some(5));
        assert_eq!(estimate_raw_length("jne target", &empty, 0), Some(6));
        assert_eq!(
            estimate_raw_length("cmp byte ptr [foo], 1", &empty, 0),
            Some(8)
        );
        assert_eq!(estimate_raw_length("imul rax, rbx, 4", &empty, 0), None);
        assert_eq!(estimate_raw_length("lea rax, [rbx+8]", &empty, 0), Some(4));
    }

    /// Regression: a `jmp` to a target within ±2 GiB of `base` must be
    /// estimated at 5 bytes (rel32), not the 14-byte indirect form iced
    /// falls back to when the target overflows i32 from base=0. The EM
    /// master toggle relied on this — base=0 produced an 11-byte
    /// over-estimate that shifted every label that followed.
    #[test]
    fn estimate_uses_base_to_keep_rel32_short() {
        let mut syms = HashMap::new();
        syms.insert("newmem_1".to_string(), 0x13ffff000);
        // base=0 ⇒ target is 5 GiB away ⇒ iced falls back to long form.
        let long = estimate_raw_length("jmp newmem_1", &syms, 0).unwrap();
        assert!(long > 5, "expected fallback form, got {long}");
        // Real cursor at the trampoline site ⇒ rel32 fits ⇒ 5 bytes.
        let short = estimate_raw_length("jmp newmem_1", &syms, 0x143006e96).unwrap();
        assert_eq!(short, 5);
    }
}
