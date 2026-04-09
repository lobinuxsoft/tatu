use std::time::Duration;

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

/// Fetch trading cards and badges for a game.
pub async fn fetch_game_cards(steam_id: String, app_id: u64) -> Result<GameCards, String> {
    tokio::task::spawn_blocking(move || fetch_game_cards_sync(&steam_id, app_id))
        .await
        .map_err(|e| format!("Task error: {e}"))?
}

fn fetch_game_cards_sync(steam_id: &str, app_id: u64) -> Result<GameCards, String> {
    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .build(),
    );

    // Fetch gamecards page from Steam.
    let url = format!(
        "https://steamcommunity.com/profiles/{steam_id}/gamecards/{app_id}"
    );
    eprintln!("[cards] requesting {url}");
    let html = agent
        .get(&url)
        .call()
        .map_err(|e| format!("Failed to fetch gamecards page: {e}"))?
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Failed to read gamecards page: {e}"))?;
    eprintln!("[cards] got {} bytes from gamecards page", html.len());

    let cards = parse_gamecards_html(&html);
    let (user_badge_level, user_badge_unlocked) = parse_user_badge(&html);

    if cards.is_empty() {
        return Err("No trading cards found for this game".to_string());
    }

    // Fetch full badge images from steamcardexchange.net.
    let badges = match fetch_badge_images_sce(app_id, user_badge_level) {
        Ok(b) if !b.is_empty() => {
            eprintln!("[cards] got {} badges from steamcardexchange", b.len());
            b
        }
        _ => {
            eprintln!("[cards] steamcardexchange failed, using page fallback");
            parse_badge_from_page(&html, user_badge_level)
        }
    };

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

/// Fetch badge images from steamcardexchange.net using HTTP/1.0 behavior (Connection: close).
fn fetch_badge_images_sce(app_id: u64, user_level: u32) -> Result<Vec<Badge>, String> {
    let url = format!("https://www.steamcardexchange.net/index.php?gamepage-appid-{app_id}");

    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .build(),
    );

    eprintln!("[badges] requesting {url}");
    let html = agent
        .get(&url)
        .header("Connection", "close")
        .header("Accept-Encoding", "identity")
        .call()
        .map_err(|e| format!("SCE request failed: {e}"))?
        .into_body()
        .read_to_string()
        .map_err(|e| format!("SCE read failed: {e}"))?;
    eprintln!("[badges] got {} bytes from steamcardexchange", html.len());

    // Badge images have alt attributes starting with "Series".
    // Pattern: <img ... src="URL" ... alt="Series X - Name">
    let badge_alt_marker = format!("/images/items/{app_id}/");
    let mut badges = Vec::new();
    let mut level_counter: u32 = 0;
    let xp_per_level = [100, 200, 300, 400, 500];

    // Find all <img> tags that have both the items path and a "Series" alt.
    let mut pos = 0;
    while let Some(img_tag_start) = html[pos..].find("<img") {
        let abs = pos + img_tag_start;
        let tag_end = match html[abs..].find('>') {
            Some(p) => abs + p,
            None => break,
        };
        let tag = &html[abs..tag_end + 1];

        // Check if this img tag contains our appid path AND a Series alt.
        if tag.contains(&badge_alt_marker) {
            if let Some(alt) = extract_between(tag, "alt=\"", "\"") {
                if alt.starts_with("Series") {
                    if let Some(src) = extract_between(tag, "src=\"", "\"") {
                        // Extract clean name from alt (remove "Series X - " and game name prefix).
                        let clean_name = alt.split(" - ").last().unwrap_or(alt).trim();

                        level_counter += 1;
                        let is_foil = level_counter > 5;
                        let level = if is_foil { 1 } else { level_counter };
                        let xp = if is_foil { 100 } else {
                            xp_per_level.get((level - 1) as usize).copied().unwrap_or(100)
                        };

                        badges.push(Badge {
                            name: clean_name.to_string(),
                            image_url: src.to_string(),
                            level,
                            xp,
                            foil: is_foil,
                            owned: if is_foil { false } else { level <= user_level },
                        });
                    }
                }
            }
        }

        pos = tag_end + 1;
    }

    eprintln!("[badges] parsed {} badges", badges.len());
    Ok(badges)
}

