//! Surgical text-VDF editing for Steam's `localconfig.vdf`.
//!
//! We deliberately do NOT round-trip the whole file through a parser:
//! `localconfig.vdf` holds a large amount of Steam state and a lossy
//! re-serialize would risk dropping fields. Instead we tokenize, locate the
//! byte span of exactly one value (or the insertion point for a missing one),
//! and splice text in — leaving every other byte untouched.
//!
//! VDF (KeyValues) is whitespace-insensitive when parsed, so imperfect
//! indentation in inserted blocks is cosmetic; Steam rewrites the file
//! cleanly on its next save. The only hard requirements are correct quoting/
//! escaping and brace balance.

use std::ops::Range;

/// The navigation path to the per-app settings inside `localconfig.vdf`.
pub const APPS_PATH: &[&str] = &["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"];

#[derive(Debug, PartialEq, Eq)]
pub enum VdfError {
    /// A key along the navigation path was missing (file isn't the expected
    /// `localconfig.vdf` shape).
    PathNotFound(String),
    /// Tokenizer hit an unterminated quoted string or unbalanced braces.
    Malformed(String),
}

impl std::fmt::Display for VdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VdfError::PathNotFound(k) => write!(f, "VDF path key not found: {k}"),
            VdfError::Malformed(m) => write!(f, "malformed VDF: {m}"),
        }
    }
}

impl std::error::Error for VdfError {}

/// A lexical token with byte offsets into the source.
enum Tok {
    /// Quoted string. `inner` is the byte range *between* the quotes (raw,
    /// still escaped).
    Str { inner: Range<usize> },
    /// `{` at this byte offset.
    Open(usize),
    /// `}` at this byte offset.
    Close(usize),
}

/// Read the current `LaunchOptions` value for `app_id`, if the app block and
/// the key both exist. Returns the logical (unescaped) value.
pub fn read_launch_options(src: &str, app_id: &str) -> Result<Option<String>, VdfError> {
    let toks = tokenize(src)?;
    let apps = navigate(&toks, src, APPS_PATH)?;
    let apps_inner = brace_token_range(&toks, &apps);
    let Some(app) = find_block(&toks, src, apps_inner, app_id) else {
        return Ok(None);
    };
    let app_inner = brace_token_range(&toks, &app);
    match find_value(&toks, src, app_inner, "LaunchOptions") {
        Some(v) => Ok(Some(unescape(&src[v]))),
        None => Ok(None),
    }
}

/// Return `src` with `app_id`'s `LaunchOptions` set to `value` (a logical,
/// unescaped string). Creates the app block and/or the key if absent. Only
/// the affected span changes; all other bytes are preserved.
pub fn set_launch_options(src: &str, app_id: &str, value: &str) -> Result<String, VdfError> {
    let toks = tokenize(src)?;
    let apps = navigate(&toks, src, APPS_PATH)?;
    let apps_inner = brace_token_range(&toks, &apps);
    let escaped = escape(value);

    let Some(app) = find_block(&toks, src, apps_inner, app_id) else {
        // App block absent: append a fresh one just before the `apps` close.
        let insert_at = apps.end;
        let block = format!(
            "\t\"{app_id}\"\n\t\t\t\t\t{{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"{escaped}\"\n\t\t\t\t\t}}\n\t\t\t\t"
        );
        return Ok(splice(src, insert_at..insert_at, &block));
    };

    let app_inner = brace_token_range(&toks, &app);
    if let Some(v) = find_value(&toks, src, app_inner, "LaunchOptions") {
        // Replace the existing value's inner bytes.
        Ok(splice(src, v, &escaped))
    } else {
        // App block exists but has no LaunchOptions: insert right after `{`.
        let insert_at = app.start + 1;
        let line = format!("\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"{escaped}\"");
        Ok(splice(src, insert_at..insert_at, &line))
    }
}

fn splice(src: &str, range: Range<usize>, with: &str) -> String {
    let mut out = String::with_capacity(src.len() + with.len());
    out.push_str(&src[..range.start]);
    out.push_str(with);
    out.push_str(&src[range.end..]);
    out
}

/// Walk the key path from the top level, returning the byte range
/// `open_brace..close_brace` of the final key's block, so callers can splice
/// relative to the braces (and derive the inner token range via
/// [`brace_token_range`]).
fn navigate(toks: &[Tok], src: &str, path: &[&str]) -> Result<Range<usize>, VdfError> {
    // `level` is the token index range to search at the current depth.
    let mut level = 0..toks.len();
    let mut block = 0..0;
    for key in path {
        block = find_block(toks, src, level.clone(), key)
            .ok_or_else(|| VdfError::PathNotFound((*key).to_string()))?;
        // Descend: search the tokens between this block's braces next.
        level = brace_token_range(toks, &block);
    }
    Ok(block)
}

