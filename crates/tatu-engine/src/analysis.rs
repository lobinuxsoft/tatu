//! Static symbol analysis for a parsed [`Script`].
//!
//! A CE-AA cheat runs in tatu's native executor only if every symbol it
//! references is bound by some script in the same table (its own
//! `aobscanmodule`, a sibling's `registersymbol`, a local label, …). Tables
//! built on a Lua framework (e.g. Manifold) bind some pointer-root symbols —
//! `CharacterManagerPtr`, `PawnManagerPtr` — inside the embedded Lua
//! interpreter, which tatu does not ship. A script that depends on one of
//! those can't execute natively and must be delegated to CE.
//!
//! [`unresolved_symbols`] computes that gap statically so the UI can flag
//! the cheat up front instead of letting the toggle fail at enable time.

use std::collections::HashSet;

use crate::parser::{NameList, Script, Statement};

/// Symbols a script *binds* — visible to itself and to sibling scripts in
/// the same table: `aobscanmodule` / `aobscan` targets, `registersymbol` /
/// `label` names, `alloc` / `globalalloc` / `define` names, and the symbolic
/// label sites it defines (`codecave:`).
pub fn provided_symbols(script: &Script) -> HashSet<String> {
    let mut out = HashSet::new();
    for stmt in script.enable.iter().chain(script.disable.iter()) {
        match stmt {
            Statement::AobScanModule { symbol, .. }
            | Statement::AobScan { symbol, .. }
            | Statement::Alloc { symbol, .. }
            | Statement::GlobalAlloc { symbol, .. } => {
                out.insert(symbol.clone());
            }
            Statement::RegisterSymbol(names) | Statement::Label(names) => {
                add_names(&mut out, names)
            }
            Statement::Define { name, .. } => {
                out.insert(name.clone());
            }
            Statement::LabelSite(name) => {
                out.insert(name.clone());
            }
            _ => {}
        }
    }
    out
}

/// Identifiers a script *references* that look like symbols: `SymbolSite`
/// bases plus operand identifiers inside `Raw` instruction lines. Registers,
/// asm keywords, and numeric literals are excluded.
pub fn referenced_symbols(script: &Script) -> HashSet<String> {
    let mut out = HashSet::new();
    for stmt in &script.enable {
        match stmt {
            Statement::SymbolSite { symbol, .. } => {
                out.insert(symbol.clone());
            }
            Statement::Raw(line) => collect_operand_symbols(line, &mut out),
            _ => {}
        }
    }
    out
}

/// Symbols a script references but neither it nor `provided_externally`
/// binds — i.e. symbols only the CE Lua framework can resolve. A non-empty
/// result means the cheat cannot run in the native executor.
///
/// `provided_externally` should hold every symbol bound by *any* script in
/// the table (siblings register shared pointer roots), unioned with this
/// script's own [`provided_symbols`].
pub fn unresolved_symbols(
    script: &Script,
    provided_externally: &HashSet<String>,
) -> HashSet<String> {
    let mut provided = provided_symbols(script);
    provided.extend(provided_externally.iter().cloned());
    referenced_symbols(script)
        .into_iter()
        .filter(|s| !provided.contains(s))
        .collect()
}

fn add_names(out: &mut HashSet<String>, names: &NameList) {
    if let NameList::Names(list) = names {
        out.extend(list.iter().cloned());
    }
}

/// Pull symbol-shaped identifiers out of one assembly line. The mnemonic
/// (first token) is skipped; the remaining operand text is split on the
/// punctuation that surrounds memory operands and offsets so a name like
/// `CharacterManagerPtr` falls out of `mov rsi,[CharacterManagerPtr+8]`.
fn collect_operand_symbols(line: &str, out: &mut HashSet<String>) {
    let line = line.trim();
    // Drop the mnemonic; everything after the first whitespace is operands.
    let Some((_mnemonic, operands)) = line.split_once(char::is_whitespace) else {
        return;
    };
    for tok in operands.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        if looks_like_symbol(tok) {
            out.insert(tok.to_string());
        }
    }
}

