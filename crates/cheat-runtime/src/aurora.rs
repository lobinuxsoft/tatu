//! Loader for Aurora SD Tool / CheatHappens trainer payloads.
//!
//! Aurora's `/api/downloadcheat/<trainerId>` endpoint returns a base64-encoded
//! DEFLATE-compressed JSON whose keys are scrambled with sequences of `;`,
//! `,` and `.` (a single trainer has ~100 unique keys, all visually
//! interchangeable). The wire-format flow lives outside this module (see the
//! aurora-logger pipeline at `~/.local/share/aurora-logger/`); by the time
//! this loader runs, the JSON is plain text on disk under
//! `~/Aurora/CapturedTrainers/downloadcheat/<id>__<utc>.json`.
//!
//! We don't need to *know* the obfuscated key names — we recognise the
//! fields by **what their values look like**:
//!
//! - `exe`: any string ending in `.exe` (case-insensitive).
//! - `appid`: integer in the Steam appid range (1..=10_000_000).
//! - `title_id`: integer outside that range when the appid was already
//!   claimed by another field (CheatHappens internal id).
//! - `title`: longest non-`.exe` non-platform string.
//! - `version`: string matching `<num>.<num>.<num>` or `<n>.<n>.<n>.<n>`.
//! - `features`: an array of dicts where each dict contains a UUID-shaped
//!   string (36 chars, four `-`).
//! - `scripts`: an array of dicts whose values contain a `[ENABLE]` string
//!   anywhere in the dict.
//!
//! Linking each user-facing feature to the script(s) that implement it is
//! **not documented by Aurora and not solved yet** (see [[project-aurora-
//! reverse]] in personal memory). We therefore expose `features` and
//! `scripts` as parallel lists and let callers decide how to bind them.

use std::path::Path;

use serde_json::Value;

const STEAM_APPID_MAX: u64 = 10_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Trainer {
    pub exe: String,
    pub title: String,
    pub version: String,
    pub appid: Option<u64>,
    pub title_id: Option<u64>,
    pub features: Vec<Feature>,
    pub scripts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feature {
    pub uuid: String,
    pub name: String,
    pub category: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuroraError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("could not locate trainer object in payload (expected wrapping array)")]
    NoTrainerObject,
}

pub fn load_trainer_file<P: AsRef<Path>>(path: P) -> Result<Trainer, AuroraError> {
    let text = std::fs::read_to_string(path)?;
    load_trainer(&text)
}

pub fn load_trainer(json: &str) -> Result<Trainer, AuroraError> {
    let root: Value = serde_json::from_str(json)?;
    let trainer_obj = find_trainer_object(&root).ok_or(AuroraError::NoTrainerObject)?;
    Ok(extract_trainer(trainer_obj, &root))
}

/// Aurora wraps the trainer dict inside a top-level array. Walk the value
/// tree until we find an Object that *looks like* the trainer (has both a
/// `.exe` string and a features-like array).
fn find_trainer_object(v: &Value) -> Option<&Value> {
    if is_trainer_object(v) {
        return Some(v);
    }
    match v {
        Value::Array(items) => items.iter().find_map(find_trainer_object),
        Value::Object(map) => map.values().find_map(find_trainer_object),
        _ => None,
    }
}

fn is_trainer_object(v: &Value) -> bool {
    let Value::Object(map) = v else { return false };
    let has_exe = map.values().any(|val| match val {
        Value::String(s) => s.to_lowercase().ends_with(".exe"),
        _ => false,
    });
    let has_features = map.values().any(|val| array_has_uuid_dict(val));
    has_exe && has_features
}

fn array_has_uuid_dict(v: &Value) -> bool {
    let Value::Array(items) = v else { return false };
    items.iter().any(|item| dict_has_uuid_string(item))
}

fn dict_has_uuid_string(v: &Value) -> bool {
    let Value::Object(map) = v else { return false };
    map.values()
        .any(|val| matches!(val, Value::String(s) if looks_like_uuid(s)))
}

fn looks_like_uuid(s: &str) -> bool {
    s.len() == 36 && s.bytes().filter(|&b| b == b'-').count() == 4
}

