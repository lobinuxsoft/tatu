//! Walk a parsed `<CheatTable>` document and project every recognisable
//! `<CheatEntry>` into a [`ManifestFeature`].

use crate::manifest::{FeatureKind, ManifestFeature, VType, ValueSpec};

use super::heuristics::{is_meaningful_header, is_real_aa_script, strip_quotes};

/// Walk the direct `<CheatEntry>` children of `node` and project each into a
/// [`ManifestFeature`], recursively descending into nested `<CheatEntries>`
/// blocks to preserve CE's tree structure as `ManifestFeature::children`.
///
/// CE tables routinely group cheats under [`FeatureKind::Header`] entries
/// (sometimes 5+ levels deep — see `ender_magnolia_v11.ct` and the Elden
/// Ring tables for depth ≥ 7). The UI relies on this nesting to render a
/// collapsible tree; flattening here would force the UI to invent a
/// grouping after-the-fact, losing fidelity (a Toggle that owns child
/// Toggles is a CE pattern that can't be expressed by Header-as-divider).
pub(super) fn walk_entries(node: roxmltree::Node, stem: &str, out: &mut Vec<ManifestFeature>) {
    // The CT document layout is `<CheatTable> ⤳ <CheatEntries> ⤳ <CheatEntry>+ ⤳ <CheatEntries> ⤳ ...`.
    // Callers pass us either a `<CheatTable>` root (top-level entry) or a
    // `<CheatEntries>` block (recursive call). Find the bag of entries to
    // iterate accordingly: a CheatEntries' direct children ARE the
    // `<CheatEntry>` elements; for CheatTable / CheatEntry roots, descend
    // through the inner `<CheatEntries>` block first.
    let entries_root = if node.has_tag_name("CheatEntries") {
        node
    } else {
        match node.children().find(|c| c.has_tag_name("CheatEntries")) {
            Some(block) => block,
            None => return,
        }
    };

    for entry in entries_root
        .children()
        .filter(|n| n.has_tag_name("CheatEntry"))
    {
        match entry_to_feature(entry, stem) {
            Some(mut feature) => {
                feature.children = child_features(entry, stem);
                out.push(feature);
            }
            None => {
                // Parent entry was skipped (Lua-only script, decorative
                // header, unsupported VType). Promote its surviving
                // children to this level so they don't get orphaned —
                // CE's authoring tool nests aggressively for visual
                // organisation, a missing parent shouldn't make the
                // user's god-mode disappear from the table.
                out.extend(child_features(entry, stem));
            }
        }
    }
}

/// Walk the nested `<CheatEntries>` block of a `<CheatEntry>` (if any) and
/// return its children projected as [`ManifestFeature`]s.
fn child_features(entry: roxmltree::Node, stem: &str) -> Vec<ManifestFeature> {
    let mut out = Vec::new();
    walk_entries(entry, stem, &mut out);
    out
}

fn entry_to_feature(entry: roxmltree::Node, stem: &str) -> Option<ManifestFeature> {
    let id = child_text(entry, "ID").unwrap_or_default();
    let description = strip_quotes(child_text(entry, "Description").unwrap_or_default());
    if description.is_empty() {
        return None;
    }
    let uuid = format!("ct-{stem}-{id}");

    if child_text(entry, "GroupHeader").is_some_and(|v| v.trim() == "1") {
        if !is_meaningful_header(&description) {
            return None;
        }
        return Some(ManifestFeature {
            uuid,
            name: description,
            category: None,
            kind: FeatureKind::Header,
            script: None,
            value: None,
            children: Vec::new(),
        });
    }

    let vtype_text = child_text(entry, "VariableType").unwrap_or_default();
    let vtype_text = vtype_text.trim();

    if vtype_text == "Auto Assembler Script" {
        let script = child_text(entry, "AssemblerScript").unwrap_or_default();
        if !is_real_aa_script(&script) {
            return None;
        }
        return Some(ManifestFeature {
            uuid,
            name: description,
            category: None,
            kind: FeatureKind::Toggle,
            script: Some(script),
            value: None,
            children: Vec::new(),
        });
    }

    // Numeric value entry — CE pointer-chain. Requires both <Address> and
    // a supported <VariableType>. Without <Address> the entry can't be
    // located in target memory.
    let base_expr = child_text(entry, "Address")?;
    let vtype = vtype_from_ce(vtype_text, signed_from_show_as_signed(entry))?;
    let offsets = parse_offsets(entry);

    Some(ManifestFeature {
        uuid,
        name: description,
        category: None,
        kind: FeatureKind::Value,
        script: None,
        value: Some(ValueSpec {
            base_expr,
            offsets,
            vtype,
        }),
        children: Vec::new(),
    })
}

/// CE's `<ShowAsSigned>` is a per-entry override of the global Settings
/// checkbox; on disk it's present only when the author flipped it. Missing
/// element means "use default" — we treat that as unsigned, matching the
/// default checkbox state.
fn signed_from_show_as_signed(entry: roxmltree::Node) -> bool {
    child_text(entry, "ShowAsSigned").is_some_and(|v| v.trim() == "1")
}

/// Map CE's `<VariableType>` string + signed flag into our [`VType`].
/// CE accepts "Byte" / "2 Bytes" / "String" / "Binary" / etc. — those are
/// outside the importer's current scope, so they return `None` and the
/// caller skips the entry.
fn vtype_from_ce(text: &str, signed: bool) -> Option<VType> {
    Some(match text {
        "4 Bytes" if signed => VType::I32,
        "4 Bytes" => VType::U32,
        "8 Bytes" if signed => VType::I64,
        "8 Bytes" => VType::U64,
        "Float" => VType::F32,
        "Double" => VType::F64,
        _ => return None,
    })
}

/// Collect `<Offset>` children in document order. Each offset is parsed as
/// hex (CE stores them without the `0x` prefix). Malformed entries get
/// silently dropped; that matches CE's own tolerance — it skips the chain
/// rather than refusing to load the table.
fn parse_offsets(entry: roxmltree::Node) -> Vec<u64> {
    let Some(offsets_node) = entry.children().find(|c| c.has_tag_name("Offsets")) else {
        return Vec::new();
    };
    offsets_node
        .children()
        .filter(|c| c.has_tag_name("Offset"))
        .filter_map(|n| {
            let raw = n.text().unwrap_or("").trim();
            let body = raw
                .strip_prefix("0x")
                .or_else(|| raw.strip_prefix("0X"))
                .unwrap_or(raw);
            u64::from_str_radix(body, 16).ok()
        })
        .collect()
}

/// Direct-child text accessor: returns the trimmed text of the first
/// element child with `tag` under `node`, or `None` if absent.
fn child_text(node: roxmltree::Node, tag: &str) -> Option<String> {
    let child = node.children().find(|c| c.has_tag_name(tag))?;
    Some(child.text().unwrap_or("").trim().to_string())
}
