use serde::{Deserialize, Serialize};

const PCGW_API: &str = "https://www.pcgamingwiki.com/w/api.php";
const STEAM_APPDETAILS: &str = "https://store.steampowered.com/api/appdetails";
const USER_AGENT: &str =
    "game-progress-tracker (+https://github.com/lobinuxsoft/game-progress-tracker)";

/// High-level DRM classification for a Steam title, from the perspective of
/// a copy purchased on Steam.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DrmStatus {
    DrmFree,
    SteamOnly,
    ThirdParty { vendors: Vec<String> },
    Unknown,
}

/// Preservability level: how feasible is it to keep a playable copy of the
/// game independent of Steam?
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Preservability {
    /// No DRM: copying the install folder is enough.
    Trivial,
    /// Only Steam wrapper DRM: Goldberg Steam Emu + Steamless cover it.
    Easy,
    /// Game is sold DRM-free on GOG (official legal alternative).
    Alternative,
    /// Publisher removed the DRM post-launch: the current Steam release is
    /// already preservable without extra tools.
    Removed { removed_vendors: Vec<String> },
    /// Third-party DRM active without a documented clean path.
    Hard,
    /// Insufficient data.
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrmInfo {
    pub status: DrmStatus,
    pub notes: String,
    pub source: String,
    pub fetched_at: u64,
    /// Whether the detected DRM affects the user's Steam-purchased copy.
    /// False for `DrmFree`; true for `SteamOnly` and `ThirdParty`.
    /// `Unknown` is conservatively reported as false.
    #[serde(default)]
    pub affects_steam_copy: bool,
    /// Human-readable explanation (Spanish) about Steam copy impact.
    #[serde(default)]
    pub explanation: String,
    /// Preservability classification (Goldberg compatibility, GOG alt, DRM removal).
    #[serde(default)]
    pub preservability: Preservability,
    /// Human-readable hint (Spanish) for the preservability level.
    #[serde(default)]
    pub preservability_hint: String,
    /// Raw PCGamingWiki Available_from tokens (stores), retained so the
    /// classifier can be re-run offline without a fresh API call.
    #[serde(default)]
    pub stores: Vec<String>,
    /// Raw PCGamingWiki Removed_DRM tokens (DRMs the publisher removed
    /// post-launch), retained for re-classification and user visibility.
    #[serde(default)]
    pub removed_drm: Vec<String>,
}

/// Raw data fetched from upstream sources before merging.
#[derive(Debug, Default, Clone)]
struct RawDrm {
    pcgw_stores: Vec<String>,
    pcgw_uses: Vec<String>,
    pcgw_removed: Vec<String>,
    pcgw_retail: Vec<String>,
    steam_drm_notice: Option<String>,
    steam_account_notice: Option<String>,
    steam_store_ok: bool,
    pcgw_ok: bool,
    pcgw_has_entry: bool,
}

/// Fetch DRM information for a Steam app ID, querying Steam Store and
/// PCGamingWiki and merging the results with a Steam-copy-centric heuristic.
pub fn fetch_drm_info(app_id: u64) -> Result<DrmInfo, String> {
    let mut raw = RawDrm::default();

    if let Some((notice, account)) = fetch_from_steam(app_id) {
        raw.steam_drm_notice = notice;
        raw.steam_account_notice = account;
        raw.steam_store_ok = true;
    }

    if let Some(pcgw) = fetch_from_pcgamingwiki(app_id) {
        raw.pcgw_stores = pcgw.stores;
        raw.pcgw_uses = pcgw.uses;
        raw.pcgw_removed = pcgw.removed;
        raw.pcgw_retail = pcgw.retail;
        raw.pcgw_has_entry = pcgw.has_entry;
        raw.pcgw_ok = true;
    }

    if !raw.steam_store_ok && !raw.pcgw_ok {
        return Err("Both Steam Store and PCGamingWiki requests failed".into());
    }

    Ok(merge(raw))
}

/// Query Steam Store appdetails and extract DRM-related fields.
fn fetch_from_steam(app_id: u64) -> Option<(Option<String>, Option<String>)> {
    let url = format!("{STEAM_APPDETAILS}?appids={app_id}&l=english");
    let body: serde_json::Value = ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .call()
        .ok()?
        .body_mut()
        .read_json()
        .ok()?;

    let data = body.get(app_id.to_string())?.get("data")?;
    let notice = data
        .get("drm_notice")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let account = data
        .get("ext_user_account_notice")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Some((notice, account))
}

struct PcgwDrm {
    stores: Vec<String>,
    uses: Vec<String>,
    removed: Vec<String>,
    retail: Vec<String>,
    has_entry: bool,
}

/// Query PCGamingWiki's Cargo API joining Infobox_game with Availability.
/// Pulls Available_from (stores), Uses_DRM, Removed_DRM, Retail_DRM so we
/// can attempt positional alignment Store ↔ DRM.
fn fetch_from_pcgamingwiki(app_id: u64) -> Option<PcgwDrm> {
    let url = format!(
        "{PCGW_API}?action=cargoquery\
         &tables=Infobox_game,Availability\
         &join_on=Infobox_game._pageName=Availability._pageName\
         &fields=Availability.Available_from=Stores,Availability.Uses_DRM=UsesDRM,Availability.Removed_DRM=RemovedDRM,Availability.Retail_DRM=RetailDRM\
         &where=Infobox_game.Steam_AppID%20HOLDS%20%22{app_id}%22\
         &format=json"
    );

    let body: serde_json::Value = ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .call()
        .ok()?
        .body_mut()
        .read_json()
        .ok()?;

    let rows = body.get("cargoquery")?.as_array()?;
    let has_entry = !rows.is_empty();
    let mut stores: Vec<String> = Vec::new();
    let mut uses: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    let mut retail: Vec<String> = Vec::new();

    for row in rows {
        let Some(title) = row.get("title") else {
            continue;
        };
        collect_csv_preserve_order(&mut stores, title.get("Stores"));
        collect_csv_preserve_order(&mut uses, title.get("UsesDRM"));
        collect_csv_preserve_order(&mut removed, title.get("RemovedDRM"));
        collect_csv_preserve_order(&mut retail, title.get("RetailDRM"));
    }

    Some(PcgwDrm {
        stores,
        uses,
        removed,
        retail,
        has_entry,
    })
}

/// Split a CSV-ish string into tokens, preserving order and empty slots.
/// Empty slots are kept so positional alignment stays intact.
fn collect_csv_preserve_order(out: &mut Vec<String>, value: Option<&serde_json::Value>) {
    let Some(s) = value.and_then(|v| v.as_str()) else {
        return;
    };
    for part in s.split(',') {
        out.push(part.trim().to_string());
    }
}

/// Merge Steam Store + PCGamingWiki raw data into a final DrmInfo.
fn merge(raw: RawDrm) -> DrmInfo {
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

    let explanation = match &status {
        DrmStatus::DrmFree => {
            "Sin DRM. Tu copia de Steam es completamente libre: el ejecutable corre sin el cliente \
             de Steam abierto ni verificación online."
                .to_string()
        }
        DrmStatus::SteamOnly => {
            "Solo usa el wrapper de Steam DRM (removible a nivel cliente). No hay DRM de terceros. \
             No afecta tu capacidad de hacer backup de manera práctica."
                .to_string()
        }
        DrmStatus::ThirdParty { vendors } => format!(
            "Tu copia de Steam está afectada por: {}. Estos DRMs están embebidos en el binario y \
             funcionan en TODOS los releases (Steam, Epic, MS Store, etc.), no solo donde los \
             compraste.",
            vendors.join(", ")
        ),
        DrmStatus::Unknown => {
            "Sin información suficiente para clasificar el DRM de la copia de Steam. PCGamingWiki \
             no tiene datos o Steam no declaró DRM. Puede ser DRM-free, solo Steam, o tener DRM no \
             detectado."
                .to_string()
        }
    };

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
        fetched_at: now_secs(),
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

fn is_gog_store(s: &str) -> bool {
    let l = s.trim().to_lowercase();
    l == "gog.com" || l == "gog" || l.starts_with("gog ")
}

/// Spanish human-readable hint describing the preservability level and the
/// concrete action the user can take.
fn preservability_hint(pres: &Preservability) -> String {
    match pres {
        Preservability::Trivial => "Preservación trivial: el juego no tiene DRM. Copiá la carpeta \
            de steamapps/common/<juego> a otro disco. No requiere herramientas."
            .into(),
        Preservability::Easy => "Compatible con Goldberg Emulator: el juego solo usa el wrapper de \
            Steam DRM. Con Goldberg (reemplazo de steam_api.dll) más Steamless (si tiene SteamStub) \
            corre offline sin el cliente de Steam."
            .into(),
        Preservability::Alternative => "Disponible DRM-free en GOG: alternativa oficial y legal \
            sin DRM. Considerá comprarlo/reclamarlo en GOG para tener una copia portable y \
            preservable sin depender de Steam."
            .into(),
        Preservability::Removed { removed_vendors } => format!(
            "DRM removido oficialmente: el publisher removió {} de la versión actual. La copia de \
             Steam ya es directamente preservable sin DRM activo.",
            removed_vendors.join(", ")
        ),
        Preservability::Hard => "Preservación compleja: el juego tiene DRM embebido activo sin \
            alternativa limpia documentada. Requeriría un crack específico del vendor — fuera del \
            alcance de esta herramienta."
            .into(),
        Preservability::Unknown => "Preservabilidad desconocida: sin datos suficientes para \
            clasificar. Puede variar desde trivial hasta compleja — refrescá los datos de DRM o \
            consultá manualmente en PCGamingWiki."
            .into(),
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

fn add_vendors(dst: &mut Vec<String>, src: &[String]) {
    for v in src {
        if !dst.contains(v) {
            dst.push(v.clone());
        }
    }
}

/// Vendors whose DRM is embedded in the game binary — affects every release,
/// including Steam.
fn detect_baked_in_vendors(text: &str) -> Vec<String> {
    const PATTERNS: &[(&str, &str)] = &[
        ("denuvo", "Denuvo"),
        ("ubisoft connect", "Ubisoft Connect"),
        ("uplay", "Ubisoft Connect"),
        ("ea app", "EA App"),
        ("ea desktop", "EA App"),
        ("origin", "EA App"),
        ("rockstar games social club", "Rockstar Launcher"),
        ("rockstar games launcher", "Rockstar Launcher"),
        ("social club", "Rockstar Launcher"),
        ("battle.net", "Battle.net"),
        ("games for windows live", "GFWL"),
        ("gfwl", "GFWL"),
        ("bethesda.net", "Bethesda.net"),
        ("securom", "SecuROM"),
        ("starforce", "StarForce"),
        ("tages", "TAGES"),
        ("vmprotect", "VMProtect"),
        ("safedisc", "SafeDisc"),
        ("arxan", "Arxan"),
    ];
    match_patterns(text, PATTERNS)
}

/// Vendors whose DRM only applies to their own store's release — NOT Steam.
fn detect_store_vendors(text: &str) -> Vec<String> {
    const PATTERNS: &[(&str, &str)] = &[
        ("epic games launcher", "Epic Games"),
        ("epic games store", "Epic Games"),
        ("epic games", "Epic Games"),
        ("microsoft store", "Microsoft Store"),
        ("gog.com", "GOG"),
        ("gog galaxy", "GOG"),
    ];
    match_patterns(text, PATTERNS)
}

fn match_patterns(text: &str, patterns: &[(&str, &str)]) -> Vec<String> {
    let t = text.to_lowercase();
    let mut out = Vec::new();
    for (needle, label) in patterns {
        if t.contains(needle) {
            let label_s = label.to_string();
            if !out.contains(&label_s) {
                out.push(label_s);
            }
        }
    }
    out
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