fn extract_trainer(trainer: &Value, root: &Value) -> Trainer {
    let Value::Object(map) = trainer else {
        return Trainer::default();
    };

    let mut exe = String::new();
    let mut title = String::new();
    let mut version = String::new();
    let mut appid: Option<u64> = None;
    let mut title_id: Option<u64> = None;
    let mut features: Vec<Feature> = Vec::new();
    let mut scripts: Vec<String> = Vec::new();
    // Longest string that isn't the exe/platform/version: provisional title.
    let mut title_candidate: (usize, String) = (0, String::new());

    for val in map.values() {
        match val {
            Value::String(s) => {
                let lower = s.to_lowercase();
                if exe.is_empty() && lower.ends_with(".exe") {
                    exe = s.clone();
                } else if version.is_empty() && looks_like_version(s) {
                    version = s.clone();
                } else if !is_platform_marker(s) && s.len() > title_candidate.0 {
                    title_candidate = (s.len(), s.clone());
                }
            }
            Value::Number(n) => {
                if let Some(u) = n.as_u64() {
                    if appid.is_none() && (1..=STEAM_APPID_MAX).contains(&u) && u >= 100 {
                        appid = Some(u);
                    }
                }
            }
            Value::Array(items) => {
                if array_has_uuid_dict(val) {
                    features.extend(items.iter().filter_map(extract_feature));
                } else if array_has_script_dict(val) {
                    scripts.extend(items.iter().filter_map(extract_script));
                }
            }
            _ => {}
        }
    }

    if title.is_empty() {
        title = title_candidate.1;
    }

    // Outer envelope: walk the root for a likely titleId (separate from appid).
    title_id = find_title_id(root, appid);

    Trainer {
        exe,
        title,
        version,
        appid,
        title_id,
        features,
        scripts,
    }
}

