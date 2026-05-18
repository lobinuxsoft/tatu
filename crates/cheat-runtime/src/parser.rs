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
    /// `alloc(symbol, size)` — allocate writable+executable memory in the
    /// target process. Size accepts decimal or `0x`-prefixed hex.
    Alloc { symbol: String, size: u64 },
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

    Ok(Script {
        enable: parse_section(enable_section)?,
        disable: parse_section(disable_section)?,
    })
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
    if parts.len() != 2 {
        return Err(ParseError::BadCall {
            fn_name: "alloc".into(),
            detail: format!("expected 2 args, got {}", parts.len()),
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
    Ok(Statement::Alloc {
        symbol: symbol.to_string(),
        size,
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
                size: 1024
            }
        );
        assert_eq!(
            classify("alloc(codecave,0x400)").unwrap(),
            Statement::Alloc {
                symbol: "codecave".into(),
                size: 1024
            }
        );
        assert_eq!(
            classify("alloc(codecave,$400)").unwrap(),
            Statement::Alloc {
                symbol: "codecave".into(),
                size: 1024
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
                Statement::Alloc { symbol, size } if symbol == "codecave" => Some(*size),
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
}
