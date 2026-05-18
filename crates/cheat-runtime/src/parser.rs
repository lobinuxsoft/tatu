//! Cheat Engine Auto-Assembler parser.
//!
//! Parses the dialect used in CE `.CT` `<AssemblerScript>` blocks and inside
//! CheatHappens Aurora trainer payloads (which we observed to be byte-for-byte
//! CE Auto-Assembler wrapped in JSON). Input always has two top-level blocks:
//!
//! ```text
//! [ENABLE]
//! aobscanmodule(originalcode_7149,$process,6F 4C ?? 4A) //unique
//! registersymbol(originalcode_7149)
//! alloc(codecave,1024)
//! codecave:
//!   push ebx
//!   mov dword ptr [r13+13C],(float)100
//! originalcode_7149:
//!   jmp codecave
//!   nop 3
//!
//! [DISABLE]
//! unregistersymbol(originalcode_7149)
//! dealloc(codecave)
//! ```
//!
//! The parser is **line-based and lossless**: every line in the source maps
//! to exactly one [`Statement`], so the executor (subtask 4) can iterate in
//! order, resolve scoping rules (label site → body until next site), and
//! decide which statements it can act on vs. defer.
//!
//! Lines we recognise structurally — `aobscanmodule`, `registersymbol`,
//! `unregistersymbol`, `label`, `alloc`, `dealloc`, label sites (`name:`),
//! and brace directives (`{$...}`) — become typed variants. Everything else
//! (raw assembly, `readmem`, `dq`, helpers) becomes [`Statement::Raw`] with
//! the original line preserved verbatim.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Script {
    pub enable: Vec<Statement>,
    pub disable: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement {
    /// `aobscanmodule(symbol, scope, pattern)` — locate the byte pattern in
    /// the given scope (typically `$process`) and bind `symbol` to the address.
    AobScanModule {
        symbol: String,
        scope: String,
        pattern: String,
    },
    /// `registersymbol(symbol)` — publish a symbol for cross-script lookups.
    RegisterSymbol(String),
    /// `unregistersymbol(symbol)` — remove a previously published symbol.
    UnregisterSymbol(String),
    /// `label(symbol)` — declare a local symbol that will be defined later
    /// by a label site (`symbol:`) somewhere in the same block.
    Label(String),
    /// `alloc(symbol, size [, near])` — allocate writable+executable memory
    /// in the target process. Size accepts decimal or `0x`-prefixed hex.
    /// `near` is the optional 3rd argument from CE AA: an address (or symbol
    /// name resolving to one) the kernel should place the new mapping close
    /// to. Critical when the script later patches a `jmp` from the original
    /// code into the alloc, because `jmp rel32` only reaches ±2 GB.
    Alloc {
        symbol: String,
        size: u64,
        near: Option<String>,
    },
    /// `dealloc(symbol)`.
    Dealloc(String),
    /// `symbol:` at the start of a line. The body of the label (raw bytes,
    /// asm, `readmem(...)`, `dq ...`) is captured as subsequent
    /// [`Statement::Raw`] entries until the next label site, ENABLE/DISABLE
    /// boundary, or end-of-input.
    LabelSite(String),
    /// `0xADDR:` or `<decimal>:` at the start of a line. CE Auto-Assembler
    /// accepts numeric label sites as a way to anchor writes at a known
    /// absolute address without a prior `aobscanmodule` or `registersymbol`.
    /// The executor sets the cursor to the address directly and the body is
    /// captured as subsequent [`Statement::Raw`] entries, identical to the
    /// symbolic [`Statement::LabelSite`] semantics.
    AbsoluteSite(u64),
    /// `{$begin_obfuscate}`, `{$end_obfuscate}`, `{$lua}`, `{$asm}` and other
    /// CE compiler directives. Preserved with surrounding braces intact.
    Directive(String),
    /// Any line not matched above. Includes assembly instructions, `readmem`,
    /// `dq`, `db`, `nop N`, etc. The executor decides whether to assemble or
    /// reject.
    Raw(String),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("missing [ENABLE] block")]
    MissingEnable,
    #[error("malformed `{fn_name}` call: {detail}")]
    BadCall { fn_name: String, detail: String },
}