fn extract_feature(v: &Value) -> Option<Feature> {
    let Value::Object(map) = v else { return None };
    let mut uuid: Option<String> = None;
    let mut name: Option<String> = None;
    let mut category: Option<String> = None;
    // Walk every value: strings catch UUID + name; nested dicts often
    // carry the user-facing name in a single sub-string field.
    for val in map.values() {
        match val {
            Value::String(s) => {
                if uuid.is_none() && looks_like_uuid(s) {
                    uuid = Some(s.clone());
                } else if name.is_none() && is_human_label(s) {
                    name = Some(s.clone());
                } else if category.is_none() && is_human_label(s) && Some(s) != name.as_ref() {
                    category = Some(s.clone());
                }
            }
            Value::Object(sub) => {
                for sval in sub.values() {
                    if let Value::String(s) = sval
                        && name.is_none()
                        && is_human_label(s)
                    {
                        name = Some(s.clone());
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    Some(Feature {
        uuid: uuid?,
        name: name.unwrap_or_default(),
        category,
    })
}

fn extract_script(v: &Value) -> Option<String> {
    let Value::Object(map) = v else { return None };
    map.values().find_map(|val| match val {
        Value::String(s) if s.contains("[ENABLE]") => Some(s.clone()),
        _ => None,
    })
}

fn array_has_script_dict(v: &Value) -> bool {
    let Value::Array(items) = v else { return false };
    items.iter().any(|item| {
        if let Value::Object(map) = item {
            map.values().any(|val| match val {
                Value::String(s) => s.contains("[ENABLE]"),
                _ => false,
            })
        } else {
            false
        }
    })
}

fn find_title_id(root: &Value, appid: Option<u64>) -> Option<u64> {
    let mut found: Option<u64> = None;
    walk_numbers(root, &mut |n| {
        if Some(n) == appid {
            return;
        }
        if (1..=STEAM_APPID_MAX).contains(&n) && n >= 100 && found.is_none() {
            found = Some(n);
        }
    });
    found
}

fn walk_numbers<F: FnMut(u64)>(v: &Value, f: &mut F) {
    match v {
        Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                f(u);
            }
        }
        Value::Array(items) => items.iter().for_each(|i| walk_numbers(i, f)),
        Value::Object(map) => map.values().for_each(|v| walk_numbers(v, f)),
        _ => {}
    }
}

fn looks_like_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split(|c: char| !c.is_ascii_digit() && c != '.').collect();
    parts.iter().any(|p| {
        let bits: Vec<&str> = p.split('.').collect();
        bits.len() >= 3
            && bits
                .iter()
                .all(|b| !b.is_empty() && b.chars().all(|c| c.is_ascii_digit()))
    })
}

fn is_platform_marker(s: &str) -> bool {
    matches!(
        s.to_uppercase().as_str(),
        "STEAM" | "EPIC" | "GOG" | "UPLAY" | "ORIGIN" | "XBOX" | "EA"
    )
}

fn is_human_label(s: &str) -> bool {
    // Heuristic: a printable English-ish label is at least 2 chars, contains a
    // letter, isn't all hex (filters out byte patterns), isn't a UUID, isn't
    // a version, isn't a known platform marker.
    if s.len() < 2 || looks_like_uuid(s) || looks_like_version(s) || is_platform_marker(s) {
        return false;
    }
    if !s.chars().any(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    if s.chars()
        .all(|c| c.is_ascii_hexdigit() || c.is_ascii_whitespace())
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_like_uuid_recognises_canonical_form() {
        assert!(looks_like_uuid("12710713-cf53-47f2-8a7a-c3139fda2677"));
        assert!(!looks_like_uuid("not-a-uuid"));
        assert!(!looks_like_uuid("12710713cf5347f28a7ac3139fda2677"));
    }

    #[test]
    fn looks_like_version_accepts_common_patterns() {
        assert!(looks_like_version("1.0.0.0"));
        assert!(looks_like_version("1.1.1"));
        assert!(looks_like_version("1.1.1 06-29-2025"));
        assert!(!looks_like_version("just text"));
        assert!(!looks_like_version("1.0"));
    }

    #[test]
    fn is_platform_marker_known_brands() {
        assert!(is_platform_marker("STEAM"));
        assert!(is_platform_marker("Epic"));
        assert!(!is_platform_marker("STEAMOS"));
    }

    #[test]
    fn is_human_label_filters_correctly() {
        assert!(is_human_label("God Mode"));
        assert!(is_human_label("Player"));
        assert!(!is_human_label("STEAM"));
        assert!(!is_human_label("1.0.0.0"));
        assert!(!is_human_label("12710713-cf53-47f2-8a7a-c3139fda2677"));
        assert!(!is_human_label("AA BB CC"));
        assert!(!is_human_label(""));
    }

    #[test]
    fn load_aurora_em_trainer_subset_fixture() {
        let json = include_str!("../tests/fixtures/aurora_em_trainer_subset.json");
        let trainer = load_trainer(json).expect("subset fixture must parse");

        assert_eq!(trainer.exe, "EnderMagnoliaSteam-Win64-Shipping.exe");
        assert_eq!(trainer.appid, Some(2725260));
        assert_eq!(trainer.title_id, Some(79142));
        assert_eq!(trainer.version, "1.1.1 06-29-2025");
        assert!(
            trainer.title.to_lowercase().contains("ender magnolia"),
            "unexpected title: {:?}",
            trainer.title
        );

        // Subset has 2 features carved out of the original 11.
        assert_eq!(trainer.features.len(), 2);
        let names: Vec<&str> = trainer.features.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"God Mode"), "names = {names:?}");
        assert!(names.contains(&"Unlimited Health"), "names = {names:?}");
        for f in &trainer.features {
            assert!(looks_like_uuid(&f.uuid), "bad uuid: {:?}", f.uuid);
        }

        // Subset has 3 scripts carved out of the original 1078.
        assert_eq!(trainer.scripts.len(), 3);
        for s in &trainer.scripts {
            assert!(s.contains("[ENABLE]"), "script missing [ENABLE]: {s:.80}");
            assert!(s.contains("aobscanmodule"), "script missing aobscanmodule");
        }
    }

    #[test]
    fn returns_error_when_payload_has_no_trainer_object() {
        let err = load_trainer("{\"unrelated\": [1, 2, 3]}").unwrap_err();
        assert!(matches!(err, AuroraError::NoTrainerObject));
    }

    #[test]
    fn returns_error_on_invalid_json() {
        let err = load_trainer("{not json at all").unwrap_err();
        assert!(matches!(err, AuroraError::Json(_)));
    }
}
