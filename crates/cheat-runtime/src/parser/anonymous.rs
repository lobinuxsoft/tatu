//! Resolution of CE Auto-Assembler anonymous-label tokens
//! (`@@:` / `@f` / `@b`) into the real label names they refer to.

use super::{ParseError, Statement};

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
pub(super) fn resolve_anonymous_refs(
    mut stmts: Vec<Statement>,
) -> Result<Vec<Statement>, ParseError> {
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

pub(super) fn contains_anon_token(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    has_word_token(&lower, "@f") || has_word_token(&lower, "@b")
}

pub(super) fn rewrite_anonymous_tokens(
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
pub(super) fn has_word_token(haystack: &str, needle: &str) -> bool {
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

pub(super) fn replace_word_token(haystack: &str, needle: &str, replacement: &str) -> String {
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

pub(super) fn is_token_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}
