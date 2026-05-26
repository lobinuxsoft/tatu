//! Audit driver for issue #131.
//!
//! Walks a directory of `.ct` (Cheat Engine table) XML files, extracts every
//! `<AssemblerScript>` body, parses it with `tatu_engine::parser`, and tallies
//! which AA commands appear and how often. The output is a frequency table
//! that drives priority for the parser+executor extension work.
//!
//! Run:
//! ```
//! cargo run -p tatu-engine --example ct_audit -- /tmp/ct-audit
//! ```
//!
//! Output goes to stdout: per-file summary, then a global frequency table of
//! every `name(...)` call seen + every `{$directive}` seen, sorted by count,
//! split into supported (parser already handles) vs unrecognised. The
//! "unrecognised" column is the work list for the parser extension.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

use tatu_engine::parser::{Statement, parse};

const SUPPORTED_CALLS: &[&str] = &[
    "aobscanmodule",
    "aobscan",
    "registersymbol",
    "unregistersymbol",
    "label",
    "alloc",
    "globalalloc",
    "dealloc",
    "define",
];

fn main() {
    let dir = env::args().nth(1).unwrap_or_else(|| "/tmp/ct-audit".into());
    let dir = Path::new(&dir);

    let mut scripts_total = 0usize;
    let mut scripts_parsed_ok = 0usize;
    let mut scripts_parse_err = 0usize;
    let mut lua_only_count = 0usize;
    let mut executable_count = 0usize;
    let mut files_seen = 0usize;

    let mut call_freq: HashMap<String, usize> = HashMap::new();
    let mut directive_freq: HashMap<String, usize> = HashMap::new();
    let mut per_file_summary: Vec<(String, usize, usize, usize)> = Vec::new();
    let mut error_kind_freq: HashMap<String, usize> = HashMap::new();

    let entries = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            eprintln!("could not read {dir:?}: {e}");
            std::process::exit(1);
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("ct") {
            continue;
        }
        files_seen += 1;
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("skip {path:?}: {e}");
                continue;
            }
        };
        let xml = String::from_utf8_lossy(&bytes);
        let scripts = extract_scripts(&xml);
        let file_scripts = scripts.len();
        let mut file_ok = 0usize;
        let mut file_err = 0usize;

        for script in scripts {
            scripts_total += 1;
            match parse(&script) {
                Ok(parsed) => {
                    scripts_parsed_ok += 1;
                    file_ok += 1;
                    if parsed.lua_only {
                        lua_only_count += 1;
                    } else {
                        executable_count += 1;
                    }
                    tally(&parsed.enable, &mut call_freq, &mut directive_freq);
                    tally(&parsed.disable, &mut call_freq, &mut directive_freq);
                }
                Err(e) => {
                    scripts_parse_err += 1;
                    file_err += 1;
                    let bucket = match &e {
                        tatu_engine::parser::ParseError::MissingEnable => "MissingEnable".into(),
                        tatu_engine::parser::ParseError::BadCall { fn_name, .. } => {
                            format!("BadCall({fn_name})")
                        }
                    };
                    *error_kind_freq.entry(bucket).or_insert(0) += 1;
                }
            }
        }

        per_file_summary.push((
            path.file_name().unwrap().to_string_lossy().into_owned(),
            file_scripts,
            file_ok,
            file_err,
        ));
    }

    println!("=== ct_audit ===");
    println!("dir: {}", dir.display());
    println!(
        "{files_seen} files, {scripts_total} scripts, parsed_ok={scripts_parsed_ok}, parse_err={scripts_parse_err}"
    );
    println!(
        "  of parsed: executable={executable_count}, lua_only={lua_only_count} (executor skips with warning)\n"
    );

    println!("--- per-file ---");
    per_file_summary.sort_by(|a, b| b.1.cmp(&a.1));
    for (name, total, ok, err) in &per_file_summary {
        println!("  {name:<40}  scripts={total:>4}  ok={ok:>4}  err={err:>3}");
    }

    println!("\n--- function-call frequency ---");
    let mut calls: Vec<_> = call_freq.iter().collect();
    calls.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    let (supported, unrecognised): (Vec<_>, Vec<_>) = calls
        .into_iter()
        .partition(|(name, _)| SUPPORTED_CALLS.contains(&name.as_str()));
    println!("  supported (already in parser):");
    for (name, count) in &supported {
        println!("    {name:<24} {count}");
    }
    println!("  unrecognised (work list):");
    for (name, count) in &unrecognised {
        println!("    {name:<24} {count}");
    }

    println!("\n--- compiler directives ---");
    let mut dirs: Vec<_> = directive_freq.iter().collect();
    dirs.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    for (name, count) in &dirs {
        println!("  {name:<30} {count}");
    }

    println!("\n--- parse-error kinds ---");
    let mut errs: Vec<_> = error_kind_freq.iter().collect();
    errs.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    for (kind, count) in &errs {
        println!("  {kind:<40} {count}");
    }
}

fn tally(
    stmts: &[Statement],
    calls: &mut HashMap<String, usize>,
    directives: &mut HashMap<String, usize>,
) {
    for stmt in stmts {
        match stmt {
            Statement::AobScanModule { .. } => {
                *calls.entry("aobscanmodule".into()).or_insert(0) += 1
            }
            Statement::AobScan { .. } => *calls.entry("aobscan".into()).or_insert(0) += 1,
            Statement::RegisterSymbol(_) => *calls.entry("registersymbol".into()).or_insert(0) += 1,
            Statement::UnregisterSymbol(_) => {
                *calls.entry("unregistersymbol".into()).or_insert(0) += 1
            }
            Statement::Label(_) => *calls.entry("label".into()).or_insert(0) += 1,
            Statement::Alloc { .. } => *calls.entry("alloc".into()).or_insert(0) += 1,
            Statement::GlobalAlloc { .. } => *calls.entry("globalalloc".into()).or_insert(0) += 1,
            Statement::Dealloc(_) => *calls.entry("dealloc".into()).or_insert(0) += 1,
            Statement::Define { .. } => *calls.entry("define".into()).or_insert(0) += 1,
            Statement::Directive(d) => *directives.entry(d.clone()).or_insert(0) += 1,
            Statement::Raw(line) => {
                if let Some(name) = call_name(line) {
                    *calls.entry(name).or_insert(0) += 1;
                }
            }
            Statement::LabelSite(_) | Statement::AbsoluteSite(_) => {}
        }
    }
}

fn call_name(line: &str) -> Option<String> {
    let l = line.trim();
    let open = l.find('(')?;
    if !l.ends_with(')') {
        return None;
    }
    let name = l[..open].trim();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some(name.to_lowercase())
}

fn extract_scripts(xml: &str) -> Vec<String> {
    use roxmltree::Document;
    let mut out = Vec::new();
    let doc = match Document::parse(xml) {
        Ok(d) => d,
        Err(_) => return out,
    };
    for node in doc
        .descendants()
        .filter(|n| n.has_tag_name("AssemblerScript"))
    {
        if let Some(text) = node.text() {
            out.push(text.to_string());
        }
    }
    out
}
