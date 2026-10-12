//! Edit a Steam launch-options string without clobbering whatever the user
//! already had there: merge `winhttp=n,b` (the Mono collector), add or remove
//! any DLL's override (OptiScaler under a proxy name Proton doesn't already
//! load native), set or remove an environment variable (GE-Proton's
//! `PROTON_DLSS_UPGRADE`).
//!
//! Steam launch options are a free-form shell prefix around `%command%`, and
//! Steam runs them through a shell: a `;` outside quotes ends the command
//! there, and a string without `%command%` is passed to the game as its
//! arguments. So a `WINEDLLOVERRIDES` list that grows past one entry gets
//! quoted (`WINEDLLOVERRIDES="dxgi=n,b;dinput8=n,b"`), and whatever we
//! prepend goes before a `%command%` placed ahead of arguments like `-dx12`.

use std::ops::Range;

/// The DLL override the Mono collector needs: load our native `winhttp.dll`
/// before the builtin (`n` = native, `b` = builtin).
const WINHTTP_ENTRY: &str = "winhttp=n,b";
const OVERRIDE_KEY: &str = "WINEDLLOVERRIDES=";
const COMMAND: &str = "%command%";

/// Return the launch-options string that has `winhttp=n,b` applied, or `None`
/// if `existing` already lists winhttp (idempotent no-op).
pub fn merge_winhttp(existing: &str) -> Option<String> {
    let trimmed = existing.trim();
    if override_mode(trimmed, "winhttp").is_some() {
        return None;
    }
    Some(insert_entry(trimmed, WINHTTP_ENTRY))
}

/// What asking for `<dll>=n,b` found.
#[derive(Debug, PartialEq, Eq)]
pub enum AddOverride {
    /// The launch options with the entry added.
    Added(String),
    /// Some entry already loads it native first: nothing to add.
    AlreadyNative,
    /// The user set it to another mode (e.g. `dxgi=b`) — left alone; the DLL
    /// next to the game won't load until they change it.
    Conflict(String),
}

/// Add `<dll>=n,b` (`dll` without `.dll`) unless the list already mentions
/// that DLL, alone or grouped (`dxgi,d3d11=n,b`), in any case.
pub fn add_override(existing: &str, dll: &str) -> AddOverride {
    let trimmed = existing.trim();
    match override_mode(trimmed, dll) {
        Some(mode) if mode.trim().to_ascii_lowercase().starts_with('n') => {
            AddOverride::AlreadyNative
        }
        Some(mode) => AddOverride::Conflict(mode),
        None => AddOverride::Added(insert_entry(trimmed, &format!("{dll}=n,b"))),
    }
}

/// `existing` without `dll` in its `WINEDLLOVERRIDES` list — only that name,
/// grouped entries keep their other names. A list left empty takes the whole
/// assignment with it. `None` when `dll` isn't listed.
pub fn remove_override(existing: &str, dll: &str) -> Option<String> {
    let trimmed = existing.trim();
    let span = find_override_span(trimmed)?;
    let mut changed = false;
    let kept: Vec<String> = trimmed[span.value.clone()]
        .split(';')
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| {
            let Some((names, mode)) = entry.split_once('=') else {
                return Some(entry.to_string());
            };
            let others: Vec<&str> = names
                .split(',')
                .filter(|n| !n.eq_ignore_ascii_case(dll))
                .collect();
            if others.len() == names.split(',').count() {
                return Some(entry.to_string());
            }
            changed = true;
            (!others.is_empty()).then(|| format!("{}={mode}", others.join(",")))
        })
        .collect();
    if !changed {
        return None;
    }
    if !kept.is_empty() {
        let list = kept.join(";");
        let list = if span.quote.is_none() && kept.len() > 1 {
            format!("\"{list}\"")
        } else {
            list
        };
        return Some(splice(trimmed, span.value, &list));
    }
    let before = trimmed[..span.key].trim_end();
    let after = trimmed[span.end..].trim_start();
    Some(match (before.is_empty(), after.is_empty()) {
        (true, _) => after.to_string(),
        (false, true) => before.to_string(),
        (false, false) => format!("{before} {after}"),
    })
}

/// The value of `key=` among the environment assignments before `%command%`.
pub fn get_env(existing: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    env_tokens(existing).into_iter().find_map(|(start, end)| {
        existing[start..end]
            .strip_prefix(&prefix)
            .map(|v| v.trim_matches(['"', '\'']).to_string())
    })
}

