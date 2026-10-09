//! What Tatu does to OptiScaler.ini: set one key, read one, and read which
//! codes a key accepts (upscaler, frame generation). The codes changed between OptiScaler
//! versions (0.9.4 calls FSR 3.1/4 `fsr31` on DX12, later builds `ffx`), so
//! they're read from the comment block above the key in the very ini being
//! installed — never a list hard-coded here.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IniOption {
    pub code: String,
    /// The ini's own description, e.g. `fsr31 (also for FSR4)`.
    pub label: String,
}

/// `text` with `key=` inside `[section]` set to `value` — every occurrence:
/// OptiScaler's ini library keeps the last of a repeated key, so they all
/// have to agree. Everything else — comments, other keys, line endings — is
/// left exactly as it was. `None` if the section or key isn't there.
pub fn set_key(text: &str, section: &str, key: &str, value: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len() + value.len());
    let mut in_section = false;
    let mut done = false;
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\r', '\n']);
        let trimmed = body.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed
                .trim_start_matches('[')
                .trim_end_matches(']')
                .eq_ignore_ascii_case(section);
        } else if in_section && key_of(trimmed).is_some_and(|k| k.eq_ignore_ascii_case(key)) {
            let ending = &line[body.len()..];
            let indent = &body[..body.len() - body.trim_start().len()];
            out.push_str(&format!("{indent}{key}={value}{ending}"));
            done = true;
            continue;
        }
        out.push_str(line);
    }
    done.then_some(out)
}

/// The codes documented above `key=`: the comment line in the block right
/// above it that lists several comma-separated options.
pub fn codes_for_key(text: &str, key: &str) -> Vec<IniOption> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(at) = lines
        .iter()
        .position(|l| key_of(l.trim()).is_some_and(|k| k.eq_ignore_ascii_case(key)))
    else {
        return Vec::new();
    };
    let comments = lines[..at]
        .iter()
        .rev()
        .take_while(|l| l.trim_start().starts_with(';'))
        .map(|l| l.trim_start().trim_start_matches(';').trim());
    let Some(list) = comments
        .max_by_key(|c| top_level_commas(c))
        .filter(|c| top_level_commas(c) > 0)
    else {
        return Vec::new();
    };
    let list = match list.to_ascii_lowercase().find("- default") {
        Some(cut) => &list[..cut],
        None => list,
    };
    split_top_level(list)
        .into_iter()
        .filter_map(|item| {
            let item = item.trim();
            let code: String = item
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            (!code.is_empty() && code.chars().all(|c| !c.is_ascii_uppercase())).then(|| IniOption {
                code,
                label: item.to_string(),
            })
        })
        .collect()
}

/// The numbered choices the comment block right above `key=` in `[section]`
/// documents, like `; 0 = FSR 4.0.2 | 1 = FSR 3.1.5 | 2 = FSR 2.3.4` (a list
/// can run over several lines).
pub fn numbered_options(text: &str, section: &str, key: &str) -> Vec<IniOption> {
    let mut in_section = false;
    let mut block: Vec<&str> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed
                .trim_start_matches('[')
                .trim_end_matches(']')
                .eq_ignore_ascii_case(section);
            block.clear();
        } else if trimmed.starts_with(';') {
            block.push(trimmed.trim_start_matches(';'));
        } else if in_section && key_of(trimmed).is_some_and(|k| k.eq_ignore_ascii_case(key)) {
            return block
                .iter()
                .flat_map(|comment| comment.split('|'))
                .filter_map(|item| {
                    let (code, label) = item.split_once('=')?;
                    let code = code.trim();
                    (!code.is_empty() && code.bytes().all(|b| b.is_ascii_digit())).then(|| {
                        IniOption {
                            code: code.to_string(),
                            label: label.trim().to_string(),
                        }
                    })
                })
                .collect();
        } else {
            block.clear();
        }
    }
    Vec::new()
}

/// The value of `key=` inside `[section]`, trimmed — the last one if it's
/// repeated, like OptiScaler's ini library (SimpleIni) reads it.
pub fn get_key(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    let mut found = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed
                .trim_start_matches('[')
                .trim_end_matches(']')
                .eq_ignore_ascii_case(section);
        } else if in_section && key_of(trimmed).is_some_and(|k| k.eq_ignore_ascii_case(key)) {
            found = trimmed.split_once('=').map(|(_, v)| v.trim().to_string());
        }
    }
    found
}

fn key_of(line: &str) -> Option<&str> {
    if line.starts_with(';') || line.starts_with('#') {
        return None;
    }
    line.split_once('=').map(|(k, _)| k.trim())
}

fn top_level_commas(s: &str) -> usize {
    split_top_level(s).len() - 1
}