/// True for a bare identifier that is not a register, asm keyword, or numeric
/// literal. CE symbols are `[A-Za-z_][A-Za-z0-9_]*`; a leading digit means a
/// number (decimal or bare hex like `2D0`), which we reject.
fn looks_like_symbol(tok: &str) -> bool {
    let mut chars = tok.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    if !tok.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    let low = tok.to_ascii_lowercase();
    !is_register(&low) && !is_keyword(&low)
}

fn is_register(low: &str) -> bool {
    const GPR: &[&str] = &[
        "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "rsp", "r8", "r9", "r10", "r11", "r12",
        "r13", "r14", "r15", "eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp", "r8d", "r9d",
        "r10d", "r11d", "r12d", "r13d", "r14d", "r15d", "ax", "bx", "cx", "dx", "si", "di", "bp",
        "sp", "al", "bl", "cl", "dl", "ah", "bh", "ch", "dh", "rip",
    ];
    if GPR.contains(&low) {
        return true;
    }
    // xmm0..15 / ymm0..15 / st0..7
    if let Some(idx) = low.strip_prefix("xmm").or_else(|| low.strip_prefix("ymm")) {
        return idx.parse::<u8>().is_ok_and(|n| n < 16);
    }
    if let Some(idx) = low.strip_prefix("st") {
        return idx.is_empty() || idx.parse::<u8>().is_ok_and(|n| n < 8);
    }
    false
}

fn is_keyword(low: &str) -> bool {
    const KW: &[&str] = &[
        "byte", "word", "dword", "qword", "tbyte", "xmmword", "ptr", "short", "near", "far",
        "float", "double",
    ];
    KW.contains(&low)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn syms(set: &[&str]) -> HashSet<String> {
        set.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn self_contained_cheat_has_no_unresolved_symbols() {
        // The DD2 Player Invincibility shape: own scan + own-symbol patch.
        let s = parse(
            "[ENABLE]\n\
             aobScanModule(IsPlayerInvincibilityHook,DD2.exe,80 7F 15 ? 75 ? 48 8B)\n\
             IsPlayerInvincibilityHook+3:\n\
             db 01\n\
             registersymbol(IsPlayerInvincibilityHook)\n\
             [DISABLE]\n",
        )
        .unwrap();
        assert!(unresolved_symbols(&s, &HashSet::new()).is_empty());
    }

    #[test]
    fn framework_dependent_cheat_flags_pointer_root() {
        // The DD2 Stamina shape: own scan, but the codecave dereferences a
        // pointer root the Lua framework binds.
        let s = parse(
            "[ENABLE]\n\
             aobScanModule(StaminaWorkHook,DD2.exe,F2 0F ? ?)\n\
             alloc(n_StaminaWork,$1000)\n\
             label(o_StaminaWork)\n\
             n_StaminaWork:\n\
             push rsi\n\
             mov rsi,CharacterManagerPtr\n\
             mov rsi,[rsi]\n\
             [DISABLE]\n",
        )
        .unwrap();
        let unresolved = unresolved_symbols(&s, &HashSet::new());
        assert_eq!(unresolved, syms(&["CharacterManagerPtr"]));
    }

    #[test]
    fn sibling_provided_symbol_resolves() {
        // Same Stamina script, but a sibling already bound the pointer root.
        let s = parse(
            "[ENABLE]\n\
             aobScanModule(StaminaWorkHook,DD2.exe,F2 0F ? ?)\n\
             n_StaminaWork:\n\
             mov rsi,CharacterManagerPtr\n\
             [DISABLE]\n",
        )
        .unwrap();
        let external = syms(&["CharacterManagerPtr"]);
        assert!(unresolved_symbols(&s, &external).is_empty());
    }

    #[test]
    fn registers_and_keywords_are_not_symbols() {
        let s = parse(
            "[ENABLE]\n\
             aobScanModule(Hook,DD2.exe,90)\n\
             Hook:\n\
             cmp byte ptr [rdi+15],00\n\
             comisd xmm0,[Hook+8]\n\
             [DISABLE]\n",
        )
        .unwrap();
        // rdi, byte, ptr, xmm0 must not register as symbols; Hook is provided.
        assert!(unresolved_symbols(&s, &HashSet::new()).is_empty());
    }
}