/// Given a byte range `open..close`, return the token-index range strictly
/// inside those braces.
fn brace_token_range(toks: &[Tok], block: &Range<usize>) -> Range<usize> {
    let open_idx = toks
        .iter()
        .position(|t| matches!(t, Tok::Open(p) if *p == block.start))
        .expect("block.start is an Open we produced");
    let close_idx = toks
        .iter()
        .position(|t| matches!(t, Tok::Close(p) if *p == block.end))
        .expect("block.end is a Close we produced");
    (open_idx + 1)..close_idx
}

/// Find `key` at the given token level whose value is a block `{...}`.
/// Returns the byte range `open_brace_offset..close_brace_offset`.
/// Skips nested blocks so only direct children of the level match.
fn find_block(toks: &[Tok], src: &str, level: Range<usize>, key: &str) -> Option<Range<usize>> {
    let mut i = level.start;
    while i < level.end {
        match &toks[i] {
            Tok::Str { inner } if unescape(&src[inner.clone()]) == key => {
                // Value must be the next token and must be a block.
                if let Some(Tok::Open(open_pos)) = toks.get(i + 1) {
                    let close_idx = matching_close(toks, i + 1);
                    if let Tok::Close(close_pos) = toks[close_idx] {
                        return Some(*open_pos..close_pos);
                    }
                }
                // Key present but scalar — not what we want; skip its value.
                i += 2;
            }
            Tok::Str { .. } => {
                // Some other key: skip it and its value (scalar or block).
                i = skip_value(toks, i);
            }
            Tok::Open(_) => {
                // Stray block — jump past it.
                i = matching_close(toks, i) + 1;
            }
            Tok::Close(_) => break,
        }
    }
    None
}

/// Find `key` at the given level whose value is a scalar string. Returns the
/// byte range of the value's inner content (between its quotes).
fn find_value(toks: &[Tok], src: &str, level: Range<usize>, key: &str) -> Option<Range<usize>> {
    let mut i = level.start;
    while i < level.end {
        match &toks[i] {
            Tok::Str { inner } if unescape(&src[inner.clone()]) == key => {
                if let Some(Tok::Str { inner: val }) = toks.get(i + 1) {
                    return Some(val.clone());
                }
                i += 2;
            }
            Tok::Str { .. } => i = skip_value(toks, i),
            Tok::Open(_) => i = matching_close(toks, i) + 1,
            Tok::Close(_) => break,
        }
    }
    None
}

/// Index just past the value of the key at `key_idx` (skips a scalar value or
/// a whole nested block).
fn skip_value(toks: &[Tok], key_idx: usize) -> usize {
    match toks.get(key_idx + 1) {
        Some(Tok::Open(_)) => matching_close(toks, key_idx + 1) + 1,
        Some(_) => key_idx + 2,
        None => key_idx + 1,
    }
}

/// Given the index of an `Open`, return the index of its matching `Close`.
fn matching_close(toks: &[Tok], open_idx: usize) -> usize {
    let mut depth = 0usize;
    for (off, t) in toks[open_idx..].iter().enumerate() {
        match t {
            Tok::Open(_) => depth += 1,
            Tok::Close(_) => {
                depth -= 1;
                if depth == 0 {
                    return open_idx + off;
                }
            }
            Tok::Str { .. } => {}
        }
    }
    // Balanced input guaranteed by tokenize(); unreachable in practice.
    toks.len() - 1
}

/// Tokenize VDF into quoted strings and braces. Whitespace and `//` line
/// comments are ignored. Errors on unterminated strings / unbalanced braces.
fn tokenize(src: &str) -> Result<Vec<Tok>, VdfError> {
    let bytes = src.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0;
    let mut depth: i64 = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() {
                    match bytes[j] {
                        b'\\' => j += 2, // skip escaped char
                        b'"' => break,
                        _ => j += 1,
                    }
                }
                if j >= bytes.len() {
                    return Err(VdfError::Malformed("unterminated string".into()));
                }
                toks.push(Tok::Str { inner: start..j });
                i = j + 1;
            }
            b'{' => {
                toks.push(Tok::Open(i));
                depth += 1;
                i += 1;
            }
            b'}' => {
                toks.push(Tok::Close(i));
                depth -= 1;
                if depth < 0 {
                    return Err(VdfError::Malformed("unbalanced '}'".into()));
                }
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    if depth != 0 {
        return Err(VdfError::Malformed("unbalanced braces".into()));
    }
    Ok(toks)
}

/// Resolve VDF string escapes to the logical value.
fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Escape a logical value for embedding inside VDF quotes.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
#[path = "vdf_tests.rs"]
mod tests;
