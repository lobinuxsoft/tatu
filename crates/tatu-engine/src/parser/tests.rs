//! Parser module tests.

use super::anonymous::has_word_token;
use super::*;

#[test]
fn parse_aobscanmodule_with_pattern() {
    let stmt = classify("aobscanmodule(originalcode_7149,$process,6F 4C ?? 4A)").unwrap();
    assert_eq!(
        stmt,
        Statement::AobScanModule {
            symbol: "originalcode_7149".into(),
            scope: "$process".into(),
            pattern: "6F 4C ?? 4A".into(),
        }
    );
}

#[test]
fn parse_command_names_are_case_insensitive() {
    // CE-AA authors (e.g. the Manifold framework on the DD2 table) write
    // commands in camelCase. The dispatch must normalise case while leaving
    // the symbol name and byte pattern untouched.
    let stmt =
        classify("aobScanModule(IsPlayerInvincibilityHook,DD2.exe,80 7F 15 ? 75 ? 48 8B)").unwrap();
    assert_eq!(
        stmt,
        Statement::AobScanModule {
            symbol: "IsPlayerInvincibilityHook".into(),
            scope: "DD2.exe".into(),
            pattern: "80 7F 15 ? 75 ? 48 8B".into(),
        }
    );
    assert_eq!(
        classify("unregisterSymbol(foo)").unwrap(),
        Statement::UnregisterSymbol(NameList::single("foo"))
    );
    assert_eq!(
        classify("AOBSCANMODULE(s,m.exe,90)").unwrap(),
        Statement::AobScanModule {
            symbol: "s".into(),
            scope: "m.exe".into(),
            pattern: "90".into(),
        }
    );
}

#[test]
fn parse_symbol_offset_site() {
    // CE-AA hook injection: `Symbol+N:` anchors writes at a scanned symbol
    // plus a byte offset (the DD2 Player Invincibility 1-byte patch shape).
    assert_eq!(
        classify("IsPlayerInvincibilityHook+3:").unwrap(),
        Statement::SymbolSite {
            symbol: "IsPlayerInvincibilityHook".into(),
            offset: 3,
        }
    );
    assert_eq!(
        classify("StaminaWorkHook-8:").unwrap(),
        Statement::SymbolSite {
            symbol: "StaminaWorkHook".into(),
            offset: -8,
        }
    );
    // Plain identifier stays a LabelSite (defines, not resolves).
    assert_eq!(
        classify("codecave:").unwrap(),
        Statement::LabelSite("codecave".into())
    );
    // Module-relative (`.exe` carries a dot) is not an identifier base →
    // falls through to Raw, unchanged from prior behaviour.
    assert_eq!(
        classify("DD2.exe+1062702:").unwrap(),
        Statement::Raw("DD2.exe+1062702:".into())
    );
}

#[test]
fn parse_registersymbol_and_friends() {
    assert_eq!(
        classify("registersymbol(foo)").unwrap(),
        Statement::RegisterSymbol(NameList::single("foo"))
    );
    assert_eq!(
        classify("unregistersymbol(foo)").unwrap(),
        Statement::UnregisterSymbol(NameList::single("foo"))
    );
    assert_eq!(
        classify("label(foo)").unwrap(),
        Statement::Label(NameList::single("foo"))
    );
    assert_eq!(
        classify("dealloc(foo)").unwrap(),
        Statement::Dealloc(NameList::single("foo"))
    );
}

#[test]
fn parse_name_list_wildcard() {
    // `(*)` — emitted by FearLess `.CT` authors at the top of [DISABLE]
    // blocks to "release everything this script registered/allocated".
    assert_eq!(
        classify("unregistersymbol(*)").unwrap(),
        Statement::UnregisterSymbol(NameList::Wildcard)
    );
    assert_eq!(
        classify("dealloc(*)").unwrap(),
        Statement::Dealloc(NameList::Wildcard)
    );
    // Trim around the `*` is OK too — `unregistersymbol( * )` is in the
    // wild (Crimson Desert tables).
    assert_eq!(
        classify("unregistersymbol( * )").unwrap(),
        Statement::UnregisterSymbol(NameList::Wildcard)
    );
}

#[test]
fn parse_name_list_comma_separated() {
    assert_eq!(
        classify("registersymbol(a,b,c)").unwrap(),
        Statement::RegisterSymbol(NameList::Names(vec!["a".into(), "b".into(), "c".into()]))
    );
}

