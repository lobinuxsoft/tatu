const PCGW_API: &str = "https://www.pcgamingwiki.com/w/api.php";
const STEAM_APPDETAILS: &str = "https://store.steampowered.com/api/appdetails";
const USER_AGENT: &str =
    "game-progress-tracker (+https://github.com/lobinuxsoft/game-progress-tracker)";

/// Data returned by a successful PCGamingWiki query. Stored lists preserve
/// order and empty slots so positional alignment (Stores ↔ Uses_DRM) stays
/// intact downstream.
pub(super) struct PcgwDrm {
    pub stores: Vec<String>,
    pub uses: Vec<String>,
    pub removed: Vec<String>,
    pub retail: Vec<String>,
    pub has_entry: bool,
}

/// Query Steam Store appdetails and extract DRM-related fields.
/// Returns (drm_notice, ext_user_account_notice).
pub(super) fn fetch_from_steam(app_id: u64) -> Option<(Option<String>, Option<String>)> {
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

/// Query PCGamingWiki's Cargo API joining Infobox_game with Availability.
/// Pulls Available_from (stores), Uses_DRM, Removed_DRM, Retail_DRM so we
/// can attempt positional alignment Store ↔ DRM.
pub(super) fn fetch_from_pcgamingwiki(app_id: u64) -> Option<PcgwDrm> {
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