pub fn parse(input: &str) -> Result<Script, ParseError> {
    let enable_idx = find_block_header(input, "ENABLE").ok_or(ParseError::MissingEnable)?;
    let disable_idx = find_block_header(input, "DISABLE");

    let (enable_section, disable_section) = match disable_idx {
        Some(d) if d > enable_idx => (
            section_after(input, enable_idx, Some(d)),
            section_after(input, d, None),
        ),
        _ => (section_after(input, enable_idx, None), ""),
    };

    let enable = resolve_anonymous_refs(parse_section(enable_section)?)?;
    let disable = resolve_anonymous_refs(parse_section(disable_section)?)?;
    Ok(Script { enable, disable })
}

/// Rewrite CE-AA anonymous-label tokens (`@f` forward / `@b` back) inside
/// `Raw` statements to the actual label name they would resolve to. CE walks
/// the statement list, tracking the most recently seen `LabelSite`. `@f`
/// becomes `labels[lastseen + 1]` and `@b` becomes `labels[lastseen]`.
/// `@@:` is also accepted but auto-renamed to a unique synthetic label that
/// then joins the regular list.
///
/// Ported from `Cheat Engine/autoassembler.pas:815-896` — same algorithm,
/// minus the random-letter rename for `@@:` (we use a deterministic
/// ordinal so error messages stay readable in tests).
fn resolve_anonymous_refs(mut stmts: Vec<Statement>) -> Result<Vec<Statement>, ParseError> {
    // First sweep: rename `@@:` LabelSites to `__anon_<ord>` so the
    // second sweep treats them as regular labels.
    let mut anon_counter = 0usize;
    for s in stmts.iter_mut() {
        if let Statement::LabelSite(name) = s
            && name == "@@"
        {
            *name = format!("__anon_{anon_counter}");
            anon_counter += 1;
        }
    }

    // Collect every LabelSite name in document order.
    let labels: Vec<String> = stmts
        .iter()
        .filter_map(|s| match s {
            Statement::LabelSite(n) => Some(n.clone()),
            _ => None,
        })
        .collect();

    // Second sweep: track `lastseen` index of the most recent LabelSite and
    // rewrite `@f` / `@b` tokens inside Raw lines accordingly.
    let mut lastseen: Option<usize> = None;
    for s in stmts.iter_mut() {
        match s {
            Statement::LabelSite(n) => {
                lastseen = labels.iter().position(|l| l == n);
            }
            Statement::Raw(line) => {
                if !contains_anon_token(line) {
                    continue;
                }
                let resolved = rewrite_anonymous_tokens(line, &labels, lastseen)?;
                *line = resolved;
            }
            _ => {}
        }
    }
    Ok(stmts)
}

fn contains_anon_token(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    has_word_token(&lower, "@f") || has_word_token(&lower, "@b")
}

fn rewrite_anonymous_tokens(
    line: &str,
    labels: &[String],
    lastseen: Option<usize>,
) -> Result<String, ParseError> {
    let lower = line.to_ascii_lowercase();
    let mut out = line.to_string();
    if has_word_token(&lower, "@f") {
        let next_idx = lastseen.map_or(0, |i| i + 1);
        let target = labels.get(next_idx).ok_or_else(|| ParseError::BadCall {
            fn_name: "@f".into(),
            detail: format!("no label declared after this point in {line:?}"),
        })?;
        out = replace_word_token(&out, "@f", target);
        out = replace_word_token(&out, "@F", target);
    }
    if has_word_token(&lower, "@b") {
        let prev_idx = lastseen.ok_or_else(|| ParseError::BadCall {
            fn_name: "@b".into(),
            detail: format!("no label declared before this point in {line:?}"),
        })?;
        let target = &labels[prev_idx];
        out = replace_word_token(&out, "@b", target);
        out = replace_word_token(&out, "@B", target);
    }
    Ok(out)
}