#[test]
fn parse_name_list_space_separated() {
    // Space-separated lists are the dominant multi-name shape in the
    // FearLess corpus (e.g. Dragon's Dogma 2 / Crimson Desert tables).
    assert_eq!(
        classify("label(pStamina pHealth bMaxStamina)").unwrap(),
        Statement::Label(NameList::Names(vec![
            "pStamina".into(),
            "pHealth".into(),
            "bMaxStamina".into(),
        ]))
    );
    assert_eq!(
        classify("unregistersymbol(pPlayer originalcode_playerBaseReadAOB)").unwrap(),
        Statement::UnregisterSymbol(NameList::Names(vec![
            "pPlayer".into(),
            "originalcode_playerBaseReadAOB".into(),
        ]))
    );
}

#[test]
fn parse_globalalloc_and_define() {
    assert_eq!(
        classify("globalalloc(newmem,0x100)").unwrap(),
        Statement::GlobalAlloc {
            symbol: "newmem".into(),
            size: 0x100,
        }
    );
    assert_eq!(
        classify("define(injectOffset, eldenring.exe+CD2FC1)").unwrap(),
        Statement::Define {
            name: "injectOffset".into(),
            value: "eldenring.exe+CD2FC1".into(),
        }
    );
    // A numeric `define` is something the executor can resolve eagerly.
    assert_eq!(
        classify("define(slot, 0x40)").unwrap(),
        Statement::Define {
            name: "slot".into(),
            value: "0x40".into(),
        }
    );
}

#[test]
fn parse_lua_only_script_does_not_error() {
    // CE scripts whose entire body is `{$lua}` have no `[ENABLE]`
    // header. Tatu can't run them but must not break the surrounding
    // table — the script surfaces as `lua_only = true` with empty
    // enable/disable blocks instead of `MissingEnable`.
    let s = parse("{$lua}\npause()\n").expect("lua-only script must parse");
    assert!(s.lua_only);
    assert!(s.enable.is_empty());
    assert!(s.disable.is_empty());
}

#[test]
fn parse_marks_enable_with_lua_directive_as_lua_only() {
    let s = parse("[ENABLE]\n{$lua}\nautoAssemble([[ ... ]])\n[DISABLE]\n").expect("must parse");
    assert!(
        s.lua_only,
        "{{$lua}} inside an ENABLE block means the script is functionally lua-only"
    );
}

#[test]
fn parse_alloc_decimal_and_hex() {
    assert_eq!(
        classify("alloc(codecave,1024)").unwrap(),
        Statement::Alloc {
            symbol: "codecave".into(),
            size: 1024,
            near: None,
        }
    );
    assert_eq!(
        classify("alloc(codecave,0x400)").unwrap(),
        Statement::Alloc {
            symbol: "codecave".into(),
            size: 1024,
            near: None,
        }
    );
    assert_eq!(
        classify("alloc(codecave,$400)").unwrap(),
        Statement::Alloc {
            symbol: "codecave".into(),
            size: 1024,
            near: None,
        }
    );
}

#[test]
fn parse_alloc_with_near_hint() {
    assert_eq!(
        classify("alloc(codecave,1024,originalcode)").unwrap(),
        Statement::Alloc {
            symbol: "codecave".into(),
            size: 1024,
            near: Some("originalcode".into()),
        }
    );
}

#[test]
fn parse_label_site() {
    assert_eq!(
        classify("codecave:").unwrap(),
        Statement::LabelSite("codecave".into())
    );
    assert_eq!(
        classify("originalcode_7149:").unwrap(),
        Statement::LabelSite("originalcode_7149".into())
    );
}

#[test]
fn parse_absolute_label_site_hex() {
    assert_eq!(
        classify("0xB056EC28:").unwrap(),
        Statement::AbsoluteSite(0xB056_EC28)
    );
    assert_eq!(
        classify("0X2A39E658:").unwrap(),
        Statement::AbsoluteSite(0x2A39_E658)
    );
    assert_eq!(classify("$DEAD:").unwrap(), Statement::AbsoluteSite(0xDEAD));
}

#[test]
fn parse_absolute_label_site_decimal() {
    assert_eq!(
        classify("708486264:").unwrap(),
        Statement::AbsoluteSite(708_486_264)
    );
}

#[test]
fn parse_directive_braces() {
    assert_eq!(
        classify("{$begin_obfuscate}").unwrap(),
        Statement::Directive("{$begin_obfuscate}".into())
    );
    assert_eq!(
        classify("{$end_obfuscate}").unwrap(),
        Statement::Directive("{$end_obfuscate}".into())
    );
}

#[test]
fn unknown_line_becomes_raw() {
    assert_eq!(
        classify("push ebx").unwrap(),
        Statement::Raw("push ebx".into())
    );
    assert_eq!(
        classify("readmem(originalcode_7149, 8)").unwrap(),
        Statement::Raw("readmem(originalcode_7149, 8)".into())
    );
    assert_eq!(classify("dq 0").unwrap(), Statement::Raw("dq 0".into()));
}

