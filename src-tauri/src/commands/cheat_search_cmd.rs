use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

const FEARLESS_TABLES_FORUM_ID: &str = "4";

fn fearless_search_url(game_name: &str) -> String {
    let encoded = urlencoding::encode(game_name);
    format!(
        "https://fearlessrevolution.com/search.php?keywords={encoded}&fid%5B%5D={FEARLESS_TABLES_FORUM_ID}&sf=titleonly&sr=topics"
    )
}

#[tauri::command]
pub fn open_fearless_search(app: AppHandle, game_name: String) -> Result<String, String> {
    let url = fearless_search_url(&game_name);
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_targets_fearless_search() {
        let u = fearless_search_url("Ender Magnolia");
        assert!(u.starts_with("https://fearlessrevolution.com/search.php?"));
    }

    #[test]
    fn url_carries_game_name_in_keywords() {
        let u = fearless_search_url("Ender Magnolia");
        assert!(u.contains("keywords=Ender%20Magnolia"));
    }

    #[test]
    fn url_filters_to_tables_subforum() {
        let u = fearless_search_url("anything");
        assert!(u.contains("fid%5B%5D=4"));
    }

    #[test]
    fn url_requests_title_only_topics() {
        let u = fearless_search_url("anything");
        assert!(u.contains("sf=titleonly"));
        assert!(u.contains("sr=topics"));
    }

    #[test]
    fn url_encodes_special_characters() {
        let u = fearless_search_url("Yakuza: Like a Dragon");
        assert!(!u.contains(": "));
        assert!(u.contains("Yakuza%3A%20Like%20a%20Dragon"));
    }

    #[test]
    fn url_encodes_ampersand() {
        let u = fearless_search_url("Beyond Good & Evil");
        assert!(u.contains("Beyond%20Good%20%26%20Evil"));
    }
}
