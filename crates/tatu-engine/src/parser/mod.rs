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

mod anonymous;

use self::anonymous::resolve_anonymous_refs;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Script {
    pub enable: Vec<Statement>,
    pub disable: Vec<Statement>,
    /// `true` when the script body has no `[ENABLE]` block but the
    /// source contains a `{$lua}` directive. Such scripts are pure-Lua
    /// payloads CE evaluates with its embedded interpreter; tatu has no
    /// Lua VM and cannot execute them, but the parser surfaces this
    /// state instead of erroring so the UI can mark the feature as
    /// "lua-not-supported" while the rest of the table keeps working.
    pub lua_only: bool,
}

/// A list of names attached to `(un)registersymbol` / `label` / `dealloc`.
/// CE-AA permits four shapes per call:
/// `name`, `name1, name2, name3`, `name1 name2 name3`, and `*` (wildcard).
/// The parser normalises all of them into this enum so downstream code
/// has a single shape to match on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameList {
    /// `(*)` shorthand for "everything this script registered/allocated".
    Wildcard,
    /// One or more explicit names. Always non-empty.
    Names(Vec<String>),
}

impl NameList {
    /// Build a one-element name list. Convenience for callers (tests,
    /// programmatic script construction) that have a single name and
    /// want to avoid spelling out the `Names(vec![…])` shape.
    pub fn single(name: impl Into<String>) -> Self {
        NameList::Names(vec![name.into()])
    }

    pub fn is_wildcard(&self) -> bool {
        matches!(self, NameList::Wildcard)
    }

    /// Iterate the names. Wildcard yields nothing — callers that want
    /// to act on `Wildcard` should branch on [`Self::is_wildcard`] first.
    pub fn names(&self) -> &[String] {
        match self {
            NameList::Wildcard => &[],
            NameList::Names(v) => v,
        }
    }
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
    /// `registersymbol(name [name …])` — publish one or more symbols for
    /// cross-script lookups. CE-AA accepts both `name`, `a, b, c` and
    /// `a b c` shapes; all normalise to a [`NameList`]. Wildcard `(*)` is
    /// also accepted (CE shorthand "all symbols defined in this script") —
    /// for `registersymbol` the executor treats it as a no-op since we have
    /// no implicit-symbol concept.
    RegisterSymbol(NameList),
    /// `unregistersymbol(name [name …])` — remove published symbols.
    /// `unregistersymbol(*)` is the dominant idiom at the top of
    /// `[DISABLE]` blocks ("clear everything this script published").
    UnregisterSymbol(NameList),
    /// `label(name [name …])` — declare one or more local symbols that
    /// will be defined later by a label site (`name:`) in the same block.
    Label(NameList),
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
    /// `globalalloc(symbol, size)` — CE-AA "global scope" allocation. The
    /// difference vs [`Statement::Alloc`] is that CE keeps the allocation
    /// alive across script enables until an explicit `dealloc`. Tatu's
    /// executor scopes both to the script lifetime (rollback frees them);
    /// we accept `globalalloc` shape-equivalent to `alloc` because the wild
    /// uses it rarely and CE's "global" semantics do not apply to our
    /// per-toggle execution model.
    GlobalAlloc { symbol: String, size: u64 },
    /// `dealloc(name [name …])`. Wildcard `(*)` releases every codecave
    /// allocated by the current script.
    Dealloc(NameList),
    /// `define(name, value)` — CE-AA token substitution. `value` is taken
    /// verbatim (number, identifier, symbol, expression — the executor
    /// resolves it lazily). Mostly used for module-relative offsets in
    /// large tables (`define(injectOffset, eldenring.exe+CD2FC1)`).
    Define { name: String, value: String },
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
    let Some(enable_idx) = find_block_header(input, "ENABLE") else {
        // No `[ENABLE]` block. Two cases observed in the FearLess corpus:
        //  - Script body is a pure `{$lua}` payload (CE evaluates it with
        //    its embedded Lua interpreter). Tatu has no Lua VM, but we
        //    refuse to error on these so the surrounding table can still
        //    enable its non-lua entries.
        //  - Truly malformed input — those still get `MissingEnable`.
        if input.contains("{$lua}") || input.contains("{$LUA}") {
            return Ok(Script {
                enable: Vec::new(),
                disable: Vec::new(),
                lua_only: true,
            });
        }
        return Err(ParseError::MissingEnable);
    };
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
    // A script whose ENABLE body contains a `{$lua}` directive is also
    // effectively lua-only — CE's Lua block is interpreted in-line and
    // its side effects are invisible to a static AA walker. Mark and
    // surface so the executor can skip with a warning rather than emit
    // a partial / broken patch.
    let lua_only = body_has_lua_directive(&enable) || body_has_lua_directive(&disable);
    Ok(Script {
        enable,
        disable,
        lua_only,
    })
}

