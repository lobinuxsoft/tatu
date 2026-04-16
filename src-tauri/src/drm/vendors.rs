/// Vendors whose DRM is embedded in the game binary — affects every release,
/// including Steam.
pub(super) fn detect_baked_in_vendors(text: &str) -> Vec<String> {
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
pub(super) fn detect_store_vendors(text: &str) -> Vec<String> {
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

/// Deduplicating append: push each element of `src` into `dst` only if not
/// already present. Used to accumulate vendor lists from multiple sources.
pub(super) fn add_vendors(dst: &mut Vec<String>, src: &[String]) {
    for v in src {
        if !dst.contains(v) {
            dst.push(v.clone());
        }
    }
}
