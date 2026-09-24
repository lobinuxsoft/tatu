//! Framework cheat tables (Manifold-style): tables whose cheats are Lua, driven
//! by a Lua framework embedded in the `.CT` rather than pure Auto-Assembler.
//!
//! A framework table carries:
//! * a `<LuaScript>` header that bootstraps the framework (loads its modules,
//!   instantiates `memory`/`utils`/`state`/… globals), and
//! * a `<Files>` block of base85+deflate-encoded modules (`Manifold.*.lua`),
//!   themes (`*.json`) and AA scripts (`*.CEA`).
//!
//! Each cheat entry is a `{$lua}` script with `[ENABLE]`/`[DISABLE]` blocks that
//! call into those framework globals. This module decodes the table, and
//! [`runtime::FrameworkRuntime`] bootstraps it and runs a cheat's blocks.

mod decode;
mod runtime;

use roxmltree::Document;

pub use runtime::{FrameworkError, FrameworkRuntime, MemRec};

/// A decoded framework table: its bootstrap header plus every embedded file
/// (`name → contents`), ready to mount into a [`crate::lua::LuaRuntime`].
#[derive(Debug, Clone)]
pub struct FrameworkTable {
    /// The `<LuaScript>` bootstrap, run once after the modules are mounted.
    pub header: String,
    /// Embedded files keyed by name: `Manifold.*.lua`, `*.json`, `*.CEA`.
    pub files: Vec<(String, String)>,
}

impl FrameworkTable {
    /// Whether this table embeds at least one Lua module (vs. only themes).
    fn has_lua_module(&self) -> bool {
        self.files.iter().any(|(n, _)| n.ends_with(".lua"))
    }
}

/// Parse a `.CT`'s XML into a [`FrameworkTable`], or `None` when it isn't a
/// framework table (no `<LuaScript>` header, or no embedded Lua modules — a
/// plain Auto-Assembler table).
pub fn parse_framework_table(xml: &str) -> Option<FrameworkTable> {
    let doc = Document::parse(xml).ok()?;

    let header = doc
        .descendants()
        .find(|n| n.has_tag_name("LuaScript"))
        .and_then(|n| n.text())?
        .to_string();
    if header.trim().is_empty() {
        return None;
    }

    let files = doc
        .descendants()
        .find(|n| n.has_tag_name("Files"))
        .into_iter()
        .flat_map(|files| files.children())
        .filter(|n| n.attribute("Encoding").is_some())
        .filter_map(|n| {
            let bytes = decode::decode_embedded_file(n.text().unwrap_or(""))?;
            let contents = String::from_utf8_lossy(&bytes).into_owned();
            Some((n.tag_name().name().to_string(), contents))
        })
        .collect();

    let table = FrameworkTable { header, files };
    table.has_lua_module().then_some(table)
}

/// Whether a cheat entry's script is a Lua payload (CE's `{$lua}` directive),
/// as opposed to an Auto-Assembler script.
pub fn is_lua_cheat(script: &str) -> bool {
    script.trim_start().starts_with("{$lua}")
}

/// Cheap check (no module decoding) of whether a `.CT` is a framework table:
/// a non-empty `<LuaScript>` header plus at least one embedded `.lua` module.
pub fn is_framework_table(xml: &str) -> bool {
    let Ok(doc) = Document::parse(xml) else {
        return false;
    };
    let has_header = doc
        .descendants()
        .find(|n| n.has_tag_name("LuaScript"))
        .and_then(|n| n.text())
        .is_some_and(|t| !t.trim().is_empty());
    let has_lua_module = doc
        .descendants()
        .any(|n| n.attribute("Encoding").is_some() && n.tag_name().name().ends_with(".lua"));
    has_header && has_lua_module
}

/// Best-effort target exe from the framework header's `Target = "Game.exe"`
/// setup line (Manifold convention). Used as an exe-binding fallback when the
/// table has no `aobscanmodule(_, exe, _)` to derive from.
pub fn framework_target_exe(xml: &str) -> Option<String> {
    let doc = Document::parse(xml).ok()?;
    let header = doc
        .descendants()
        .find(|n| n.has_tag_name("LuaScript"))
        .and_then(|n| n.text())?;
    let after = &header[header.find("Target")?..];
    let open = after.find('"')? + 1;
    let close = after[open..].find('"')? + open;
    let exe = after[open..close].trim();
    (!exe.is_empty()).then(|| exe.to_string())
}

