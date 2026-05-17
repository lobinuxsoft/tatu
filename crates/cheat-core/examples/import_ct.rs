// Convert a Cheat Engine .CT file into a CheatTable JSON config that
// cheat-core can load. Writes the JSON to stdout and a summary of
// imported / skipped entries to stderr.
//
// Usage:
//   cargo run --example import_ct -p cheat-core -- <input.CT> <app_id> <exe_pattern> [game_name]
//
// Example:
//   cargo run --example import_ct -p cheat-core -- \
//       ~/Downloads/EnderMagnolia.CT 2725260 EnderMagniolaSteam-Win64-Shipping.exe \
//       "ENDER MAGNOLIA" > ~/.config/backlog-tracker/cheats/2725260.json

use cheat_core::ct_import::{ImportedEntry, SkipReason, parse_ct};
use cheat_core::types::CheatTable;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 || args.len() > 5 {
        eprintln!(
            "Usage: {} <input.CT> <app_id> <exe_pattern> [game_name]",
            args[0]
        );
        return ExitCode::from(1);
    }
    let input_path = &args[1];
    let app_id: u64 = match args[2].parse() {
        Ok(n) => n,
        Err(_) => {
            eprintln!("invalid app_id '{}': expected u64", args[2]);
            return ExitCode::from(1);
        }
    };
    let exe_pattern = args[3].clone();
    let game_name = args.get(4).cloned().unwrap_or_else(|| exe_pattern.clone());

    let xml = match fs::read_to_string(input_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("could not read {input_path}: {e}");
            return ExitCode::from(1);
        }
    };

    let entries = match parse_ct(&xml) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("parse failed: {e}");
            return ExitCode::from(2);
        }
    };

    let mut cheats = Vec::new();
    let mut skipped_by_reason: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut skipped_details: Vec<(String, String)> = Vec::new();

    for entry in entries {
        match entry {
            ImportedEntry::Cheat(c) => cheats.push(c),
            ImportedEntry::Skipped {
                description,
                reason,
            } => {
                let (key, detail) = describe_skip(&reason);
                *skipped_by_reason.entry(key).or_insert(0) += 1;
                skipped_details.push((description, detail));
            }
        }
    }

    let table = CheatTable {
        app_id,
        game_name,
        exe_pattern,
        cheats,
    };

    let json = match serde_json::to_string_pretty(&table) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("serialize failed: {e}");
            return ExitCode::from(3);
        }
    };
    println!("{json}");

    eprintln!(
        "import: {} cheats produced, {} entries skipped",
        table.cheats.len(),
        skipped_details.len()
    );
    for (reason, count) in &skipped_by_reason {
        eprintln!("  skipped[{reason}] = {count}");
    }
    if !skipped_details.is_empty() && skipped_details.len() <= 20 {
        eprintln!("--- skipped detail ---");
        for (desc, detail) in skipped_details {
            eprintln!("  {desc}: {detail}");
        }
    }

    ExitCode::SUCCESS
}

fn describe_skip(reason: &SkipReason) -> (&'static str, String) {
    match reason {
        SkipReason::AssemblerScript => (
            "assembler-script",
            "needs AOB+codecave (out of scope)".into(),
        ),
        SkipReason::GroupingHeader => ("grouping-header", "folder/header entry, no address".into()),
        SkipReason::SymbolicAddress(name) => (
            "symbolic-address",
            format!("references symbol '{name}' (needs AOB script first)"),
        ),
        SkipReason::UnsupportedVariableType(t) => (
            "unsupported-variable-type",
            format!("variable type '{t}' not yet supported"),
        ),
        SkipReason::UnsupportedAddressForm(s) => (
            "unsupported-address-form",
            format!("address '{s}' not in any known form"),
        ),
    }
}
