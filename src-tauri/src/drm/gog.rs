use super::hints::preservability_hint;
use super::types::{DrmInfo, Preservability};

const GOG_CATALOG_API: &str = "https://catalog.gog.com/v1/catalog";
const USER_AGENT: &str =
    "game-progress-tracker (+https://github.com/lobinuxsoft/game-progress-tracker)";

/// When PCGamingWiki, Steam Store, and the local-file probe (#238) all
/// leave a game `Unknown`, query GOG's own public catalog by title
/// directly — PCGamingWiki simply doesn't have an entry for a lot of
/// less-covered titles, and that gap says nothing about whether the game
/// is actually sold DRM-free on GOG.
///
/// Live-verified against Hellpoint (628670): `catalog.gog.com/v1/catalog
/// ?query=like:Hellpoint` is the same search endpoint the storefront
/// itself uses, no auth or special headers needed, and correctly returns
/// the base game (`productType: "game"`, title exactly `"Hellpoint"`)
/// alongside unrelated DLC/pack/spin-off entries that a looser match would
/// have false-positived on.
pub(super) fn upgrade_if_on_gog(steam_title: &str, info: DrmInfo) -> DrmInfo {
    if info.preservability != Preservability::Unknown {
        return info;
    }
    if !confirmed_on_gog(steam_title) {
        return info;
    }

    let mut info = info;
    info.preservability = Preservability::Alternative;
    info.preservability_hint = preservability_hint(&Preservability::Alternative);
    info.notes = if info.notes.is_empty() {
        "Confirmado en el catálogo de GOG por título".to_string()
    } else {
        format!(
            "{} | Confirmado en el catálogo de GOG por título",
            info.notes
        )
    };
    info
}

fn confirmed_on_gog(steam_title: &str) -> bool {
    let url = format!(
        "{GOG_CATALOG_API}?query=like:{}&limit=10",
        urlencoding::encode(steam_title)
    );
    let Ok(mut response) = ureq::get(&url).header("User-Agent", USER_AGENT).call() else {
        return false;
    };
    let Ok(body) = response.body_mut().read_json::<serde_json::Value>() else {
        return false;
    };
    let Some(products) = body.get("products").and_then(|v| v.as_array()) else {
        return false;
    };

    // GOG's own "game" productType, not a dlc/pack/movie — and an exact
    // title match after normalizing, not just a substring: "like:" search
    // also returns spin-offs and unrelated entries sharing a word.
    let target = normalize_title(steam_title);
    products.iter().any(|p| {
        p.get("productType").and_then(|v| v.as_str()) == Some("game")
            && p.get("title")
                .and_then(|v| v.as_str())
                .is_some_and(|t| normalize_title(t) == target)
    })
}

/// Deliberately simple — lowercase and alphanumeric-only, no attempt at
/// stripping "Ultimate Edition"-style suffixes. Live-verified this is
/// already enough (Hellpoint matches exactly on both stores); add more
/// normalization only once a real mismatch demonstrates the need.
fn normalize_title(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_case_and_punctuation() {
        assert_eq!(normalize_title("Hellpoint"), normalize_title("hellpoint"));
        assert_eq!(normalize_title("Portal 2"), normalize_title("PORTAL: 2!"));
    }

    #[test]
    fn distinct_titles_stay_distinct() {
        assert_ne!(
            normalize_title("Hellpoint"),
            normalize_title("Hellpoint: The Thespian Feast")
        );
    }

    #[test]
    fn already_classified_is_left_untouched() {
        let info = DrmInfo {
            status: super::super::types::DrmStatus::Unknown,
            notes: String::new(),
            source: "none".to_string(),
            fetched_at: 0,
            affects_steam_copy: false,
            explanation: String::new(),
            preservability: Preservability::Hard,
            preservability_hint: String::new(),
            stores: Vec::new(),
            removed_drm: Vec::new(),
        };
        let result = upgrade_if_on_gog("anything", info.clone());
        assert_eq!(result.preservability, info.preservability);
    }
}