fn body_has_lua_directive(stmts: &[Statement]) -> bool {
    stmts.iter().any(|s| {
        matches!(s, Statement::Directive(d) if {
            let l = d.to_ascii_lowercase();
            l == "{$lua}" || l.starts_with("{$lua ")
        })
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
                return parse_name_list_call(args, "registersymbol", Statement::RegisterSymbol);
            }
            "unregistersymbol" => {
                return parse_name_list_call(args, "unregistersymbol", Statement::UnregisterSymbol);
            }
            "label" => return parse_name_list_call(args, "label", Statement::Label),
            "alloc" => return parse_alloc(args),
            "globalalloc" => return parse_globalalloc(args),
            "dealloc" => return parse_name_list_call(args, "dealloc", Statement::Dealloc),
            "define" => return parse_define(args),
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

fn parse_name_list_call<F>(args: &str, fn_name: &str, ctor: F) -> Result<Statement, ParseError>
where
    F: FnOnce(NameList) -> Statement,
{
    let list = parse_name_list(args, fn_name)?;
    Ok(ctor(list))
}

/// Parse the argument list of a name-list call (`label`, `(un)registersymbol`,
/// `dealloc`). Accepts wildcard `*`, comma-separated, or whitespace-separated
/// identifiers — all three appear in the FearLess `.CT` corpus.
fn parse_name_list(args: &str, fn_name: &str) -> Result<NameList, ParseError> {
    let trimmed = args.trim();
    if trimmed == "*" {
        return Ok(NameList::Wildcard);
    }
    // Comma takes precedence: `dealloc(a,b,c)`. If no comma is present,
    // fall back to whitespace separation: `dealloc(a b c)` — both shapes
    // are emitted by CE-table authors interchangeably.
    let names: Vec<&str> = if trimmed.contains(',') {
        trimmed.split(',').map(str::trim).collect()
    } else {
        trimmed.split_whitespace().collect()
    };
    if names.is_empty() {
        return Err(ParseError::BadCall {
            fn_name: fn_name.into(),
            detail: "expected at least one name".into(),
        });
    }
    for n in &names {
        if !is_identifier(n) {
            return Err(ParseError::BadCall {
                fn_name: fn_name.into(),
                detail: format!("invalid identifier {n:?}"),
            });
        }
    }
    Ok(NameList::Names(
        names.into_iter().map(String::from).collect(),
    ))
}

fn parse_globalalloc(args: &str) -> Result<Statement, ParseError> {
    let parts: Vec<&str> = args.split(',').map(str::trim).collect();
    if parts.len() != 2 {
        return Err(ParseError::BadCall {
            fn_name: "globalalloc".into(),
            detail: format!("expected 2 args, got {}", parts.len()),
        });
    }
    if !is_identifier(parts[0]) {
        return Err(ParseError::BadCall {
            fn_name: "globalalloc".into(),
            detail: format!("invalid identifier {:?}", parts[0]),
        });
    }
    let size = parse_size(parts[1]).ok_or_else(|| ParseError::BadCall {
        fn_name: "globalalloc".into(),
        detail: format!("invalid size {:?}", parts[1]),
    })?;
    Ok(Statement::GlobalAlloc {
        symbol: parts[0].to_string(),
        size,
    })
}

/// `define(name, value)`. `value` is captured verbatim — CE accepts
/// numeric literals, identifiers, module-relative offsets (`mod.exe+1234`)
/// and arithmetic expressions; tatu defers evaluation to symbol-resolution
/// time inside the executor / `asm` layer.
fn parse_define(args: &str) -> Result<Statement, ParseError> {
    let parts: Vec<&str> = args.splitn(2, ',').map(str::trim).collect();
    if parts.len() != 2 {
        return Err(ParseError::BadCall {
            fn_name: "define".into(),
            detail: format!("expected 2 args, got {}", parts.len()),
        });
    }
    if !is_identifier(parts[0]) {
        return Err(ParseError::BadCall {
            fn_name: "define".into(),
            detail: format!("invalid identifier {:?}", parts[0]),
        });
    }
    if parts[1].is_empty() {
        return Err(ParseError::BadCall {
            fn_name: "define".into(),
            detail: "empty value".into(),
        });
    }
    Ok(Statement::Define {
        name: parts[0].to_string(),
        value: parts[1].to_string(),
    })
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
mod tests;
