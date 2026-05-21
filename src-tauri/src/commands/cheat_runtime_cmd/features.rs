//! `cheat_runtime_list_features` — produces the per-game feature roster the
//! frontend renders, tagged with liveness + symbol-readiness flags.

use cheat_runtime::{
    AddrExpr, FeatureKind, FreezeRegistry, ManifestFeature, ValueSpec, find_pid_by_exe,
    load_manifests_for, parse_addr_expr,
};
use serde::Serialize;
use tauri::State;

use super::{ActiveCheats, purge_stale_cheats};

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
    let manifests = load_manifests_for(&app_id).map_err(|e| e.to_string())?;
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

    let mut out = Vec::new();
    for m in manifests {
        let game_running = find_pid_by_exe(&m.exe).is_some();
        for f in m.features {
            let required_symbol = master_symbol_for_value(&f);
            let symbol_ready = match &required_symbol {
                Some(name) => registered_symbols.contains(name),
                None => true,
            };
            let value_spec = f.value.clone();
            out.push(FeatureView {
                manifest_title: m.title.clone(),
                manifest_exe: m.exe.clone(),
                active: active_keys.contains(&f.uuid),
                uuid: f.uuid,
                name: f.name,
                category: f.category,
                kind: f.kind,
                value_spec,
                required_symbol,
                symbol_ready,
                game_running,
            });
        }
    }
    Ok(out)
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
