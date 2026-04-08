use serde::{Deserialize, Serialize};

const STEAM_KEY: &str = "A8D0E682CAA040522E22151861FB71C1";
const STEAM_ID: &str = "76561198261354179";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: u64,
    pub name: String,
    pub hours: f64,
    pub icon: String,
    /// Category: "" = game, "tool", "mp", "demo".
    pub tag: String,
    /// Genre tags from Steam store (e.g. "Action", "RPG").
    #[serde(default)]
    pub genres: Vec<String>,
    /// Total number of achievements, 0 if none.
    #[serde(default)]
    pub achievements: u32,
    /// Whether the game has Steam trading cards.
    #[serde(default)]
    pub has_cards: bool,
    /// Header image URL from Steam store.
    #[serde(default)]
    pub header_img: String,
}

pub fn fetch_games() -> Result<Vec<Game>, String> {
    let url = format!(
        "https://api.steampowered.com/IPlayerService/GetOwnedGames/v0001/\
         ?key={STEAM_KEY}&steamid={STEAM_ID}&format=json\
         &include_appinfo=true&include_played_free_games=true"
    );

    let body: serde_json::Value = ureq::get(&url)
        .call()
        .map_err(|e| format!("HTTP error: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("JSON parse error: {e}"))?;

    let raw_games = body["response"]["games"]
        .as_array()
        .ok_or("No games array in Steam response")?;

    let mut games: Vec<Game> = raw_games
        .iter()
        .filter_map(|g| {
            let id = g["appid"].as_u64()?;
            let name = g["name"].as_str().unwrap_or("Unknown").to_string();
            let minutes = g["playtime_forever"].as_u64().unwrap_or(0);
            let hours = (minutes as f64 / 60.0 * 10.0).round() / 10.0;
            let icon = g["img_icon_url"].as_str().unwrap_or("").to_string();
            let tag = classify(id, &name);
            Some(Game {
                id,
                name,
                hours,
                icon,
                tag,
                genres: vec![],
                achievements: 0,
                has_cards: false,
                header_img: String::new(),
            })
        })
        .collect();

    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(games)
}

/// Fetch details from the Steam Store API for a batch of app IDs.
/// Steam rate-limits to ~200 requests per 5 minutes, so we sleep between calls.
/// `on_progress` is called after each game with (current, total).
pub fn fetch_details_for(
    games: &mut [Game],
    on_progress: impl Fn(usize, usize),
) {
    let total = games.len();
    for (i, game) in games.iter_mut().enumerate() {
        // Skip games that already have genres loaded.
        if !game.genres.is_empty() {
            on_progress(i + 1, total);
            continue;
        }

        let url = format!(
            "https://store.steampowered.com/api/appdetails?appids={}&l=english",
            game.id
        );

        if let Ok(resp) = ureq::get(&url).call() {
            if let Ok(body) = resp.into_body().read_json::<serde_json::Value>() {
                let key = game.id.to_string();
                if let Some(data) = body.get(&key).and_then(|v| v.get("data")) {
                    // Genres.
                    if let Some(arr) = data["genres"].as_array() {
                        game.genres = arr
                            .iter()
                            .filter_map(|g| g["description"].as_str().map(String::from))
                            .collect();
                    }

                    // Achievements.
                    if let Some(total) = data["achievements"]["total"].as_u64() {
                        game.achievements = total as u32;
                    }

                    // Trading cards: look for "Steam Trading Cards" in categories.
                    if let Some(cats) = data["categories"].as_array() {
                        game.has_cards = cats.iter().any(|c| {
                            c["id"].as_u64() == Some(29) // 29 = Steam Trading Cards
                        });
                    }

                    // Header image.
                    if let Some(img) = data["header_image"].as_str() {
                        game.header_img = img.to_string();
                    }

                    // Auto-classify type from Steam if we haven't hardcoded it.
                    if game.tag.is_empty() {
                        if let Some(app_type) = data["type"].as_str() {
                            match app_type {
                                "demo" => game.tag = "demo".into(),
                                "mod" => game.tag = "demo".into(),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        on_progress(i + 1, total);

        // Rate limit: ~300ms between requests to stay under Steam limits.
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}

fn classify(id: u64, name: &str) -> String {
    const TOOLS: &[u64] = &[
        365670, 431730, 404790, 431960, 993090, 920490, 235900, 943760, 1105890, 1364390, 382110,
        590830,
    ];
    if TOOLS.contains(&id) {
        return "tool".into();
    }

    const DEMO_IDS: &[u64] = &[3362820, 3413760, 3333090, 3448280, 950670, 3470630, 1902490];
    if DEMO_IDS.contains(&id) {
        return "demo".into();
    }
    let nl = name.to_lowercase();
    for w in ["playtest", "trial version", "playable teaser", "friend's pass"] {
        if nl.contains(w) {
            return "demo".into();
        }
    }
    if nl.contains(" demo") && !nl.contains("demon") && !nl.contains("demol") {
        return "demo".into();
    }

    const MP: &[u64] = &[
        730, 320, 360, 291550, 945360, 582660, 706220, 373680, 306130, 230410, 1085660, 744900,
        2139460, 1623660, 222520, 466240, 739630, 760160, 3097560, 844870, 1273710, 557180,
        1236300, 952070, 921590, 950700, 943150, 286160, 2141910, 1604030, 550, 493520, 359320,
        1203620, 728880, 976310, 1448840, 1403370, 582010, 2246340, 1335200, 588430, 322170,
        1722860,
    ];
    if MP.contains(&id) {
        return "mp".into();
    }

    String::new()
}
