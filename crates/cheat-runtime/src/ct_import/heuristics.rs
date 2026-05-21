//! Cheat-table heuristics: header-vs-ornament, Lua-vs-AA, exe recovery,
//! prereq inference (RE Engine titles need REFramework).

use crate::manifest::{ManifestFeature, Prereq};

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

/// Known Capcom RE Engine exe names. Every CT for these titles carries
/// the fixstuff caveat *"praydog REFramework is required to bypass
/// anti-cheat included in this game and avoid crashes"* — without
/// REFramework, our AOB scans + trampolines crash the game inside a
/// few seconds. List is exact-match (case-insensitive) on the
/// `aobscanmodule` exe binding emitted by the CT importer.
///
/// Future titles get added as the community publishes CTs for them; an
/// unknown exe falls back to "no prereqs", which is the conservative
/// default (extra prereq always blockable from the UI, missed prereq
/// crashes the game on enable — so we err on the under-detect side).
const RE_ENGINE_EXES: &[&str] = &[
    "PRAGMATA.exe",
    "MonsterHunterWilds.exe",
    "MonsterHunterRise.exe",
    "MHRise.exe",
    "re2.exe",
    "re3.exe",
    "re4.exe",
    "re7.exe",
    "re8.exe",
    "RE2.exe",
    "RE3.exe",
    "RE4.exe",
    "RE7.exe",
    "RE8.exe",
    "DragonsDogma2.exe",
    "DMC5.exe",
    "DevilMayCry5.exe",
    "StreetFighter6.exe",
    "SF6.exe",
];

/// Auto-derive the prereqs vector from the resolved exe name. Returns
/// `[Prereq::Reframework]` for any RE Engine title, empty otherwise.
pub(super) fn infer_prereqs(exe: &str) -> Vec<Prereq> {
    if RE_ENGINE_EXES
        .iter()
        .any(|known| known.eq_ignore_ascii_case(exe))
    {
        vec![Prereq::Reframework {
            required_for_anticheat: true,
        }]
    } else {
        Vec::new()
    }
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
