//! `CompatToolMapping` patcher for Steam's root `config.vdf`.
//!
//! Steam stores the per-game compat tool selection under
//! `InstallConfigStore.Software.Valve.Steam.CompatToolMapping`. The
//! tracker upserts an entry there so when Steam reloads the file
//! (on startup) the game's "Force the use of a specific Steam Play
//! compatibility tool" picker shows "Tatu Launcher" already
//! checked.
//!
//! We do not use a full VDF library — the format is permissive (tab
//! indentation, embedded whitespace, no escaping rules we need to
//! preserve) and the failure mode for a malformed write is
//! recoverable (Steam keeps a `.bak`). A minimal text-level
//! scanner is enough and avoids dragging in a serializer for a
//! single block.
//!
//! Steam rewrites `config.vdf` on exit, so the install routine must
//! refuse to run while the client is alive — otherwise the patch
//! gets clobbered.

use std::fs;

use crate::tatu_launcher::TatuLauncherError;
use crate::tatu_launcher::paths::{COMPAT_TOOL_NAME, config_vdf_path};

const SECTION_HEADER: &str = "\"CompatToolMapping\"";
const ENTRY_PRIORITY: &str = "250";

/// Refuse to patch `config.vdf` if `steam` is currently running.
/// Linux: walks `/proc/*/comm` looking for the exact name `steam`.
/// Empty match → safe to edit. On unsupported platforms returns
/// `false` (i.e. assumes safe — the tracker only really runs on
/// Linux for now).
fn steam_running() -> bool {
    #[cfg(target_os = "linux")]
    {
        let Ok(rd) = fs::read_dir("/proc") else {
            return false;
        };
        for entry in rd.flatten() {
            let comm_path = entry.path().join("comm");
            let Ok(comm) = fs::read_to_string(&comm_path) else {
                continue;
            };
            if comm.trim() == "steam" {
                return true;
            }
        }
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Read the configured compat tool name for `app_id`. `None` if the
/// section or entry is missing.
pub fn get_compat_tool_for_app(app_id: &str) -> Result<Option<String>, TatuLauncherError> {
    let path = config_vdf_path()?;
    let Ok(content) = fs::read_to_string(&path) else {
        return Ok(None);
    };
    Ok(parse_tool_for_app(&content, app_id))
}

/// Set the compat tool name for `app_id` to `COMPAT_TOOL_NAME`. If
/// the entry already exists, the `name` field is overwritten; if
/// not, a new block is inserted at the end of the
/// `CompatToolMapping` section. `priority` is set to 250 to match
/// every existing entry we've seen Steam emit.
pub fn set_compat_tool_for_app(app_id: &str) -> Result<(), TatuLauncherError> {
    if steam_running() {
        return Err(TatuLauncherError::SteamRunning);
    }
    let path = config_vdf_path()?;
    let content = fs::read_to_string(&path)?;
    let patched = upsert_entry(&content, app_id, COMPAT_TOOL_NAME, ENTRY_PRIORITY)?;
    write_atomic(&path, &patched)?;
    Ok(())
}

/// Locate `"<app_id>"` block inside `CompatToolMapping` and return
/// its `name` value, if any.
fn parse_tool_for_app(content: &str, app_id: &str) -> Option<String> {
    let section = locate_section(content)?;
    let block = locate_app_block(&content[section.start..section.end], app_id)?;
    let section_text = &content[section.start..section.end];
    let block_text = &section_text[block.start..block.end];
    extract_value(block_text, "name")
}

#[derive(Debug, Clone, Copy)]
struct Range {
    start: usize,
    end: usize,
}

fn locate_section(content: &str) -> Option<Range> {
    let header = content.find(SECTION_HEADER)?;
    let after_header = &content[header + SECTION_HEADER.len()..];
    let open = after_header.find('{')?;
    let body_start = header + SECTION_HEADER.len() + open + 1;
    let close = match_braces(content, body_start)?;
    Some(Range {
        start: body_start,
        end: close,
    })
}

/// Forward-scan from `body_start` (just past the `{` of a VDF block)
/// to the matching `}`. Returns the byte index of that closing brace.
fn match_braces(content: &str, body_start: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut depth: i32 = 1;
    let mut i = body_start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            // Skip past quoted strings so `{`/`}` inside a value
            // does not unbalance the count. VDF values can carry
            // arbitrary characters between quotes (including braces
            // for some tool configs).
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 1;
                    }
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Locate `"<app_id>" { ... }` inside an already-narrowed
/// `CompatToolMapping` body slice. Returns the range *relative* to
/// the slice start.
fn locate_app_block(section_body: &str, app_id: &str) -> Option<Range> {
    let needle = format!("\"{app_id}\"");
    let key = section_body.find(&needle)?;
    let after_key = &section_body[key + needle.len()..];
    let open_rel = after_key.find('{')?;
    let block_open_abs = key + needle.len() + open_rel;
    let block_body_start = block_open_abs + 1;
    let close_abs = match_braces(section_body, block_body_start)?;
    Some(Range {
        start: key,
        end: close_abs + 1,
    })
}

/// Extract a `"<key>" "<value>"` pair from a VDF block body. Tabs
/// or spaces between key and value are tolerated.
fn extract_value(block: &str, key: &str) -> Option<String> {
    let key_q = format!("\"{key}\"");
    let key_pos = block.find(&key_q)?;
    let after_key = &block[key_pos + key_q.len()..];
    let value_start = after_key.find('"')?;
    let value_body = &after_key[value_start + 1..];
    let value_end = value_body.find('"')?;
    Some(value_body[..value_end].to_string())
}

/// Insert / replace the appid block. Returns the rewritten file
/// content; errors only if the section header is absent.
fn upsert_entry(
    content: &str,
    app_id: &str,
    name: &str,
    priority: &str,
) -> Result<String, TatuLauncherError> {
    let section = locate_section(content).ok_or_else(|| {
        TatuLauncherError::ConfigVdfShape(
            "CompatToolMapping section not found — Steam may have never written one yet"
                .to_string(),
        )
    })?;
    let section_body = &content[section.start..section.end];

    let new_block = format_entry(section_body, app_id, name, priority);

    if let Some(block) = locate_app_block(section_body, app_id) {
        let mut out = String::with_capacity(content.len() + 64);
        out.push_str(&content[..section.start]);
        out.push_str(&section_body[..block.start]);
        out.push_str(&new_block);
        out.push_str(&section_body[block.end..]);
        out.push_str(&content[section.end..]);
        Ok(out)
    } else {
        // Insert before the closing `}` of the section. Re-detect
        // the section so we can find the line break just before
        // its closing brace (we want the new block to sit nicely
        // indented above the `}`).
        let insertion = section.end; // points at the `}` itself
        let mut out = String::with_capacity(content.len() + 96);
        out.push_str(&content[..insertion]);
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&new_block);
        out.push_str(&content[insertion..]);
        Ok(out)
    }
}

/// Build the textual block matching the indentation style the
/// surrounding section already uses. We infer the indentation from
/// the first existing entry; if the section is empty we fall back
/// to a tab-style indent that matches every config.vdf we've seen
/// Steam emit.
fn format_entry(section_body: &str, app_id: &str, name: &str, priority: &str) -> String {
    let (key_indent, field_indent) = detect_indent(section_body);
    format!(
        "{key_indent}\"{app_id}\"\n{key_indent}{{\n\
         {field_indent}\"name\"\t\t\"{name}\"\n\
         {field_indent}\"config\"\t\t\"\"\n\
         {field_indent}\"priority\"\t\t\"{priority}\"\n\
         {key_indent}}}\n",
    )
}

/// Sniff indentation from a sibling entry. Returns `(key_indent,
/// field_indent)` — falls back to the canonical 4-tab / 5-tab
/// scheme Steam writes for top-level CompatToolMapping siblings.
fn detect_indent(section_body: &str) -> (String, String) {
    for line in section_body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 3 {
            let key_indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            // Field rows live one indentation step deeper. Tabs are
            // the canonical step.
            let field_indent = format!("{key_indent}\t");
            return (key_indent, field_indent);
        }
    }
    ("\t\t\t\t\t".to_string(), "\t\t\t\t\t\t".to_string())
}