fn parse_gamecards_html(html: &str) -> Vec<TradingCard> {
    let mut cards = Vec::new();
    let mut pos = 0;

    while let Some(card_start) = html[pos..].find("badge_card_set_card ") {
        let abs_start = pos + card_start;
        let card_class_end = abs_start + 40;

        let class_region = &html[abs_start..card_class_end.min(html.len())];
        let owned = class_region.contains("owned") && !class_region.contains("unowned");

        let block_end = html[abs_start + 20..]
            .find("badge_card_set_card ")
            .map(|p| abs_start + 20 + p)
            .unwrap_or(html.len());
        let block = &html[abs_start..block_end];

        let image_url = extract_between(block, "src=\"", "\"")
            .unwrap_or_default();

        let quantity = extract_between(block, "badge_card_set_text_qty\">", "</div>")
            .and_then(|q| q.trim().trim_matches(|c| c == '(' || c == ')').parse::<u32>().ok())
            .unwrap_or(if owned { 1 } else { 0 });

        let name = extract_card_name(block);
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

fn parse_user_badge(html: &str) -> (u32, String) {
    let Some(badge_section) = html.find("badge_current") else { return (0, String::new()) };
    let region = &html[badge_section..];
    let region_end = region.find("badge_detail_tasks").unwrap_or(region.len());
    let region = &region[..region_end];

    if region.contains("badge_empty_circle") {
        return (0, String::new());
    }

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

fn parse_badge_from_page(html: &str, user_level: u32) -> Vec<Badge> {
    let mut badges = Vec::new();
    let xp_per_level = [100, 200, 300, 400, 500];

    let badge_section = html.find("badge_current");
    let current_image = badge_section.and_then(|start| {
        let region = &html[start..];
        extract_between(region, "src=\"", "\"")
            .filter(|url| url.contains("/images/items/"))
            .map(|s| s.to_string())
    });

    let current_name = badge_section.and_then(|start| {
        let region = &html[start..];
        if region.contains("badge_info_title") {
            extract_between(region, "badge_info_title\">", "</div>")
                .map(|s| s.trim().to_string())
        } else if region.contains("badge_empty_name") {
            extract_between(region, "badge_empty_name\">", "</div>")
                .map(|s| s.trim().to_string())
        } else {
            None
        }
    });

    for level in 1..=5u32 {
        let owned = level <= user_level;
        let image_url = if owned && level == user_level {
            current_image.clone().unwrap_or_default()
        } else {
            String::new()
        };
        let name = if level == user_level || (level == user_level + 1 && user_level < 5) {
            current_name.clone().unwrap_or_else(|| format!("Nivel {level}"))
        } else {
            format!("Nivel {level}")
        };

        badges.push(Badge {
            name,
            image_url,
            level,
            xp: xp_per_level[(level - 1) as usize],
            foil: false,
            owned,
        });
    }

    badges
}

fn extract_card_name(block: &str) -> String {
    if let Some(title_start) = block.find("badge_card_set_title") {
        let after_title = &block[title_start..];
        let name_region = if let Some(qty_end) = after_title.find("badge_card_set_text_qty") {
            let after_qty_div = &after_title[qty_end..];
            after_qty_div.find("</div>").map(|p| &after_qty_div[p + 6..])
        } else {
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
    let mut found_first = false;
    let mut search_pos = 0;
    while let Some(idx) = block[search_pos..].find("badge_card_set_text") {
        let abs = search_pos + idx;
        let region = &block[abs..];
        if region.starts_with("badge_card_set_text ellipsis") && !region.starts_with("badge_card_set_text_") {
            if found_first {
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

fn extract_between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let s = text.find(start)? + start.len();
    let e = text[s..].find(end)? + s;
    Some(&text[s..e])
}