/// Split a `{$lua}` cheat into its `[ENABLE]` and `[DISABLE]` Lua sources.
///
/// A script without an explicit `[ENABLE]` marker is treated as all-enable
/// (CE runs the whole `{$lua}` body on enable). The `{$lua}` directive and
/// block markers are stripped; surrounding whitespace is trimmed.
pub fn lua_enable_disable(script: &str) -> (String, String) {
    let body = script.trim_start().strip_prefix("{$lua}").unwrap_or(script);
    match body.find("[ENABLE]") {
        Some(en) => {
            let after = &body[en + "[ENABLE]".len()..];
            match after.find("[DISABLE]") {
                Some(dis) => (
                    after[..dis].trim().to_string(),
                    after[dis + "[DISABLE]".len()..].trim().to_string(),
                ),
                None => (after.trim().to_string(), String::new()),
            }
        }
        None => (body.trim().to_string(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_lua_cheats() {
        assert!(is_lua_cheat("{$lua}\n[ENABLE]\nfoo()"));
        assert!(is_lua_cheat("  {$lua} stuff"));
        assert!(!is_lua_cheat("[ENABLE]\naobscanmodule(x,y,z)"));
    }

    #[test]
    fn splits_enable_disable() {
        let (en, dis) = lua_enable_disable("{$lua}\n[ENABLE]\nfoo()\n[DISABLE]\nbar()");
        assert_eq!(en, "foo()");
        assert_eq!(dis, "bar()");
    }

    #[test]
    fn enable_only_when_no_markers() {
        let (en, dis) = lua_enable_disable("{$lua}\nfoo()\nbar()");
        assert_eq!(en, "foo()\nbar()");
        assert!(dis.is_empty());
    }

    #[test]
    fn enable_without_disable() {
        let (en, dis) = lua_enable_disable("{$lua}\n[ENABLE]\nfoo()");
        assert_eq!(en, "foo()");
        assert!(dis.is_empty());
    }

    // End-to-end against a real framework table: decode → bootstrap → run a
    // cheat block. The table is the author's copyrighted artefact (gitignored),
    // so the test skips when absent. Drives self-pid (no game): memory ops are
    // harmless, what's exercised is the loader + bootstrap + block execution.
    //   cargo test -p cheat-runtime --lib framework_smoke -- --ignored --nocapture
    #[test]
    #[ignore = "needs a CE table in .local-ct/ (gitignored)"]
    fn framework_smoke_loads_and_runs_a_cheat_block() {
        use std::path::Path;
        let ct = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local-ct/DD2_v6.0.0_Full.ct");
        if !ct.exists() {
            eprintln!("SKIP: no CE table at {ct:?}");
            return;
        }
        let xml = std::fs::read_to_string(&ct).unwrap();

        let table = parse_framework_table(&xml).expect("DD2 is a framework table");
        assert!(
            table
                .files
                .iter()
                .filter(|(n, _)| n.ends_with(".lua"))
                .count()
                >= 10,
            "expected the Manifold module set"
        );

        // Bootstrap against ourselves (the theme `[UI]` log errors are expected:
        // the UI stub can't synthesise CE's LCL component tree; non-fatal).
        let pid = nix::unistd::getpid();
        let rt = FrameworkRuntime::load(pid, &table).expect("bootstrap");
        assert_eq!(rt.pid(), pid);

        // Run a benign [ENABLE] block that calls a live framework global with a
        // memrec in scope — proves the bootstrapped runtime executes cheat Lua.
        let memrec = MemRec {
            id: 1,
            description: "Smoke Cheat".to_string(),
        };
        rt.enable(
            &memrec,
            r#"logger:Info("[smoke] enable " .. memrec.Description)"#,
        )
        .expect("enable block runs");
        eprintln!("\n==== FRAMEWORK SMOKE: load + bootstrap + cheat block OK ====");
    }
}
