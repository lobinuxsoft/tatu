use super::hints::preservability_hint;
use super::types::{DrmInfo, DrmStatus, Preservability};
use super::vendors::{add_vendors, detect_baked_in_vendors, detect_store_vendors};

/// Raw data fetched from upstream sources before merging. Shared contract
/// between `sources` (producer) and `classify` (consumer).
#[derive(Debug, Default, Clone)]
pub(super) struct RawDrm {
    pub pcgw_stores: Vec<String>,
    pub pcgw_uses: Vec<String>,
    pub pcgw_removed: Vec<String>,
    pub pcgw_retail: Vec<String>,
    pub steam_drm_notice: Option<String>,
    pub steam_account_notice: Option<String>,
    pub steam_store_ok: bool,
    pub pcgw_ok: bool,
    pub pcgw_has_entry: bool,
}

/// Merge Steam Store + PCGamingWiki raw data into a final DrmInfo.
pub(super) fn merge(raw: RawDrm, fetched_at: u64) -> DrmInfo {
    let mut notes_parts: Vec<String> = Vec::new();

    // Step 1 — detect baked-in vendors from all sources.
    // Baked-in DRM affects every release of the game, Steam included.
    let mut baked_in: Vec<String> = Vec::new();
    if let Some(ref notice) = raw.steam_drm_notice {
        add_vendors(&mut baked_in, &detect_baked_in_vendors(notice));
        notes_parts.push(format!("Steam drm_notice: {notice}"));
    }
    if let Some(ref notice) = raw.steam_account_notice {
        add_vendors(&mut baked_in, &detect_baked_in_vendors(notice));
        notes_parts.push(format!("Cuenta externa requerida: {notice}"));
    }
    for token in raw.pcgw_uses.iter().chain(raw.pcgw_retail.iter()) {
        add_vendors(&mut baked_in, &detect_baked_in_vendors(token));
    }

    if !raw.pcgw_stores.is_empty() {
        notes_parts.push(format!("PCGW Stores: {}", raw.pcgw_stores.join(", ")));
    }
    if !raw.pcgw_uses.is_empty() {
        notes_parts.push(format!("PCGW Uses: {}", raw.pcgw_uses.join(", ")));
    }
    if !raw.pcgw_removed.is_empty() {
        notes_parts.push(format!("PCGW Removed: {}", raw.pcgw_removed.join(", ")));
    }

    // Step 2 — positional alignment, only when Stores.len == Uses_DRM.len.
    let steam_release_drm = aligned_steam_drm(&raw.pcgw_stores, &raw.pcgw_uses);

    // Step 3 — classify from the Steam-copy perspective.
    let status = if !baked_in.is_empty() {
        DrmStatus::ThirdParty {
            vendors: baked_in.clone(),
        }
    } else if let Some(srd) = steam_release_drm.as_deref() {
        let l = srd.to_lowercase();
        if is_drm_free_token(&l) {
            DrmStatus::DrmFree
        } else if l == "steam" {
            DrmStatus::SteamOnly
        } else if l.is_empty() {
            fallback_classify(&raw)
        } else {
            // Position resolved to a store-wrapper that isn't Steam: weird, treat as Unknown.
            DrmStatus::Unknown
        }
    } else {
        fallback_classify(&raw)
    };

    let affects_steam_copy = matches!(&status, DrmStatus::ThirdParty { .. } | DrmStatus::SteamOnly);
    let explanation = impact_explanation(&status);

    // Add any detected store-only vendors (MS Store, Epic Games) to notes for visibility.
    let mut store_only: Vec<String> = Vec::new();
    for token in raw.pcgw_uses.iter() {
        add_vendors(&mut store_only, &detect_store_vendors(token));
    }
    if !store_only.is_empty() {
        notes_parts.push(format!(
            "DRMs de otros stores (NO afectan Steam): {}",
            store_only.join(", ")
        ));
    }

    let source = match (raw.pcgw_ok && raw.pcgw_has_entry, raw.steam_store_ok) {
        (true, true) => "merged",
        (true, false) => "pcgamingwiki",
        (false, true) => "steam",
        _ => "none",
    };

    let preservability = classify_preservability(&raw, &status);
    let preservability_hint = preservability_hint(&preservability);

    DrmInfo {
        status,
        notes: notes_parts.join(" | "),
        source: source.into(),
        fetched_at,
        affects_steam_copy,
        explanation,
        preservability,
        preservability_hint,
        stores: raw.pcgw_stores,
        removed_drm: raw.pcgw_removed,
    }
}

