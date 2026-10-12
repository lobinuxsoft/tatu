//! Device-side paths: what's allowed, how it's quoted, and the two shell
//! outputs (`df`, `du`) the transfer reads back.

use crate::AndroidError;

/// Shared storage GameNative's own "Añadir juego personalizado" picker can
/// reach: internal storage (`/sdcard`) plus SD cards / USB drives
/// (`/storage/<id>`). Anything else is refused, so a typo — or a hostile
/// path — can never point the transfer at the rest of the phone.
const ALLOWED_ROOTS: &[&str] = &["/sdcard", "/storage"];

/// Rejects a destination before it gets anywhere near the phone's shell:
/// empty, outside [`ALLOWED_ROOTS`], a `..` segment, or a control character
/// (never in a real folder name, and it would corrupt adb's line protocol).
/// Spaces, apostrophes and Unicode are fine — real game folders are named
/// `Baldur's Gate 3` — because [`quote`] is the injection defence, not this.
pub fn validate_remote_path(path: &str) -> Result<(), AndroidError> {
    let reject = |reason| {
        Err(AndroidError::InvalidRemotePath {
            path: path.to_string(),
            reason,
        })
    };
    let under_root = ALLOWED_ROOTS.iter().any(|root| {
        path.strip_prefix(root)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    });
    if !under_root {
        return reject("tiene que estar dentro de /sdcard o /storage");
    }
    if path.split('/').any(|segment| segment == "..") {
        return reject("no puede contener \"..\"");
    }
    if path.chars().any(char::is_control) {
        return reject("contiene caracteres de control");
    }
    Ok(())
}

/// Single-quotes `s` as one literal token for the device's `/system/bin/sh`
/// (`adb shell` joins its arguments and re-parses them there). An embedded
/// `'` becomes the usual close-quote / escaped-quote / reopen-quote `'\''`.
pub(crate) fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub(crate) fn join(root: &str, name: &str) -> String {
    format!("{}/{name}", root.trim_end_matches('/'))
}

/// Free bytes from `df -k <path>`: the `Available` column of the one data
/// row. Tokens are taken across every line after the header so a long
/// filesystem name wrapped onto its own line doesn't shift the columns.
pub(crate) fn parse_df_available(out: &str) -> Option<u64> {
    let tokens: Vec<&str> = out
        .lines()
        .skip(1)
        .flat_map(str::split_whitespace)
        .collect();
    tokens.get(3)?.parse::<u64>().ok().map(|kib| kib * 1024)
}

/// Bytes from `du -sk <path>` (`<kib>\t<path>`).
pub(crate) fn parse_du_bytes(out: &str) -> Option<u64> {
    out.split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()
        .map(|kib| kib * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_paths() {
        for ok in [
            "/sdcard",
            "/sdcard/Games",
            "/storage/emulated/0/Games/Baldur's Gate 3",
            "/storage/1A2B-3C4D/Juegos",
        ] {
            assert!(validate_remote_path(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "/data/data/app.gamenative",
            "/sdcardx/Games",
            "/sdcard/../data",
            "/sdcard/Games\nrm",
            "sdcard/Games",
        ] {
            assert!(validate_remote_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn quoting_survives_apostrophes() {
        assert_eq!(
            quote("/sdcard/Baldur's Gate"),
            r"'/sdcard/Baldur'\''s Gate'"
        );
        assert_eq!(join("/sdcard/Games/", "Hades"), "/sdcard/Games/Hades");
    }

    #[test]
    fn df_and_du() {
        let df = "Filesystem     1K-blocks     Used Available Use% Mounted on\n\
                  /dev/fuse      115249236 49000000  66249236  43% /storage/emulated\n";
        assert_eq!(parse_df_available(df), Some(66_249_236 * 1024));
        let wrapped = "Filesystem 1K-blocks Used Available Use% Mounted on\n\
                       /dev/some/very/long/name\n 100 40 60 40% /storage/emulated\n";
        assert_eq!(parse_df_available(wrapped), Some(60 * 1024));
        assert_eq!(parse_df_available("df: /nope: No such file"), None);
        assert_eq!(parse_du_bytes("2048\t/sdcard/Games/.x"), Some(2048 * 1024));
    }
}
