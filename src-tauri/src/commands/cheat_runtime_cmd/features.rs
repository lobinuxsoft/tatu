//! `cheat_runtime_list_features` — produces the per-game feature roster the
//! frontend renders, tagged with liveness + symbol-readiness flags.

use cheat_runtime::{
    AddrExpr, FeatureKind, FreezeRegistry, Manifest, ManifestFeature, UnityBackend, ValueSpec,
    analysis, detect_unity_backend, find_pid_by_exe, is_mono_symbol, load_manifests_for,
    parse_addr_expr, parse_script,
};
use serde::Serialize;
use tauri::State;

use super::{ActiveCheats, purge_stale_cheats};
use crate::steam::detect_game_exe;
use crate::steam::exe::find_install_path;

#[derive(Debug, Serialize)]
pub struct FeatureView {
    pub manifest_title: String,
    pub manifest_exe: String,
    pub uuid: String,
    pub name: String,
    pub category: Option<String>,
    pub kind: FeatureKind,
    pub active: bool,
    pub game_running: bool,
    /// Present only for `kind == Value`. The frontend uses this to render
    /// a typed input + freeze toggle and to detect whether the value's
    /// symbol dependency is satisfied (see [`master_symbol_for_value`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_spec: Option<ValueSpec>,
    /// Name of the AA symbol this Value entry's `base_expr` dereferences,
    /// if any. Used by the UI to dim Value features whose scaffold cheat
    /// isn't enabled yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_symbol: Option<String>,
    /// `true` if the value's required symbol is currently bound by some
    /// active cheat (or the value has no symbol dep). The UI uses this to
    /// gate the read/write/freeze controls on a per-feature basis.
    pub symbol_ready: bool,
    /// `true` when this Toggle's script references a symbol no script in the
    /// table binds — a pointer root resolved only by the CE Lua framework
    /// (e.g. `CharacterManagerPtr`). Such cheats can't run in the native
    /// executor; the UI surfaces them as "needs CE" instead of a toggle that
    /// would fail at enable time. Always `false` for Headers / Values and for
    /// self-contained Toggles.
    pub needs_ce: bool,
    /// Nested feature subtree. Mirrors `ManifestFeature::children` —
    /// see #133. Empty for leaves; omitted from JSON when empty so the
    /// payload stays compact for flat manifests.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<FeatureView>,
}

#[tauri::command]
pub fn cheat_runtime_list_features(
    app_id: String,
    active: State<'_, ActiveCheats>,
    freezes: State<'_, FreezeRegistry>,
) -> Result<Vec<FeatureView>, String> {
    // Drop registry entries whose backing process / bridge is gone —
    // the UI must not paint toggles as active when the game has been
    // closed out-of-band. This also cancels any freeze loops keyed
    // on the same feature_uuid so the frontend's frozen-value
    // indicators clear at the same time.
    purge_stale_cheats(&active, Some(&freezes))?;
    let exe_hint = detect_game_exe(&app_id).ok();
    let manifests = load_manifests_for(&app_id, exe_hint.as_deref()).map_err(|e| e.to_string())?;
    let (active_keys, registered_symbols) = {
        let guard = active
            .lock()
            .map_err(|e| format!("active registry poisoned: {e}"))?;
        let keys: std::collections::HashSet<String> = guard.keys().cloned().collect();
        let mut syms: std::collections::HashSet<String> = std::collections::HashSet::new();
        for cheat in guard.values() {
            for k in cheat.symbols().keys() {
                syms.insert(k.clone());
            }
        }
        (keys, syms)
    };

    // Unity Mono games can resolve `Class:Method` symbols through the collector
    // at enable time, so those don't count as "needs CE".
    let mono_backend = find_install_path(&app_id)
        .map(|dir| detect_unity_backend(&dir) == UnityBackend::Mono)
        .unwrap_or(false);

    let mut out = Vec::new();
    for m in manifests {
        let game_running = find_pid_by_exe(&m.exe).is_some();
        let provided_symbols = collect_provided_symbols(&m);
        let ctx = ViewCtx {
            manifest_title: &m.title,
            manifest_exe: &m.exe,
            active_keys: &active_keys,
            registered_symbols: &registered_symbols,
            game_running,
            provided_symbols: &provided_symbols,
            framework: m.framework,
            mono_backend,
        };
        for f in &m.features {
            out.push(build_view(f, &ctx));
        }
    }
    Ok(out)
}

