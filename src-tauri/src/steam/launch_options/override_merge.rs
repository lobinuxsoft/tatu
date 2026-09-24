//! Merge `winhttp=n,b` into a Steam launch-options string without clobbering
//! whatever the user already had there.
//!
//! Steam launch options are a free-form shell-ish prefix around `%command%`.
//! The piece we care about is the `WINEDLLOVERRIDES` environment assignment,
//! whose value is a `;`-separated list of `dll=mode` entries (e.g.
//! `WINEDLLOVERRIDES="dxgi=n,b;dinput8=n,b"`). We need our `winhttp=n,b` in
//! that list and a `%command%` somewhere so the game still launches.

/// The DLL override the Mono collector needs: load our native `winhttp.dll`
/// before the builtin (`n` = native, `b` = builtin).
const WINHTTP_ENTRY: &str = "winhttp=n,b";
const OVERRIDE_KEY: &str = "WINEDLLOVERRIDES=";

/// Return the launch-options string that has `winhttp=n,b` applied, or `None`
/// if `existing` already covers it (idempotent no-op).
pub fn merge_winhttp(existing: &str) -> Option<String> {
    let trimmed = existing.trim();

    // Already present anywhere → nothing to do.
    if has_winhttp_override(trimmed) {
        return None;
    }

    if trimmed.is_empty() {
        return Some(format!("{OVERRIDE_KEY}{WINHTTP_ENTRY} %command%"));
    }

    match find_override_span(trimmed) {
        // There is a WINEDLLOVERRIDES already: splice our entry into its list.
        // The entry has no spaces, so the existing quoting style stays valid
        // whether or not the value was quoted — splice in place.
        Some((val_start, val_end, _quoted)) => {
            let list = &trimmed[val_start..val_end];
            let merged_list = if list.is_empty() {
                WINHTTP_ENTRY.to_string()
            } else {
                format!("{list};{WINHTTP_ENTRY}")
            };
            let mut out = String::with_capacity(trimmed.len() + WINHTTP_ENTRY.len() + 1);
            out.push_str(&trimmed[..val_start]);
            out.push_str(&merged_list);
            out.push_str(&trimmed[val_end..]);
            Some(out)
        }
        // No WINEDLLOVERRIDES: prepend one, ensuring %command% is present.
        None => {
            let base = if trimmed.contains("%command%") {
                trimmed.to_string()
            } else {
                format!("{trimmed} %command%")
            };
            Some(format!("{OVERRIDE_KEY}{WINHTTP_ENTRY} {base}"))
        }
    }
}

/// True if a `WINEDLLOVERRIDES` value already lists `winhttp` (any mode).
fn has_winhttp_override(s: &str) -> bool {
    match find_override_span(s) {
        Some((start, end, _)) => s[start..end]
            .split(';')
            .any(|entry| entry.split('=').next() == Some("winhttp")),
        None => false,
    }
}

/// Locate the value of the first `WINEDLLOVERRIDES=` assignment. Returns the
/// byte range of the *value* (excluding surrounding quotes) and whether it was
/// quoted. The value ends at the closing quote (if quoted) or the next space.
fn find_override_span(s: &str) -> Option<(usize, usize, bool)> {
    let key_at = s.find(OVERRIDE_KEY)?;
    let after = key_at + OVERRIDE_KEY.len();
    let rest = &s[after..];
    if let Some(stripped) = rest.strip_prefix('"') {
        // Quoted: value runs to the next unescaped quote.
        let val_start = after + 1;
        let close_rel = stripped.find('"')?;
        Some((val_start, val_start + close_rel, true))
    } else {
        // Unquoted: value runs to the next whitespace (or end).
        let end_rel = rest.find(char::is_whitespace).unwrap_or(rest.len());
        Some((after, after + end_rel, false))
    }
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
    }

    #[test]
    fn prepends_when_no_override_present() {
        assert_eq!(
            merge_winhttp("PROTON_FSR4_UPGRADE=1 %command%").unwrap(),
            "WINEDLLOVERRIDES=winhttp=n,b PROTON_FSR4_UPGRADE=1 %command%"
        );
    }

    #[test]
    fn adds_command_token_when_absent() {
        assert_eq!(
            merge_winhttp("gamemoderun").unwrap(),
            "WINEDLLOVERRIDES=winhttp=n,b gamemoderun %command%"
        );
    }

    #[test]
    fn merges_into_unquoted_override() {
        assert_eq!(
            merge_winhttp("WINEDLLOVERRIDES=dxgi=n,b SteamDeck=0 %command%").unwrap(),
            "WINEDLLOVERRIDES=dxgi=n,b;winhttp=n,b SteamDeck=0 %command%"
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
    fn merges_into_empty_quoted_override() {
        assert_eq!(
            merge_winhttp("WINEDLLOVERRIDES=\"\" %command%").unwrap(),
            "WINEDLLOVERRIDES=\"winhttp=n,b\" %command%"
        );
    }
}