/// `existing` with `key=value` set (`Some`) or removed (`None`), before
/// `%command%`. `None` when nothing changes.
pub fn set_env(existing: &str, key: &str, value: Option<&str>) -> Option<String> {
    let trimmed = existing.trim();
    let prefix = format!("{key}=");
    let found = env_tokens(trimmed)
        .into_iter()
        .find(|&(start, end)| trimmed[start..end].starts_with(&prefix));
    match (found, value) {
        (Some((start, end)), Some(v)) => {
            let token = format!("{key}={v}");
            (trimmed[start..end] != token).then(|| splice(trimmed, start..end, &token))
        }
        (Some((start, end)), None) => {
            let before = trimmed[..start].trim_end();
            let after = trimmed[end..].trim_start();
            let rest = format!("{before} {after}").trim().to_string();
            // A bare `%command%` is what Steam runs anyway: give back the
            // empty options the user most likely started from.
            Some(if rest == COMMAND { String::new() } else { rest })
        }
        (None, Some(v)) => Some(prepend(trimmed, &format!("{key}={v}"))),
        (None, None) => None,
    }
}

/// The mode `dll` has in the `WINEDLLOVERRIDES` list, if it's listed.
pub fn override_mode(s: &str, dll: &str) -> Option<String> {
    let span = find_override_span(s)?;
    s[span.value].split(';').find_map(|entry| {
        let (names, mode) = entry.split_once('=')?;
        names
            .split(',')
            .any(|n| n.trim().eq_ignore_ascii_case(dll))
            .then(|| mode.to_string())
    })
}

/// `trimmed` with `entry` added to its `WINEDLLOVERRIDES` list, creating the
/// assignment — and `%command%` — when absent.
fn insert_entry(trimmed: &str, entry: &str) -> String {
    let Some(span) = find_override_span(trimmed) else {
        return prepend(trimmed, &format!("{OVERRIDE_KEY}{entry}"));
    };
    let list = &trimmed[span.value.clone()];
    let merged = if list.is_empty() {
        entry.to_string()
    } else if span.quote.is_some() {
        format!("{list};{entry}")
    } else {
        // Unquoted, the `;` would end the shell command right there.
        format!("\"{list};{entry}\"")
    };
    splice(trimmed, span.value, &merged)
}

/// `assignment` in front of the command. Without `%command%` the whole string
/// is the game's arguments, so those stay after it.
fn prepend(trimmed: &str, assignment: &str) -> String {
    if trimmed.is_empty() {
        format!("{assignment} {COMMAND}")
    } else if trimmed.contains(COMMAND) {
        format!("{assignment} {trimmed}")
    } else {
        format!("{assignment} {COMMAND} {trimmed}")
    }
}

fn splice(s: &str, range: Range<usize>, with: &str) -> String {
    format!("{}{with}{}", &s[..range.start], &s[range.end..])
}

/// A `WINEDLLOVERRIDES=` assignment: where its token starts, its value (inside
/// the quotes, if any) and where the token ends (after the closing quote).
struct Span {
    key: usize,
    value: Range<usize>,
    quote: Option<char>,
    end: usize,
}

/// The first `WINEDLLOVERRIDES=` that is a whole token (not the tail of
/// `PROTON_WINEDLLOVERRIDES=`) before `%command%` — after it, it's an
/// argument to the game.
fn find_override_span(s: &str) -> Option<Span> {
    let limit = s.find(COMMAND).unwrap_or(s.len());
    let mut from = 0;
    let key = loop {
        if from >= limit {
            return None;
        }
        let at = from + s[from..limit].find(OVERRIDE_KEY)?;
        if at == 0 || s[..at].ends_with(char::is_whitespace) {
            break at;
        }
        from = at + OVERRIDE_KEY.len();
    };
    let after = key + OVERRIDE_KEY.len();
    let rest = &s[after..];
    match rest.chars().next() {
        Some(quote @ ('"' | '\'')) => {
            let close = rest[1..].find(quote)?;
            let value = after + 1..after + 1 + close;
            Some(Span {
                key,
                end: value.end + 1,
                value,
                quote: Some(quote),
            })
        }
        _ => {
            let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
            Some(Span {
                key,
                value: after..after + len,
                quote: None,
                end: after + len,
            })
        }
    }
}