/// Per-manifest context handed to the recursive view builder so each call
/// site picks up the same liveness + symbol-readiness flags.
struct ViewCtx<'a> {
    manifest_title: &'a str,
    manifest_exe: &'a str,
    active_keys: &'a std::collections::HashSet<String>,
    registered_symbols: &'a std::collections::HashSet<String>,
    game_running: bool,
    /// Every symbol bound by *any* script in this manifest — the resolution
    /// baseline for [`compute_needs_ce`]. Sibling scripts often register the
    /// shared pointer roots one cheat dereferences.
    provided_symbols: &'a std::collections::HashSet<String>,
    /// Whether this manifest is a Lua framework table — its `{$lua}` cheats run
    /// in the framework runtime, so they aren't "needs CE".
    framework: bool,
    /// Whether the game is Unity Mono — `Class:Method` symbols then resolve via
    /// the collector, so they don't make a cheat "needs CE".
    mono_backend: bool,
}

fn build_view(f: &ManifestFeature, ctx: &ViewCtx<'_>) -> FeatureView {
    let required_symbol = master_symbol_for_value(f);
    let symbol_ready = match &required_symbol {
        Some(name) => ctx.registered_symbols.contains(name),
        None => true,
    };
    let children = f.children.iter().map(|c| build_view(c, ctx)).collect();
    // A framework table's `{$lua}` cheat runs in the framework runtime, so it's
    // not "needs CE" — a pointer-root-dependent one may still fail at enable
    // time (until the pointer scanner lands), which surfaces as a runtime error
    // rather than a pre-emptive badge.
    let needs_ce = if f.lua && ctx.framework {
        false
    } else {
        f.script
            .as_deref()
            .is_some_and(|src| compute_needs_ce(src, ctx.provided_symbols, ctx.mono_backend))
    };
    FeatureView {
        manifest_title: ctx.manifest_title.to_string(),
        manifest_exe: ctx.manifest_exe.to_string(),
        active: ctx.active_keys.contains(&f.uuid),
        uuid: f.uuid.clone(),
        name: f.name.clone(),
        category: f.category.clone(),
        kind: f.kind,
        value_spec: f.value.clone(),
        required_symbol,
        symbol_ready,
        game_running: ctx.game_running,
        needs_ce,
        children,
    }
}

/// Every symbol any script in the manifest binds. Built once per
/// `list_features` call and shared across all `build_view` invocations as
/// the resolution baseline (a cheat may reference a pointer root a sibling
/// registers).
fn collect_provided_symbols(manifest: &Manifest) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for f in manifest.features_recursive() {
        if let Some(src) = &f.script
            && let Ok(script) = parse_script(src)
        {
            out.extend(analysis::provided_symbols(&script));
        }
    }
    out
}

/// A Toggle needs CE delegation when its script is pure Lua, or references a
/// symbol no script in the table binds (a Lua-resolved pointer root). An
/// unparseable script returns `false` so enable surfaces the real error.
///
/// When `mono_backend` is set, `Class:Method` symbols are resolvable through the
/// collector at enable time, so they don't count toward "needs CE" — only
/// genuinely unbindable symbols (Lua pointer roots) do.
fn compute_needs_ce(
    script_src: &str,
    provided: &std::collections::HashSet<String>,
    mono_backend: bool,
) -> bool {
    match parse_script(script_src) {
        Ok(script) => {
            if script.lua_only {
                return true;
            }
            analysis::unresolved_symbols(&script, provided)
                .iter()
                .any(|s| !(mono_backend && is_mono_symbol(s)))
        }
        Err(_) => false,
    }
}

/// For a Value feature, return the name of the symbol its `base_expr`
/// dereferences. `None` for literals, Headers, Toggles, and any unparseable
/// expression (the runtime will reject the read at apply time with a more
/// specific error, but the UI can already dim the row).
fn master_symbol_for_value(f: &ManifestFeature) -> Option<String> {
    let spec = f.value.as_ref()?;
    match parse_addr_expr(&spec.base_expr).ok()? {
        AddrExpr::SymbolDeref { symbol, .. } => Some(symbol),
        AddrExpr::Literal(_) => None,
    }
}
