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
        let Some(script) = &f.script else {
            continue;
        };
        for line in script.lines() {
            let trimmed = line.trim();
            let Some(rest) = trimmed.strip_prefix("aobscanmodule(") else {
                continue;
            };
            let mut parts = rest.splitn(3, ',');
            let _name = parts.next();
            let Some(exe) = parts.next().map(str::trim) else {
                continue;
            };
            if !exe.is_empty() {
                return Some(exe.to_string());
            }
        }
    }
    None
}
