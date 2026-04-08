use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingCard {
    pub name: String,
    pub image_url: String,
    pub owned: bool,
    pub quantity: u32,
    pub series_info: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Badge {
    pub name: String,
    pub image_url: String,
    pub level: u32,
    pub xp: u32,
    pub foil: bool,
    pub owned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameCards {
    pub app_id: u64,
    pub cards: Vec<TradingCard>,
    pub badges: Vec<Badge>,
    pub user_badge_level: u32,
    pub user_badge_unlocked: String,
    pub fetched_at: u64,
}

/// Fetch trading card set for a game by scraping the Steam badge/gamecards page.
/// Returns all cards in the set with owned/unowned status.
pub fn fetch_game_cards(steam_id: &str, app_id: u64) -> Result<GameCards, String> {
    let url = format!(
        "https://steamcommunity.com/profiles/{steam_id}/gamecards/{app_id}"
    );

    let html = ureq::get(&url)
        .call()
        .map_err(|e| format!("Failed to fetch gamecards page: {e}"))?
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Failed to read gamecards page: {e}"))?;

    let cards = parse_gamecards_html(&html);
    let (user_badge_level, user_badge_unlocked) = parse_user_badge(&html);

    if cards.is_empty() {
        return Err("No trading cards found for this game".to_string());
    }

    // Fetch all badge levels from steamcardexchange.net.
    let mut badges = fetch_badge_levels(app_id).unwrap_or_default();

    // Mark owned badges based on user's current level.
    for badge in &mut badges {
        if !badge.foil {
            badge.owned = badge.level <= user_badge_level;
        }
    }

    let fetched_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Ok(GameCards {
        app_id,
        cards,
        badges,
        user_badge_level,
        user_badge_unlocked,
        fetched_at,
    })
}

fn parse_gamecards_html(html: &str) -> Vec<TradingCard> {
    let mut cards = Vec::new();
    let mut pos = 0;

    while let Some(card_start) = html[pos..].find("badge_card_set_card ") {
        let abs_start = pos + card_start;
        let card_class_end = abs_start + 40;

        // Determine owned/unowned from class.
        let class_region = &html[abs_start..card_class_end.min(html.len())];
        let owned = class_region.contains("owned") && !class_region.contains("unowned");

        // Find the next card block boundary.
        let block_end = html[abs_start + 20..]
            .find("badge_card_set_card ")
            .map(|p| abs_start + 20 + p)
            .unwrap_or(html.len());
        let block = &html[abs_start..block_end];

        // Extract image URL.
        let image_url = extract_between(block, "src=\"", "\"")
            .unwrap_or_default();

        // Extract quantity (e.g., "(2)").
        let quantity = extract_between(block, "badge_card_set_text_qty\">", "</div>")
            .and_then(|q| q.trim().trim_matches(|c| c == '(' || c == ')').parse::<u32>().ok())
            .unwrap_or(if owned { 1 } else { 0 });

        // Extract card name (after qty div or directly in title).
        let name = extract_card_name(block);

        // Extract series info (e.g., "5 of 15, Series 1").
        let series_info = extract_series_info(block);

        if !image_url.is_empty() {
            cards.push(TradingCard {
                name,
                image_url: image_url.to_string(),
                owned,
                quantity,
                series_info,
            });
        }

        pos = abs_start + 20;
    }

    cards
}

fn extract_card_name(block: &str) -> String {
    // The name is in the badge_card_set_title div, after any qty div.
    if let Some(title_start) = block.find("badge_card_set_title") {
        let after_title = &block[title_start..];
        // Find content after the last </div> of qty or after the title class.
        // The name text is between the qty div (if present) and <div style="clear:
        let name_region = if let Some(qty_end) = after_title.find("badge_card_set_text_qty") {
            // After the qty </div>
            let after_qty_div = &after_title[qty_end..];
            after_qty_div.find("</div>").map(|p| &after_qty_div[p + 6..])
        } else {
            // No qty, name is directly after the opening tag
            after_title.find('>').map(|p| &after_title[p + 1..])
        };

        if let Some(region) = name_region {
            if let Some(end) = region.find('<') {
                let name = region[..end].trim().to_string();
                if !name.is_empty() {
                    return name;
                }
            }
        }
    }
    String::from("Unknown")
}

fn extract_series_info(block: &str) -> String {
    // Series info is in the second badge_card_set_text div (not the title one).
    let mut found_first = false;
    let mut search_pos = 0;
    while let Some(idx) = block[search_pos..].find("badge_card_set_text") {
        let abs = search_pos + idx;
        let region = &block[abs..];
        if region.starts_with("badge_card_set_text ellipsis") && !region.starts_with("badge_card_set_text_") {
            if found_first {
                // This is the second one (series info).
                if let Some(content) = extract_between(region, ">", "</div>") {
                    let cleaned = content.trim().to_string();
                    if !cleaned.is_empty() {
                        return cleaned;
                    }
                }
            }
        }
        if region.starts_with("badge_card_set_text ") && !region.starts_with("badge_card_set_text_") {
            found_first = true;
        }
        search_pos = abs + 19;
    }
    String::new()
}

/// Parse the user's current badge level from the gamecards page.
fn parse_user_badge(html: &str) -> (u32, String) {
    let Some(badge_section) = html.find("badge_current") else { return (0, String::new()) };
    let region = &html[badge_section..];
    let region_end = region.find("badge_detail_tasks").unwrap_or(region.len());
    let region = &region[..region_end];

    if region.contains("badge_empty_circle") {
        return (0, String::new());
    }

    // Extract level from "Level X, Y XP".
    let level_text = extract_between(region, "badge_info_description\">", "badge_info_unlocked")
        .and_then(|s| extract_between(s, "<div>", "</div>"))
        .unwrap_or("");

    let level = level_text
        .split(|c: char| !c.is_ascii_digit())
        .find(|s| !s.is_empty())
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    let unlocked = extract_between(region, "badge_info_unlocked\">", "</div>")
        .unwrap_or("")
        .trim()
        .to_string();

    (level, unlocked)
}

/// Fetch all badge levels (normal + foil) from steamcardexchange.net.
fn fetch_badge_levels(app_id: u64) -> Result<Vec<Badge>, String> {
    let url = format!("https://www.steamcardexchange.net/index.php?gamepage-appid-{app_id}");

    let html = ureq::get(&url)
        .call()
        .map_err(|e| format!("Failed to fetch badge data: {e}"))?
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Failed to read badge data: {e}"))?;

    let mut badges = Vec::new();

    // Pattern: badge image with alt text, followed by name and level/XP info.
    let badge_img_prefix = format!(
        "https://steamcdn-a.akamaihd.net/steamcommunity/public/images/items/{app_id}/"
    );

    let mut pos = 0;
    let mut level_counter: u32 = 0;
    let xp_per_level = [100, 200, 300, 400, 500];

    while let Some(idx) = html[pos..].find(&badge_img_prefix) {
        let abs = pos + idx;
        // Extract image URL.
        let img_start = html[..abs].rfind("src=\"").map(|p| p + 5).unwrap_or(abs);
        let img_end = html[abs..].find('"').map(|p| abs + p).unwrap_or(abs);
        let image_url = html[img_start..img_end].to_string();

        // Extract name from alt attribute.
        let alt_region = &html[img_end..];
        let name = extract_between(&html[img_start - 5..img_end + 200], "alt=\"", "\"")
            .unwrap_or("Unknown");

        // Clean name: remove "Series X - " prefix.
        let clean_name = if let Some(dash_pos) = name.find(" - ") {
            name[dash_pos + 3..].trim()
        } else {
            name.trim()
        };

        level_counter += 1;
        let is_foil = level_counter > 5;
        let level = if is_foil { 1 } else { level_counter };
        let xp = if is_foil { 100 } else { xp_per_level.get((level - 1) as usize).copied().unwrap_or(100) };

        badges.push(Badge {
            name: clean_name.to_string(),
            image_url,
            level,
            xp,
            foil: is_foil,
            owned: false,
        });

        pos = img_end + 1;
    }

    Ok(badges)
}

fn extract_between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let s = text.find(start)? + start.len();
    let e = text[s..].find(end)? + s;
    Some(&text[s..e])
}