/// Token-boundary aware search: returns `true` if `needle` appears in
/// `haystack` not surrounded by identifier characters (`@` counts as part
/// of the token here so `@f` doesn't match inside `foo@f` accidentally).
fn has_word_token(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let n = needle.as_bytes();
    let mut i = 0;
    while i + n.len() <= bytes.len() {
        if &bytes[i..i + n.len()] == n {
            let after_ok = bytes.get(i + n.len()).map_or(true, |c| !is_token_char(*c));
            if after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn replace_word_token(haystack: &str, needle: &str, replacement: &str) -> String {
    let bytes = haystack.as_bytes();
    let n = needle.as_bytes();
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0;
    while i + n.len() <= bytes.len() {
        if &bytes[i..i + n.len()] == n {
            let after_ok = bytes.get(i + n.len()).map_or(true, |c| !is_token_char(*c));
            if after_ok {
                out.push_str(replacement);
                i += n.len();
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out.push_str(&haystack[i..]);
    out
}

fn is_token_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn find_block_header(input: &str, tag: &str) -> Option<usize> {
    let needle = format!("[{tag}]");
    let pos = input.find(&needle)?;
    Some(pos + needle.len())
}

fn section_after(input: &str, start: usize, end: Option<usize>) -> &str {
    let bytes = input.as_bytes();
    // start is just past `[ENABLE]`/`[DISABLE]` — skip the following newline.
    let mut s = start;
    while s < bytes.len() && (bytes[s] == b'\r' || bytes[s] == b'\n') {
        s += 1;
    }
    let e = match end {
        // `end` points past `[DISABLE]`, so back up to the start of that line.
        Some(e) => e - b"[DISABLE]".len(),
        None => bytes.len(),
    };
    &input[s..e]
}

fn parse_section(section: &str) -> Result<Vec<Statement>, ParseError> {
    section.lines().filter_map(parse_line).collect()
}

fn parse_line(raw: &str) -> Option<Result<Statement, ParseError>> {
    let trimmed = strip_comment(raw).trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(classify(trimmed))
}

fn strip_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

fn classify(line: &str) -> Result<Statement, ParseError> {
    // Compiler directive: `{$...}` (possibly with trailing whitespace inside).
    if line.starts_with("{$") && line.ends_with('}') {
        return Ok(Statement::Directive(line.to_string()));
    }
    // Label site: ends with `:`, has no whitespace inside the name part.
    if let Some(name) = line.strip_suffix(':') {
        let name = name.trim();
        if is_identifier(name) {
            return Ok(Statement::LabelSite(name.to_string()));
        }
        // CE-AA anonymous label `@@:` — flows through to
        // `resolve_anonymous_refs`, which renames it to `__anon_<ord>`.
        if name == "@@" {
            return Ok(Statement::LabelSite("@@".to_string()));
        }
        if let Some(addr) = parse_size(name) {
            return Ok(Statement::AbsoluteSite(addr));
        }
    }
    // Function-call style commands: `name(args)`.
    if let Some(open) = line.find('(')
        && line.ends_with(')')
    {
        let fn_name = line[..open].trim();
        let args = &line[open + 1..line.len() - 1];
        match fn_name {
            "aobscanmodule" => return parse_aobscan(args),
            "registersymbol" => {
                return parse_unary(args, "registersymbol", Statement::RegisterSymbol);
            }
            "unregistersymbol" => {
                return parse_unary(args, "unregistersymbol", Statement::UnregisterSymbol);
            }
            "label" => return parse_unary(args, "label", Statement::Label),
            "alloc" => return parse_alloc(args),
            "dealloc" => return parse_unary(args, "dealloc", Statement::Dealloc),
            _ => {}
        }
    }
    Ok(Statement::Raw(line.to_string()))
}

fn parse_aobscan(args: &str) -> Result<Statement, ParseError> {
    let parts: Vec<&str> = args.splitn(3, ',').map(str::trim).collect();
    if parts.len() != 3 {
        return Err(ParseError::BadCall {
            fn_name: "aobscanmodule".into(),
            detail: format!("expected 3 args, got {}", parts.len()),
        });
    }
    Ok(Statement::AobScanModule {
        symbol: parts[0].to_string(),
        scope: parts[1].to_string(),
        pattern: parts[2].to_string(),
    })
}

fn parse_unary<F>(args: &str, fn_name: &str, ctor: F) -> Result<Statement, ParseError>
where
    F: FnOnce(String) -> Statement,
{
    let symbol = args.trim();
    if !is_identifier(symbol) {
        return Err(ParseError::BadCall {
            fn_name: fn_name.into(),
            detail: format!("invalid identifier {symbol:?}"),
        });
    }
    Ok(ctor(symbol.to_string()))
}

fn parse_alloc(args: &str) -> Result<Statement, ParseError> {
    let parts: Vec<&str> = args.split(',').map(str::trim).collect();
    if !(parts.len() == 2 || parts.len() == 3) {
        return Err(ParseError::BadCall {
            fn_name: "alloc".into(),
            detail: format!("expected 2 or 3 args, got {}", parts.len()),
        });
    }
    let symbol = parts[0];
    if !is_identifier(symbol) {
        return Err(ParseError::BadCall {
            fn_name: "alloc".into(),
            detail: format!("invalid identifier {symbol:?}"),
        });
    }
    let size = parse_size(parts[1]).ok_or_else(|| ParseError::BadCall {
        fn_name: "alloc".into(),
        detail: format!("invalid size {:?}", parts[1]),
    })?;
    let near = parts.get(2).map(|s| s.to_string());
    Ok(Statement::Alloc {
        symbol: symbol.to_string(),
        size,
        near,
    })
}

fn parse_size(token: &str) -> Option<u64> {
    if let Some(hex) = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16).ok()
    } else if let Some(hex) = token.strip_prefix('$') {
        u64::from_str_radix(hex, 16).ok()
    } else {
        token.parse::<u64>().ok()
    }
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
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
    fn parse_registersymbol_and_friends() {
        assert_eq!(
            classify("registersymbol(foo)").unwrap(),
            Statement::RegisterSymbol("foo".into())
        );
        assert_eq!(
            classify("unregistersymbol(foo)").unwrap(),
            Statement::UnregisterSymbol("foo".into())
        );
        assert_eq!(
            classify("label(foo)").unwrap(),
            Statement::Label("foo".into())
        );
        assert_eq!(
            classify("dealloc(foo)").unwrap(),
            Statement::Dealloc("foo".into())
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
        assert_eq!(stmt, Statement::RegisterSymbol("x".into()));
    }

    #[test]
    fn missing_enable_block_errors() {
        assert_eq!(
            parse("just some noise\n[DISABLE]\n"),
            Err(ParseError::MissingEnable)
        );
    }

    #[test]
    fn parse_minimal_script() {
        let src = "[ENABLE]\nregistersymbol(foo)\n\n[DISABLE]\nunregistersymbol(foo)\n";
        let s = parse(src).unwrap();
        assert_eq!(s.enable, vec![Statement::RegisterSymbol("foo".into())]);
        assert_eq!(s.disable, vec![Statement::UnregisterSymbol("foo".into())]);
    }

    #[test]
    fn parse_aurora_em_unlharvestflag_fixture() {
        let src = include_str!("../tests/fixtures/aurora_em_unlharvestflag.txt");
        let script = parse(src).expect("real Aurora script must parse");

        // The ENABLE block must contain the aobscan, registers, alloc, labels.
        let names: Vec<_> = script
            .enable
            .iter()
            .filter_map(|s| match s {
                Statement::AobScanModule { symbol, .. } => Some(("aob", symbol.as_str())),
                Statement::RegisterSymbol(s) => Some(("reg", s.as_str())),
                Statement::Alloc { symbol, .. } => Some(("alloc", symbol.as_str())),
                Statement::Label(s) => Some(("label", s.as_str())),
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
        let has_dealloc = script
            .disable
            .iter()
            .any(|s| matches!(s, Statement::Dealloc(name) if name == "codecave"));
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
        let src = include_str!("../tests/fixtures/aurora_em_unlharvestflag.txt");
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
}