/// Split on commas that aren't inside parentheses:
/// `xess (native DX11, Arc only), fsr22` → two items.
fn split_top_level(s: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth <= 0 => {
                items.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push(&s[start..]);
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    /// OptiScaler 0.9.4's release ini (CRLF), abridged.
    const RELEASE: &str = "[Upscalers]\r\n; -------\r\n; Select Upscaler for Dx11 games\r\n; fsr22 (native DX11), fsr31 (native DX11), xess (native DX11, Arc only), xess_12 (dx11on12), fsr21_12 (dx11on12), fsr22_12 (dx11on12), fsr31_12 (dx11on12, FSR4), dlss - Default (auto) is fsr22\r\nDx11Upscaler=auto\r\n\r\n; Select Upscaler for Dx12 games\r\n; xess, fsr21, fsr22, fsr31 (also for FSR4), dlss - Default (auto) is xess\r\nDx12Upscaler=auto\r\n\r\n[Spoofing]\r\n; Dxgi spoofing\r\nDxgi=auto\r\n";

    /// OptiScaler master's layout: codes and default on separate lines.
    const MASTER: &str = "[Upscalers]\n; Select Upscaler for Dx12 games\n; xess, fsr21, fsr22, ffx (FSR 2.3; 3.1; 4.x), sgsr2, dlss\n; Default (auto) is DLSS when capable gpu, FSR4 when capable gpu, XeSS otherwise\nDx12Upscaler=auto\n";

    fn codes(text: &str, key: &str) -> Vec<String> {
        codes_for_key(text, key)
            .into_iter()
            .map(|o| o.code)
            .collect()
    }

    #[test]
    fn reads_the_codes_each_version_documents() {
        assert_eq!(
            codes(RELEASE, "Dx11Upscaler"),
            [
                "fsr22", "fsr31", "xess", "xess_12", "fsr21_12", "fsr22_12", "fsr31_12", "dlss"
            ]
        );
        assert_eq!(
            codes(RELEASE, "Dx12Upscaler"),
            ["xess", "fsr21", "fsr22", "fsr31", "dlss"]
        );
        assert_eq!(
            codes(MASTER, "Dx12Upscaler"),
            ["xess", "fsr21", "fsr22", "ffx", "sgsr2", "dlss"]
        );
        assert_eq!(
            codes_for_key(RELEASE, "Dx12Upscaler")[3].label,
            "fsr31 (also for FSR4)"
        );
        assert!(codes(RELEASE, "VulkanUpscaler").is_empty());
    }

    #[test]
    fn sets_one_key_and_touches_nothing_else() {
        let edited = set_key(RELEASE, "Upscalers", "Dx12Upscaler", "fsr31").unwrap();
        assert_eq!(
            edited,
            RELEASE.replace("Dx12Upscaler=auto", "Dx12Upscaler=fsr31")
        );
        let edited = set_key(&edited, "Spoofing", "Dxgi", "false").unwrap();
        assert!(edited.contains("Dxgi=false\r\n"));
        assert!(edited.contains("Dx11Upscaler=auto\r\n"));
        // A key outside its section isn't matched.
        assert_eq!(set_key(RELEASE, "Spoofing", "Dx12Upscaler", "x"), None);
        assert_eq!(set_key(MASTER, "Nope", "Dx12Upscaler", "x"), None);
    }

    #[test]
    fn a_repeated_key_is_set_everywhere_and_read_like_simpleini() {
        // SimpleIni (OptiScaler's reader) keeps the last value of a repeated
        // key, also across a section written twice.
        let text =
            "[CAS]\r\nEnabled=false\r\n[Other]\r\nEnabled=keep\r\n[cas]\r\n  Enabled = false\r\n";
        assert_eq!(get_key(text, "CAS", "Enabled").as_deref(), Some("false"));
        let edited = set_key(text, "CAS", "Enabled", "true").unwrap();
        assert_eq!(
            edited,
            "[CAS]\r\nEnabled=true\r\n[Other]\r\nEnabled=keep\r\n[cas]\r\n  Enabled=true\r\n"
        );
        assert_eq!(get_key(&edited, "cas", "ENABLED").as_deref(), Some("true"));
        assert_eq!(
            get_key(&edited, "Other", "Enabled").as_deref(),
            Some("keep")
        );
        assert_eq!(set_key(text, "CAS", "Missing", "1"), None);
    }

    #[test]
    fn numbered_choices_need_the_comment_right_above_the_key() {
        let text = "[FSR]\n; 0 = FSR 4.0.2 | 1 = FSR 3.1.5\n; Default (auto) is 0 - FSR4 for RDNA4\nUpscalerIndex=auto\n\n\
            ; 0 = FSR 4.0.0 | 1 = FSR 3.1.6\n\nFGIndex=auto\n[Other]\n; 0 = X\nUpscalerIndex=auto\n";
        let codes = |section, key| {
            numbered_options(text, section, key)
                .into_iter()
                .map(|o| format!("{}={}", o.code, o.label))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            codes("FSR", "UpscalerIndex"),
            ["0=FSR 4.0.2", "1=FSR 3.1.5"]
        );
        // A blank line between the list and the key: it isn't this key's.
        assert!(codes("FSR", "FGIndex").is_empty());
        assert_eq!(codes("other", "UpscalerIndex"), ["0=X"]);
    }
}
