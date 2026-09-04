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

/// DRM-related fields pulled from a Steam Store appdetails response, plus
/// the game's own title — needed to query GOG's catalog by name (#237)
/// without a second round-trip to Steam just to get it.
pub(super) struct SteamStoreDrm {
    pub drm_notice: Option<String>,
    pub account_notice: Option<String>,
    pub name: Option<String>,
}

/// Query Steam Store appdetails and extract DRM-related fields.
pub(super) fn fetch_from_steam(app_id: u64) -> Option<SteamStoreDrm> {
    let url = format!("{STEAM_APPDETAILS}?appids={app_id}&l=english");
    let body: serde_json::Value = ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .call()
        .ok()?
        .body_mut()
        .read_json()
        .ok()?;

    let data = body.get(app_id.to_string())?.get("data")?;
    let drm_notice = data
        .get("drm_notice")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let account_notice = data
        .get("ext_user_account_notice")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let name = data
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Some(SteamStoreDrm {
        drm_notice,
        account_notice,
        name,
    })
}

/// Logs into PCGamingWiki with a bot password (`Special:BotPasswords`) —
/// required for `cargoquery` since their August 2026 server migration
/// locked it behind auth (anonymous queries now return `permissiondenied`,
/// confirmed live against their own docs example). `username` is the full
/// `user@botname` form the bot password page gives you. The returned
/// `Agent` carries the session cookie for every later call made through it
/// (`cookies` cargo feature) — log in once per bulk run, not per game, or
/// the extra login round-trip eats into PCGW's 60 req/min budget for
/// nothing.
pub fn login_pcgw(username: &str, bot_password: &str) -> Option<ureq::Agent> {
    if username.is_empty() || bot_password.is_empty() {
        return None;
    }
    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .user_agent(USER_AGENT)
            .build(),
    );

    let token_url = format!("{PCGW_API}?action=query&meta=tokens&type=login&format=json");
    let token_body: serde_json::Value = agent
        .get(&token_url)
        .call()
        .ok()?
        .body_mut()
        .read_json()
        .ok()?;
    let login_token = token_body
        .get("query")?
        .get("tokens")?
        .get("logintoken")?
        .as_str()?
        .to_string();

    let login_url = format!("{PCGW_API}?action=login&format=json");
    let login_body: serde_json::Value = agent
        .post(&login_url)
        .send_form([
            ("lgname", username),
            ("lgpassword", bot_password),
            ("lgtoken", &login_token),
        ])
        .ok()?
        .body_mut()
        .read_json()
        .ok()?;

    (login_body.get("login")?.get("result")?.as_str()? == "Success").then_some(agent)
}

/// Query PCGamingWiki's Cargo API joining Game with Availability. Pulls
/// Present (stores), Uses_DRM, Removed_DRM, Retail_DRM so we can attempt
/// positional alignment Store ↔ DRM. `Infobox_game`/`Available_from` were
/// renamed to `Game`/`Present` in PCGW's August 2026 migration — same
/// migration that added the auth requirement `agent` satisfies.
pub(super) fn fetch_from_pcgamingwiki(agent: &ureq::Agent, app_id: u64) -> Option<PcgwDrm> {
    let url = format!(
        "{PCGW_API}?action=cargoquery\
         &tables=Game,Availability\
         &join_on=Game._pageName=Availability._pageName\
         &fields=Availability.Present=Stores,Availability.Uses_DRM=UsesDRM,Availability.Removed_DRM=RemovedDRM,Availability.Retail_DRM=RetailDRM\
         &where=Game.Steam_AppID%20HOLDS%20%22{app_id}%22\
         &format=json"
    );

    let body: serde_json::Value = agent.get(&url).call().ok()?.body_mut().read_json().ok()?;

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
