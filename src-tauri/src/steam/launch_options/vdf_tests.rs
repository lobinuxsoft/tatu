use super::*;

/// A minimal but structurally faithful localconfig.vdf.
fn sample() -> String {
    "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"1507580\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"PROTON_FSR4_UPGRADE=1 %command%\"\n\t\t\t\t\t}\n\t\t\t\t\t\"480\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LastPlayed\"\t\t\"123\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n".to_string()
}

#[test]
fn reads_existing_launch_options() {
    let got = read_launch_options(&sample(), "1507580").unwrap();
    assert_eq!(got.as_deref(), Some("PROTON_FSR4_UPGRADE=1 %command%"));
}

#[test]
fn reads_none_when_key_absent() {
    // 480 has no LaunchOptions key.
    assert_eq!(read_launch_options(&sample(), "480").unwrap(), None);
}

#[test]
fn reads_none_when_app_absent() {
    assert_eq!(read_launch_options(&sample(), "999999").unwrap(), None);
}

#[test]
fn replaces_existing_value_preserving_rest() {
    let src = sample();
    let out =
        set_launch_options(&src, "1507580", "WINEDLLOVERRIDES=winhttp=n,b %command%").unwrap();
    assert_eq!(
        read_launch_options(&out, "1507580").unwrap().as_deref(),
        Some("WINEDLLOVERRIDES=winhttp=n,b %command%")
    );
    // The sibling app and its key are untouched.
    assert_eq!(read_launch_options(&out, "480").unwrap(), None);
    assert!(out.contains("\"LastPlayed\"\t\t\"123\""));
}

#[test]
fn inserts_key_into_app_without_launch_options() {
    let out =
        set_launch_options(&sample(), "480", "WINEDLLOVERRIDES=winhttp=n,b %command%").unwrap();
    assert_eq!(
        read_launch_options(&out, "480").unwrap().as_deref(),
        Some("WINEDLLOVERRIDES=winhttp=n,b %command%")
    );
    // Existing key in that block survives.
    assert!(out.contains("\"LastPlayed\""));
    // Still valid VDF (balanced, re-parseable).
    assert!(tokenize(&out).is_ok());
}

#[test]
fn creates_app_block_when_absent() {
    let out = set_launch_options(
        &sample(),
        "1145360",
        "WINEDLLOVERRIDES=winhttp=n,b %command%",
    )
    .unwrap();
    assert_eq!(
        read_launch_options(&out, "1145360").unwrap().as_deref(),
        Some("WINEDLLOVERRIDES=winhttp=n,b %command%")
    );
    // Pre-existing apps still resolve.
    assert_eq!(
        read_launch_options(&out, "1507580").unwrap().as_deref(),
        Some("PROTON_FSR4_UPGRADE=1 %command%")
    );
    assert!(tokenize(&out).is_ok());
}

#[test]
fn round_trips_escaped_quotes() {
    let out = set_launch_options(&sample(), "480", "PROTON_CPU_AFFINITY=\"f\" %command%").unwrap();
    assert_eq!(
        read_launch_options(&out, "480").unwrap().as_deref(),
        Some("PROTON_CPU_AFFINITY=\"f\" %command%")
    );
    // Stored form must escape the quotes.
    assert!(out.contains("PROTON_CPU_AFFINITY=\\\"f\\\""));
}

#[test]
fn errors_on_missing_path() {
    let err = read_launch_options("\"NotConfig\"\n{\n}\n", "480").unwrap_err();
    assert!(matches!(err, VdfError::PathNotFound(_)));
}

#[test]
fn errors_on_unbalanced_braces() {
    assert!(matches!(
        tokenize("\"a\"\n{\n"),
        Err(VdfError::Malformed(_))
    ));
}

/// Validate the editor against the real (large) localconfig.vdf if present.
/// Reads every app's LaunchOptions, then exercises set_launch_options on a
/// synthetic app id and confirms the result re-tokenizes cleanly and the
/// other apps still resolve. Never writes to disk. `#[ignore]` because it
/// depends on a local Steam install.
#[test]
#[ignore]
fn round_trips_real_localconfig() {
    let home = std::env::var("HOME").unwrap();
    let mut found = None;
    for base in [
        format!("{home}/.local/share/Steam/userdata"),
        format!("{home}/.steam/steam/userdata"),
    ] {
        let Ok(users) = std::fs::read_dir(&base) else {
            continue;
        };
        for u in users.flatten() {
            let p = u.path().join("config/localconfig.vdf");
            if p.is_file() {
                found = Some(p);
                break;
            }
        }
    }
    let path = found.expect("no real localconfig.vdf found");
    let src = std::fs::read_to_string(&path).unwrap();

    // Tokenizes without error and navigates to the apps section.
    let toks = tokenize(&src).expect("real file tokenizes");
    let apps = navigate(&toks, &src, APPS_PATH).expect("apps path resolves");
    let inner = brace_token_range(&toks, &apps);

    // Read every direct child app id and its LaunchOptions (must not panic).
    let mut i = inner.start;
    let mut sampled: Option<String> = None;
    while i < inner.end {
        if let Tok::Str { inner: r } = &toks[i] {
            let app_id = unescape(&src[r.clone()]);
            let _ = read_launch_options(&src, &app_id).unwrap();
            if sampled.is_none() {
                sampled = Some(app_id);
            }
        }
        i = skip_value(&toks, i);
    }

    // Edit a synthetic, certainly-absent app id: must create a block, keep
    // the file balanced, and leave a pre-existing app readable.
    let edited =
        set_launch_options(&src, "999999999", "WINEDLLOVERRIDES=winhttp=n,b %command%").unwrap();
    tokenize(&edited).expect("edited file still balanced");
    assert_eq!(
        read_launch_options(&edited, "999999999")
            .unwrap()
            .as_deref(),
        Some("WINEDLLOVERRIDES=winhttp=n,b %command%")
    );
    if let Some(existing) = sampled {
        // The sampled app still resolves identically after the edit.
        assert_eq!(
            read_launch_options(&edited, &existing).unwrap(),
            read_launch_options(&src, &existing).unwrap()
        );
    }
    // Only our insertion changed the length; everything else preserved.
    assert!(edited.len() > src.len());
    assert!(edited.contains(&src[src.len() - 50..]));
}