#[test]
fn comment_stripping() {
    let stmt = classify(strip_comment("registersymbol(x) //note here").trim()).unwrap();
    assert_eq!(stmt, Statement::RegisterSymbol(NameList::single("x")));
}

#[test]
fn missing_enable_block_errors() {
    // Truly malformed input (no [ENABLE] AND no `{$lua}`) still errors —
    // we only soften the case where the source is a recognisable Lua
    // payload (see [`parse_lua_only_script_does_not_error`]).
    assert_eq!(
        parse("just some noise\n[DISABLE]\n"),
        Err(ParseError::MissingEnable)
    );
}

#[test]
fn parse_minimal_script() {
    let src = "[ENABLE]\nregistersymbol(foo)\n\n[DISABLE]\nunregistersymbol(foo)\n";
    let s = parse(src).unwrap();
    assert_eq!(
        s.enable,
        vec![Statement::RegisterSymbol(NameList::single("foo"))]
    );
    assert_eq!(
        s.disable,
        vec![Statement::UnregisterSymbol(NameList::single("foo"))]
    );
}

#[test]
fn parse_aurora_em_unlharvestflag_fixture() {
    let src = include_str!("../../tests/fixtures/aurora_em_unlharvestflag.txt");
    let script = parse(src).expect("real Aurora script must parse");

    // The ENABLE block must contain the aobscan, registers, alloc, labels.
    let names: Vec<_> = script
        .enable
        .iter()
        .filter_map(|s| match s {
            Statement::AobScanModule { symbol, .. } => Some(("aob", symbol.as_str())),
            Statement::RegisterSymbol(list) => list.names().first().map(|n| ("reg", n.as_str())),
            Statement::Alloc { symbol, .. } => Some(("alloc", symbol.as_str())),
            Statement::Label(list) => list.names().first().map(|n| ("label", n.as_str())),
            Statement::LabelSite(s) => Some(("site", s.as_str())),
            Statement::Directive(_) => Some(("dir", "_")),
            _ => None,
        })
        .collect();

    // Sanity: must see the AOB scan with the expected symbol.
    assert!(
        names
            .iter()
            .any(|&(k, n)| k == "aob" && n == "originalcode_7149"),
        "expected aobscanmodule(originalcode_7149, ...), got {names:?}"
    );
    // Must capture the alloc(codecave, 1024).
    let alloc = script
        .enable
        .iter()
        .find_map(|s| match s {
            Statement::Alloc { symbol, size, .. } if symbol == "codecave" => Some(*size),
            _ => None,
        })
        .expect("alloc(codecave,1024) must be present");
    assert_eq!(alloc, 1024);
    // Both directive markers must be present.
    let dir_count = script
        .enable
        .iter()
        .filter(|s| matches!(s, Statement::Directive(_)))
        .count();
    assert_eq!(
        dir_count, 2,
        "expected {{$begin_obfuscate}} and {{$end_obfuscate}}"
    );

    // DISABLE block must unregister and dealloc.
    let has_dealloc = script.disable.iter().any(|s| {
        matches!(
            s,
            Statement::Dealloc(NameList::Names(names))
                if names.iter().any(|n| n == "codecave")
        )
    });
    assert!(has_dealloc, "DISABLE must contain dealloc(codecave)");
    let unreg_count = script
        .disable
        .iter()
        .filter(|s| matches!(s, Statement::UnregisterSymbol(_)))
        .count();
    assert!(
        unreg_count >= 3,
        "expected at least 3 unregistersymbol calls"
    );
}

#[test]
fn parse_section_handles_crlf() {
    let src = "[ENABLE]\r\nregistersymbol(foo)\r\n[DISABLE]\r\nunregistersymbol(foo)\r\n";
    let s = parse(src).unwrap();
    assert_eq!(s.enable.len(), 1);
    assert_eq!(s.disable.len(), 1);
}

#[test]
fn alloc_with_bad_size_errors() {
    let err = classify("alloc(codecave,notanumber)").unwrap_err();
    assert!(matches!(err, ParseError::BadCall { fn_name, .. } if fn_name == "alloc"));
}

#[test]
fn aobscan_with_two_args_errors() {
    let err = classify("aobscanmodule(foo,$process)").unwrap_err();
    assert!(matches!(err, ParseError::BadCall { fn_name, .. } if fn_name == "aobscanmodule"));
}

#[test]
fn parse_aobscan_global_two_args() {
    // CE-AA's 2-arg form for Mono / JIT-emitted targets where the
    // executable image can't carry the bytes. Enigma of Fear and
    // similar Unity tables use this almost exclusively.
    let stmt = classify("aobscan(INJECT,F2 0F 5C C1 F2 0F 5A E8)").unwrap();
    assert_eq!(
        stmt,
        Statement::AobScan {
            symbol: "INJECT".into(),
            pattern: "F2 0F 5C C1 F2 0F 5A E8".into(),
        }
    );
}

