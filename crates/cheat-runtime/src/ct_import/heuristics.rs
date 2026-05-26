//! Cheat-table heuristics: header-vs-ornament, Lua-vs-AA, exe recovery.

use crate::manifest::ManifestFeature;

/// Drop CE `<GroupHeader>` entries that are pure visual ornament: ASCII
/// separators (`---------`), runic dividers (`◣⫘⫘⫘…◢`), info notes prefixed
/// with `❖`, and clarifications wrapped in `《…》`. Cheat-table authors use
/// `<GroupHeader>1</GroupHeader>` for *any* description-only entry — CE
/// itself renders them all uniformly, but a tracker UI listing 108 of them
/// between 5 actual cheats hides the cheats. Concrete rule:
///
/// - reject if the description starts with `❖`, contains a matched `《…》`
///   info-note wrapper, or starts with `❎`/`⚠` UI guidance markers,
/// - reject if it contains no Unicode letters at all (separators),
/// - keep everything else, including the common `【 Title 】` section-header
///   shape used by this author.
pub(super) fn is_meaningful_header(description: &str) -> bool {
    let trimmed = description.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with('❖') {
        return false;
    }
    if trimmed.contains('《') && trimmed.contains('》') {
        return false;
    }
    if !trimmed.chars().any(char::is_alphabetic) {
        return false;
    }
    true
}

/// Strip a single pair of surrounding straight ASCII quotes that CE wraps
/// around `<Description>` literals in the CT file (`"Player Stats"`).
/// Anything fancier (smart quotes, unbalanced) is left untouched.
pub(super) fn strip_quotes(s: String) -> String {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s
    }
}

/// Decide whether an `<AssemblerScript>` body carries an AA script our
/// executor can compile. We need one of the structural primitives the
/// executor implements: `aobscanmodule`, `alloc(`, raw `db ` data writes, or
/// a `registersymbol` (`registersymbol` without the rest would be useless
/// but still parses; that's a CE author error, not ours). Lua-only entries
/// (`{$lua}` blocks, `luacall(...)`) lack all of these and get skipped.
pub(super) fn is_real_aa_script(body: &str) -> bool {
    let needles = ["aobscanmodule", "alloc(", "registersymbol", "\ndb ", " db "];
    needles.iter().any(|n| body.contains(n))
}

/// Scan every toggle's script for the first `aobscanmodule(name, exe, …)`
/// line and return the `exe` argument. CE's aobscanmodule syntax pins the
/// scan to a specific loaded module, so the second comma-separated argument
/// is always the executable / DLL name — that's the binding we surface
/// to the launcher.
pub(super) fn derive_exe(features: &[ManifestFeature]) -> Option<String> {
    for f in features {
        if let Some(found) = derive_exe_from_feature(f) {
            return Some(found);
        }
    }
    None
}

/// Recursive helper — walks `feature` + `feature.children`. Post-#133
/// manifests are trees, so the first AA toggle anchoring the exe name
/// may sit several levels below the root.
///
/// Two recovery strategies, in priority order:
///
/// 1. `aobscanmodule(name, exe, …)` — CE's canonical binding. Authors of
///    module-scoped scans always spell the exe / DLL name as the second
///    argument; we surface it verbatim.
/// 2. `{ Game : X.exe }` comment block — CE's table template inserts one
///    at the top of generated scripts (see Cheat Engine's "Add a new
///    auto-assemble script" boilerplate). This catches Mono / Unity
///    tables that use plain `aobscan(INJECT, …)` (no module scope) and
///    therefore have no aobscanmodule line at all, but still carry the
///    exe name in the convention block.
fn derive_exe_from_feature(feature: &ManifestFeature) -> Option<String> {
    if let Some(script) = &feature.script {
        if let Some(exe) = parse_aobscanmodule_exe(script) {
            return Some(exe);
        }
        if let Some(exe) = parse_comment_block_exe(script) {
            return Some(exe);
        }
    }
    for child in &feature.children {
        if let Some(found) = derive_exe_from_feature(child) {
            return Some(found);
        }
    }
    None
}

fn parse_aobscanmodule_exe(script: &str) -> Option<String> {
    for line in script.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("aobscanmodule(") else {
            continue;
        };
        let mut parts = rest.splitn(3, ',');
        let _name = parts.next();
        let exe = parts.next().map(str::trim)?;
        if !exe.is_empty() {
            return Some(exe.to_string());
        }
    }
    None
}

/// Match the `Game : X.exe` field inside CE's standard `{ ... }` comment
/// block at the top of a script. The block can carry multiple fields
/// (`Game`, `Version`, `Date`, `Author`, …); we only need `Game` and we
/// look for any `.exe` token to stay forgiving of formatting.
fn parse_comment_block_exe(script: &str) -> Option<String> {
    // CE's comment block is `{ ... }` spanning multiple lines. Find the
    // opening brace, then scan inside it for `Game` followed by `:`
    // and an `.exe` token.
    let open = script.find('{')?;
    let close = script[open..].find('}').map(|i| open + i)?;
    let block = &script[open..=close];
    for line in block.lines() {
        let lower = line.to_ascii_lowercase();
        let Some(idx) = lower.find("game") else {
            continue;
        };
        let after = &line[idx + "game".len()..];
        // Allow `Game :`, `Game=`, `Game-` etc.
        let after = after
            .trim_start_matches(|c: char| c.is_whitespace() || c == ':' || c == '=' || c == '-');
        // Pull tokens until we hit one ending in `.exe` (or `.EXE`).
        for token in after.split_whitespace() {
            let token = token.trim_matches(|c: char| c == ',' || c == ';');
            if token.to_ascii_lowercase().ends_with(".exe") && !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}