/// Byte ranges of the whitespace-separated tokens before `%command%` (the
/// environment prefix), quotes kept inside their token.
fn env_tokens(s: &str) -> Vec<(usize, usize)> {
    let end = s.find(COMMAND).unwrap_or(s.len());
    let mut tokens = Vec::new();
    let mut start = None;
    let mut quote: Option<char> = None;
    for (i, c) in s[..end].char_indices() {
        match c {
            '"' | '\'' if quote.is_none() => {
                quote = Some(c);
                start.get_or_insert(i);
            }
            c if Some(c) == quote => quote = None,
            c if c.is_whitespace() && quote.is_none() => {
                if let Some(st) = start.take() {
                    tokens.push((st, i));
                }
            }
            _ => {
                start.get_or_insert(i);
            }
        }
    }
    if let Some(st) = start {
        tokens.push((st, end));
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_gets_full_override() {
        assert_eq!(
            merge_winhttp("").unwrap(),
            "WINEDLLOVERRIDES=winhttp=n,b %command%"
        );
    }

    #[test]
    fn whitespace_only_treated_as_empty() {
        assert_eq!(
            merge_winhttp("   ").unwrap(),
            "WINEDLLOVERRIDES=winhttp=n,b %command%"
        );
    }

    #[test]
    fn already_present_is_noop() {
        assert_eq!(
            merge_winhttp("WINEDLLOVERRIDES=winhttp=n,b %command%"),
            None
        );
        assert_eq!(
            merge_winhttp("WINEDLLOVERRIDES=\"dxgi=n,b;winhttp=n,b\" %command%"),
            None
        );
        // Grouped, and in another case: still there.
        assert_eq!(
            merge_winhttp("WINEDLLOVERRIDES=\"dxgi,WINHTTP=n,b\" %command%"),
            None
        );
    }

    #[test]
    fn prepends_when_no_override_present() {
        assert_eq!(
            merge_winhttp("PROTON_FSR4_UPGRADE=1 %command%").unwrap(),
            "WINEDLLOVERRIDES=winhttp=n,b PROTON_FSR4_UPGRADE=1 %command%"
        );
    }

    /// Without `%command%`, Steam passes the options to the game as
    /// arguments: they stay arguments.
    #[test]
    fn options_without_command_stay_arguments() {
        assert_eq!(
            merge_winhttp("-dx12 -skipintro").unwrap(),
            "WINEDLLOVERRIDES=winhttp=n,b %command% -dx12 -skipintro"
        );
        assert_eq!(
            set_env("-dx12", "PROTON_DLSS_UPGRADE", Some("1")).unwrap(),
            "PROTON_DLSS_UPGRADE=1 %command% -dx12"
        );
    }

    /// A `;` outside quotes would end the shell command: the grown list is
    /// quoted.
    #[test]
    fn merges_into_unquoted_override() {
        assert_eq!(
            merge_winhttp("WINEDLLOVERRIDES=dxgi=n,b SteamDeck=0 %command%").unwrap(),
            "WINEDLLOVERRIDES=\"dxgi=n,b;winhttp=n,b\" SteamDeck=0 %command%"
        );
        assert_eq!(
            merge_winhttp("WINEDLLOVERRIDES='dinput8=n,b' %command%").unwrap(),
            "WINEDLLOVERRIDES='dinput8=n,b;winhttp=n,b' %command%"
        );
    }

    #[test]
    fn merges_into_quoted_override() {
        assert_eq!(
            merge_winhttp("WINEDLLOVERRIDES=\"dxgi=n,b;dinput8=n,b\" %command%").unwrap(),
            "WINEDLLOVERRIDES=\"dxgi=n,b;dinput8=n,b;winhttp=n,b\" %command%"
        );
    }

    #[test]
    fn only_a_whole_token_before_the_command_counts() {
        assert_eq!(
            merge_winhttp("PROTON_WINEDLLOVERRIDES=x %command%").unwrap(),
            "WINEDLLOVERRIDES=winhttp=n,b PROTON_WINEDLLOVERRIDES=x %command%"
        );
        assert_eq!(
            add_override("%command% WINEDLLOVERRIDES=winmm=n,b", "winmm"),
            AddOverride::Added(
                "WINEDLLOVERRIDES=winmm=n,b %command% WINEDLLOVERRIDES=winmm=n,b".into()
            )
        );
    }

    #[test]
    fn add_override_respects_what_is_already_there() {
        assert_eq!(
            add_override("", "winmm"),
            AddOverride::Added("WINEDLLOVERRIDES=winmm=n,b %command%".into())
        );
        assert_eq!(
            add_override("WINEDLLOVERRIDES=\"dxgi,winmm=n,b\" %command%", "winmm"),
            AddOverride::AlreadyNative
        );
        assert_eq!(
            add_override("WINEDLLOVERRIDES=WINMM=N,B %command%", "winmm"),
            AddOverride::AlreadyNative
        );
        assert_eq!(
            add_override("WINEDLLOVERRIDES=winmm=b %command%", "winmm"),
            AddOverride::Conflict("b".into())
        );
        assert_eq!(
            add_override("WINEDLLOVERRIDES=dxgi=n,b %command%", "winmm"),
            AddOverride::Added("WINEDLLOVERRIDES=\"dxgi=n,b;winmm=n,b\" %command%".into())
        );
    }

    #[test]
    fn remove_override_takes_only_that_dll() {
        assert_eq!(
            remove_override("WINEDLLOVERRIDES=\"dxgi=n,b;winmm=n,b\" %command%", "winmm").unwrap(),
            "WINEDLLOVERRIDES=\"dxgi=n,b\" %command%"
        );
        assert_eq!(
            remove_override("WINEDLLOVERRIDES=winmm,dinput8=n,b %command%", "winmm").unwrap(),
            "WINEDLLOVERRIDES=dinput8=n,b %command%"
        );
        assert_eq!(
            remove_override(
                "gamemoderun WINEDLLOVERRIDES=winmm=n,b %command% -dx12",
                "winmm"
            )
            .unwrap(),
            "gamemoderun %command% -dx12"
        );
        assert_eq!(
            remove_override("WINEDLLOVERRIDES=\"winmm=n,b\" %command%", "winmm").unwrap(),
            "%command%"
        );
        assert_eq!(
            remove_override("WINEDLLOVERRIDES='winmm=n,b' mangohud %command%", "winmm").unwrap(),
            "mangohud %command%"
        );
        assert_eq!(
            remove_override("WINEDLLOVERRIDES=dxgi=n,b %command%", "winmm"),
            None
        );
        assert_eq!(remove_override("%command%", "winmm"), None);
    }

    #[test]
    fn env_vars_are_set_and_removed_before_the_command() {
        assert_eq!(
            set_env("", "PROTON_DLSS_UPGRADE", Some("1")).unwrap(),
            "PROTON_DLSS_UPGRADE=1 %command%"
        );
        assert_eq!(
            set_env(
                "mangohud %command% -skipintro",
                "PROTON_DLSS_UPGRADE",
                Some("1")
            )
            .unwrap(),
            "PROTON_DLSS_UPGRADE=1 mangohud %command% -skipintro"
        );
        assert_eq!(
            set_env(
                "PROTON_DLSS_UPGRADE=1 %command%",
                "PROTON_DLSS_UPGRADE",
                Some("1")
            ),
            None
        );
        assert_eq!(
            set_env(
                "PROTON_DLSS_UPGRADE=1 %command%",
                "PROTON_DLSS_UPGRADE",
                Some("310.2")
            )
            .unwrap(),
            "PROTON_DLSS_UPGRADE=310.2 %command%"
        );
        assert_eq!(
            set_env(
                "A=1 PROTON_DLSS_UPGRADE=1 %command%",
                "PROTON_DLSS_UPGRADE",
                None
            )
            .unwrap(),
            "A=1 %command%"
        );
        assert_eq!(
            set_env(
                "PROTON_DLSS_UPGRADE=1 %command%",
                "PROTON_DLSS_UPGRADE",
                None
            )
            .unwrap(),
            ""
        );
        // After %command% it's an argument to the game, not ours to touch.
        assert_eq!(
            set_env(
                "%command% PROTON_DLSS_UPGRADE=1",
                "PROTON_DLSS_UPGRADE",
                None
            ),
            None
        );
        assert_eq!(
            get_env(
                "X=\"a b\" Y='c d' PROTON_USE_OPTISCALER=1 %command%",
                "PROTON_USE_OPTISCALER"
            )
            .as_deref(),
            Some("1")
        );
        assert_eq!(
            get_env("%command% PROTON_USE_OPTISCALER=1", "PROTON_USE_OPTISCALER"),
            None
        );
    }

    #[test]
    fn merges_into_empty_quoted_override() {
        assert_eq!(
            merge_winhttp("WINEDLLOVERRIDES=\"\" %command%").unwrap(),
            "WINEDLLOVERRIDES=\"winhttp=n,b\" %command%"
        );
    }
}