/// Classify how feasible preservation of a Steam copy is, given the DRM
/// status and the raw PCGamingWiki data (stores availability and removed DRMs).
/// Priority when status is ThirdParty:
/// 1. GOG availability — buying / redeeming on GOG is the cleanest legal path.
/// 2. Removed DRM — the publisher already removed heavy DRM from the current release.
/// 3. Otherwise Hard — no documented clean path.
fn classify_preservability(raw: &RawDrm, status: &DrmStatus) -> Preservability {
    match status {
        DrmStatus::DrmFree => Preservability::Trivial,
        DrmStatus::SteamOnly => Preservability::Easy,
        DrmStatus::ThirdParty { vendors: _ } => {
            // GOG is DRM-free by store policy: if the game is sold on GOG,
            // there is an official legal preservation path regardless of the
            // Steam release's DRM.
            if raw.pcgw_stores.iter().any(|s| is_gog_store(s)) {
                return Preservability::Alternative;
            }

            // Any baked-in DRM officially removed post-launch?
            let mut removed_vendors: Vec<String> = Vec::new();
            for r in &raw.pcgw_removed {
                for v in detect_baked_in_vendors(r) {
                    if !removed_vendors.contains(&v) {
                        removed_vendors.push(v);
                    }
                }
            }
            if !removed_vendors.is_empty() {
                return Preservability::Removed { removed_vendors };
            }

            Preservability::Hard
        }
        DrmStatus::Unknown => Preservability::Unknown,
    }
}

/// Return the DRM token for the Steam release via positional alignment with
/// Available_from. Only returns Some(_) if the two lists share length and a
/// Steam-ish store entry is present.
fn aligned_steam_drm(stores: &[String], uses: &[String]) -> Option<String> {
    if stores.is_empty() || uses.is_empty() || stores.len() != uses.len() {
        return None;
    }
    let pos = stores.iter().position(|s| is_steam_store(s))?;
    uses.get(pos).cloned()
}

fn is_steam_store(s: &str) -> bool {
    let l = s.trim().to_lowercase();
    l == "steam" || l == "steamworks" || l.starts_with("steam ")
}

fn is_gog_store(s: &str) -> bool {
    let l = s.trim().to_lowercase();
    l == "gog.com" || l == "gog" || l.starts_with("gog ")
}

fn is_drm_free_token(lower: &str) -> bool {
    let t = lower.trim();
    t == "drm-free" || t == "drm free" || t == "drmfree"
}

/// Fallback classification when positional alignment is not usable.
fn fallback_classify(raw: &RawDrm) -> DrmStatus {
    let all_tokens: Vec<String> = raw
        .pcgw_uses
        .iter()
        .chain(raw.pcgw_retail.iter())
        .cloned()
        .collect();

    let has_drm_free = all_tokens
        .iter()
        .any(|t| is_drm_free_token(&t.to_lowercase()));
    let has_steam = all_tokens.iter().any(|t| t.to_lowercase() == "steam");

    // If the wiki explicitly has DRM-free entries across releases AND no Steam
    // token appears, trust the DRM-free signal.
    if has_drm_free && !has_steam {
        return DrmStatus::DrmFree;
    }
    if has_steam {
        return DrmStatus::SteamOnly;
    }
    if raw.steam_drm_notice.is_some() || raw.steam_account_notice.is_some() {
        // Steam Store reported something unusual we did not map to a vendor.
        return DrmStatus::SteamOnly;
    }
    DrmStatus::Unknown
}

fn impact_explanation(status: &DrmStatus) -> String {
    match status {
        DrmStatus::DrmFree => "Sin DRM. Tu copia de Steam es completamente libre: el ejecutable \
            corre sin el cliente de Steam abierto ni verificación online."
            .to_string(),
        DrmStatus::SteamOnly => "Solo usa el wrapper de Steam DRM (removible a nivel cliente). \
            No hay DRM de terceros. No afecta tu capacidad de hacer backup de manera práctica."
            .to_string(),
        DrmStatus::ThirdParty { vendors } => format!(
            "Tu copia de Steam está afectada por: {}. Estos DRMs están embebidos en el binario y \
             funcionan en TODOS los releases (Steam, Epic, MS Store, etc.), no solo donde los \
             compraste.",
            vendors.join(", ")
        ),
        DrmStatus::Unknown => "Sin información suficiente para clasificar el DRM de la copia de \
            Steam. PCGamingWiki no tiene datos o Steam no declaró DRM. Puede ser DRM-free, solo \
            Steam, o tener DRM no detectado."
            .to_string(),
    }
}