#[test]
fn aobscan_with_one_arg_errors() {
    let err = classify("aobscan(foo)").unwrap_err();
    assert!(matches!(err, ParseError::BadCall { fn_name, .. } if fn_name == "aobscan"));
}

#[test]
fn anonymous_forward_label_rewritten_to_next_label() {
    let src = "[ENABLE]\n\
               codecave:\n\
               cmp eax, 1\n\
               jne @f\n\
               mov eax, 2\n\
               code:\n\
               ret\n\
               [DISABLE]\n";
    let s = parse(src).unwrap();
    // The `jne @f` should have been rewritten to `jne code` because
    // `code:` is the next LabelSite after the codecave block.
    let rewritten = s
        .enable
        .iter()
        .find_map(|s| match s {
            Statement::Raw(line) if line.starts_with("jne ") => Some(line.clone()),
            _ => None,
        })
        .expect("jne @f rewritten");
    assert_eq!(rewritten, "jne code");
}

#[test]
fn anonymous_backward_label_rewritten_to_previous() {
    let src = "[ENABLE]\n\
               start:\n\
               add eax, 1\n\
               loop_body:\n\
               sub eax, 1\n\
               jne @b\n\
               ret\n\
               [DISABLE]\n";
    let s = parse(src).unwrap();
    // `@b` resolves to the most recent label before the line,
    // which is `loop_body`.
    let rewritten = s
        .enable
        .iter()
        .find_map(|s| match s {
            Statement::Raw(line) if line.starts_with("jne ") => Some(line.clone()),
            _ => None,
        })
        .expect("jne @b rewritten");
    assert_eq!(rewritten, "jne loop_body");
}

#[test]
fn double_at_label_gets_synthetic_name() {
    let src = "[ENABLE]\n\
               @@:\n\
               jmp @b\n\
               [DISABLE]\n";
    let s = parse(src).unwrap();
    let site = s.enable.iter().find_map(|s| match s {
        Statement::LabelSite(n) => Some(n.clone()),
        _ => None,
    });
    assert_eq!(site.as_deref(), Some("__anon_0"));
    let raw = s.enable.iter().find_map(|s| match s {
        Statement::Raw(line) => Some(line.clone()),
        _ => None,
    });
    assert_eq!(raw.as_deref(), Some("jmp __anon_0"));
}

#[test]
fn forward_anon_without_next_label_errors() {
    let src = "[ENABLE]\n\
               start:\n\
               jmp @f\n\
               [DISABLE]\n";
    let err = parse(src).unwrap_err();
    assert!(matches!(err, ParseError::BadCall { fn_name, .. } if fn_name == "@f"));
}

#[test]
fn anon_token_inside_identifier_not_replaced() {
    // `foo@f` and `@for` should not match the `@f` token.
    let src = "[ENABLE]\n\
               anchor:\n\
               db DE @forward AD\n\
               next:\n\
               [DISABLE]\n";
    let s = parse(src).unwrap();
    // db line should be preserved verbatim — `@forward` is its own
    // identifier and is not the `@f` token.
    let raw = s.enable.iter().find_map(|s| match s {
        Statement::Raw(line) => Some(line.clone()),
        _ => None,
    });
    assert_eq!(raw.as_deref(), Some("db DE @forward AD"));
}

/// The real Ender Magnolia `unlHarvestFlag` Aurora trainer parses
/// end-to-end and `jne @f` resolves to the next regular label
/// (`code`) — that's the path CE's `autoassembler.pas:875` takes
/// when no explicit `@@:` precedes the `@f`.
#[test]
fn em_fixture_parses_and_resolves_anon_forward() {
    let src = include_str!("../../tests/fixtures/aurora_em_unlharvestflag.txt");
    let s = parse(src).expect("EM fixture must parse");

    // `jne @f` should have been rewritten to `jne code` — `code:` is
    // the next label after the codecave body.
    let jne_line = s
        .enable
        .iter()
        .find_map(|stmt| match stmt {
            Statement::Raw(line) if line.starts_with("jne ") => Some(line.clone()),
            _ => None,
        })
        .expect("jne line present");
    assert_eq!(jne_line, "jne code");

    // No raw line still contains a literal `@f` / `@b` token after
    // resolution.
    for stmt in &s.enable {
        if let Statement::Raw(line) = stmt {
            assert!(
                !has_word_token(&line.to_ascii_lowercase(), "@f"),
                "unresolved @f in {line:?}"
            );
            assert!(
                !has_word_token(&line.to_ascii_lowercase(), "@b"),
                "unresolved @b in {line:?}"
            );
        }
    }
}
