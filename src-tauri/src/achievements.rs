use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Achievement {
    pub api_name: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub icon_gray: String,
    pub achieved: bool,
    pub unlock_time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameAchievements {
    pub app_id: u64,
    pub achievements: Vec<Achievement>,
    pub last_max_unlock_time: u64,
    pub fetched_at: u64,
}

/// Fetch full achievement data for a game: player unlock status + schema (names/icons).
pub fn fetch_game_achievements(
    api_key: &str,
    steam_id: &str,
    app_id: u64,
) -> Result<GameAchievements, String> {
    // 1. Player achievements (unlock status).
    let player_url = format!(
        "https://api.steampowered.com/ISteamUserStats/GetPlayerAchievements/v0001/\
         ?appid={app_id}&key={api_key}&steamid={steam_id}&format=json"
    );
    let player_body: serde_json::Value = match ureq::get(&player_url).call() {
        Ok(resp) => resp
            .into_body()
            .read_json()
            .map_err(|e| format!("Failed to parse player achievements: {e}"))?,
        Err(ureq::Error::StatusCode(403)) => {
            return Err("Profile is not public".to_string());
        }
        Err(e) => return Err(format!("Failed to fetch player achievements: {e}")),
    };

    if player_body["playerstats"]["success"].as_bool() != Some(true) {
        let err = player_body["playerstats"]["error"]
            .as_str()
            .unwrap_or("Unknown error");
        return Err(err.to_string());
    }

    let player_achs = player_body["playerstats"]["achievements"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut status_map: std::collections::HashMap<String, (bool, u64)> =
        std::collections::HashMap::new();
    for ach in &player_achs {
        let name = ach["apiname"].as_str().unwrap_or("").to_string();
        let achieved = ach["achieved"].as_u64().unwrap_or(0) == 1;
        let unlock_time = ach["unlocktime"].as_u64().unwrap_or(0);
        status_map.insert(name, (achieved, unlock_time));
    }

    // 2. Schema (names, descriptions, icons).
    let schema_url = format!(
        "https://api.steampowered.com/ISteamUserStats/GetSchemaForGame/v2/\
         ?appid={app_id}&key={api_key}&l=english&format=json"
    );
    let schema_body: serde_json::Value = ureq::get(&schema_url)
        .call()
        .map_err(|e| format!("Failed to fetch achievement schema: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("Failed to parse achievement schema: {e}"))?;

    let schema_achs = schema_body["game"]["availableGameStats"]["achievements"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    // 3. Merge.
    let mut achievements = Vec::new();
    let mut max_unlock_time: u64 = 0;

    for sch in &schema_achs {
        let api_name = sch["name"].as_str().unwrap_or("").to_string();
        let (achieved, unlock_time) = status_map.get(&api_name).copied().unwrap_or((false, 0));

        if achieved && unlock_time > max_unlock_time {
            max_unlock_time = unlock_time;
        }

        achievements.push(Achievement {
            api_name,
            name: sch["displayName"].as_str().unwrap_or("").to_string(),
            description: sch["description"].as_str().unwrap_or("").to_string(),
            icon: sch["icon"].as_str().unwrap_or("").to_string(),
            icon_gray: sch["icongray"].as_str().unwrap_or("").to_string(),
            achieved,
            unlock_time,
        });
    }

    let fetched_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Ok(GameAchievements {
        app_id,
        achievements,
        last_max_unlock_time: max_unlock_time,
        fetched_at,
    })
}

/// Lightweight check: fetch only the max unlock time from player achievements.
pub fn fetch_max_unlock_time(api_key: &str, steam_id: &str, app_id: u64) -> Result<u64, String> {
    let url = format!(
        "https://api.steampowered.com/ISteamUserStats/GetPlayerAchievements/v0001/\
         ?appid={app_id}&key={api_key}&steamid={steam_id}&format=json"
    );
    let body: serde_json::Value = match ureq::get(&url).call() {
        Ok(resp) => resp
            .into_body()
            .read_json()
            .map_err(|e| format!("Failed to parse player achievements: {e}"))?,
        Err(ureq::Error::StatusCode(403)) => {
            return Err("Profile is not public".to_string());
        }
        Err(e) => return Err(format!("Failed to fetch player achievements: {e}")),
    };

    let achs = body["playerstats"]["achievements"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let max_time = achs
        .iter()
        .filter(|a| a["achieved"].as_u64() == Some(1))
        .filter_map(|a| a["unlocktime"].as_u64())
        .max()
        .unwrap_or(0);

    Ok(max_time)
}