fn write_atomic(path: &std::path::Path, data: &str) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tatu.tmp");
    let tmp = std::path::PathBuf::from(tmp);
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
"InstallConfigStore"
{
	"Software"
	{
		"Valve"
		{
			"Steam"
			{
				"CompatToolMapping"
				{
					"2725260"
					{
						"name"		"GE-Proton10-26"
						"config"		""
						"priority"		"250"
					}
					"2420110"
					{
						"name"		"Proton-aurora"
						"config"		""
						"priority"		"250"
					}
				}
			}
		}
	}
}
"#;

    const EMPTY_SECTION: &str = r#"
"InstallConfigStore"
{
	"Software"
	{
		"Valve"
		{
			"Steam"
			{
				"CompatToolMapping"
				{
				}
			}
		}
	}
}
"#;

    #[test]
    fn parses_existing_tool_assignment() {
        assert_eq!(
            parse_tool_for_app(SAMPLE, "2725260"),
            Some("GE-Proton10-26".to_string())
        );
        assert_eq!(
            parse_tool_for_app(SAMPLE, "2420110"),
            Some("Proton-aurora".to_string())
        );
    }

    #[test]
    fn parses_none_for_missing_app() {
        assert_eq!(parse_tool_for_app(SAMPLE, "999999"), None);
    }

    #[test]
    fn upsert_replaces_existing_block() {
        let patched = upsert_entry(SAMPLE, "2725260", "tatu-launcher", "250").unwrap();
        assert_eq!(
            parse_tool_for_app(&patched, "2725260"),
            Some("tatu-launcher".to_string())
        );
        // Sibling untouched.
        assert_eq!(
            parse_tool_for_app(&patched, "2420110"),
            Some("Proton-aurora".to_string())
        );
    }

    #[test]
    fn upsert_inserts_new_block_when_absent() {
        let patched = upsert_entry(SAMPLE, "1234567", "tatu-launcher", "250").unwrap();
        assert_eq!(
            parse_tool_for_app(&patched, "1234567"),
            Some("tatu-launcher".to_string())
        );
        // Existing siblings stay.
        assert_eq!(
            parse_tool_for_app(&patched, "2725260"),
            Some("GE-Proton10-26".to_string())
        );
    }

    #[test]
    fn upsert_into_empty_section_works() {
        let patched = upsert_entry(EMPTY_SECTION, "2725260", "tatu-launcher", "250").unwrap();
        assert_eq!(
            parse_tool_for_app(&patched, "2725260"),
            Some("tatu-launcher".to_string())
        );
    }

    #[test]
    fn upsert_errors_when_section_missing() {
        let err = upsert_entry("\"InstallConfigStore\" {}", "1", "x", "250").unwrap_err();
        assert!(matches!(err, TatuLauncherError::ConfigVdfShape(_)));
    }

    #[test]
    fn match_braces_skips_quoted_content() {
        // Braces inside a string value must not unbalance the count.
        let s = r#"{ "name" "{tricky}" }"#;
        // body_start = 1 (just past the leading `{`)
        let close = match_braces(s, 1).unwrap();
        assert_eq!(&s[close..close + 1], "}");
    }

    #[test]
    fn detect_indent_recovers_tabs_from_sibling() {
        let body = "\n\t\t\t\t\t\"2725260\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"name\" \"x\"\n\t\t\t\t\t}\n";
        let (key, field) = detect_indent(body);
        assert!(key.starts_with('\t'));
        assert!(field.starts_with(&key[..]));
        assert!(field.len() > key.len(), "field must be deeper than key");
    }
}
