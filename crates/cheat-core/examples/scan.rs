// Minimal memory scanner using cheat-core primitives.
//
// Usage:
//   cargo run --example scan -p cheat-core -- <pid|exe_pattern> <value> <type>
//
// Examples:
//   cargo run --example scan -p cheat-core -- EnderMagnolia.exe 100 u32
//   cargo run --example scan -p cheat-core -- 12345 1234.5 f32
//
// Iterate /proc/<pid>/maps for readable + writable regions, search for
// the value's little-endian byte pattern, print every match. Use
// repeated runs after the in-game value changes to narrow the address
// set mentally (the scanner is stateless).

use cheat_core::attach::find_process_by_exe;
use cheat_core::memory::read_bytes;
use procfs::process::{MMPermissions, MMapPath, Process};
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("Usage: {} <pid|exe_pattern> <value> <type>", args[0]);
        eprintln!("  type: u8 | u16 | u32 | u64 | i8 | i16 | i32 | i64 | f32 | f64");
        return ExitCode::from(1);
    }

    let target = &args[1];
    let value = &args[2];
    let ty = &args[3];

    let pid = match target.parse::<i32>() {
        Ok(n) => n,
        Err(_) => match find_process_by_exe(target) {
            Ok(p) => p.pid,
            Err(e) => {
                eprintln!("could not locate process '{target}': {e}");
                return ExitCode::from(1);
            }
        },
    };

    let needle = match encode_needle(value, ty) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("invalid value '{value}' for type '{ty}': {e}");
            return ExitCode::from(1);
        }
    };

    let proc = match Process::new(pid) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("could not open /proc/{pid}: {e}");
            return ExitCode::from(1);
        }
    };
    let maps = match proc.maps() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("could not read /proc/{pid}/maps: {e}");
            return ExitCode::from(1);
        }
    };

    let mut matches: Vec<(u64, String)> = Vec::new();
    let mut regions_scanned: u64 = 0;
    let mut bytes_scanned: u64 = 0;

    for map in maps {
        if !map
            .perms
            .contains(MMPermissions::READ | MMPermissions::WRITE)
        {
            continue;
        }

        let (start, end) = map.address;
        let size = end - start;
        if size == 0 || size > 1 << 30 {
            continue;
        }

        let chunk = match read_bytes(pid, start, size as usize) {
            Ok(b) => b,
            Err(_) => continue,
        };
        regions_scanned += 1;
        bytes_scanned += size;

        let label = label_of(&map.pathname);
        for offset in find_all(&chunk, &needle) {
            let addr = start + offset as u64;
            matches.push((addr, label.clone()));
            if matches.len() > 5000 {
                eprintln!(
                    "scanner: more than 5000 matches — narrow your search (change the in-game value and re-run)"
                );
                print_matches(&matches);
                return ExitCode::from(2);
            }
        }
    }

    eprintln!(
        "scanner: {} matches in {} regions ({} MB scanned)",
        matches.len(),
        regions_scanned,
        bytes_scanned >> 20
    );
    print_matches(&matches);

    ExitCode::SUCCESS
}

fn encode_needle(value: &str, ty: &str) -> Result<Vec<u8>, String> {
    let parse_err = |e: Box<dyn std::error::Error>| e.to_string();
    Ok(match ty {
        "u8" => value
            .parse::<u8>()
            .map_err(|e| parse_err(Box::new(e)))?
            .to_le_bytes()
            .to_vec(),
        "u16" => value
            .parse::<u16>()
            .map_err(|e| parse_err(Box::new(e)))?
            .to_le_bytes()
            .to_vec(),
        "u32" => value
            .parse::<u32>()
            .map_err(|e| parse_err(Box::new(e)))?
            .to_le_bytes()
            .to_vec(),
        "u64" => value
            .parse::<u64>()
            .map_err(|e| parse_err(Box::new(e)))?
            .to_le_bytes()
            .to_vec(),
        "i8" => value
            .parse::<i8>()
            .map_err(|e| parse_err(Box::new(e)))?
            .to_le_bytes()
            .to_vec(),
        "i16" => value
            .parse::<i16>()
            .map_err(|e| parse_err(Box::new(e)))?
            .to_le_bytes()
            .to_vec(),
        "i32" => value
            .parse::<i32>()
            .map_err(|e| parse_err(Box::new(e)))?
            .to_le_bytes()
            .to_vec(),
        "i64" => value
            .parse::<i64>()
            .map_err(|e| parse_err(Box::new(e)))?
            .to_le_bytes()
            .to_vec(),
        "f32" => value
            .parse::<f32>()
            .map_err(|e| parse_err(Box::new(e)))?
            .to_le_bytes()
            .to_vec(),
        "f64" => value
            .parse::<f64>()
            .map_err(|e| parse_err(Box::new(e)))?
            .to_le_bytes()
            .to_vec(),
        _ => return Err(format!("unknown type '{ty}'")),
    })
}

fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    let n = needle.len();
    if n == 0 || n > haystack.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let alignment = match n {
        1 => 1,
        2 => 2,
        4 => 4,
        8 => 8,
        _ => 1,
    };
    let mut i = 0;
    while i + n <= haystack.len() {
        if &haystack[i..i + n] == needle {
            out.push(i);
        }
        i += alignment;
    }
    out
}

fn label_of(path: &MMapPath) -> String {
    match path {
        MMapPath::Path(p) => p.to_string_lossy().into_owned(),
        MMapPath::Heap => "[heap]".into(),
        MMapPath::Stack => "[stack]".into(),
        MMapPath::TStack(_) => "[stack:thread]".into(),
        MMapPath::Anonymous => "[anon]".into(),
        MMapPath::Vsyscall => "[vsyscall]".into(),
        MMapPath::Vdso => "[vdso]".into(),
        MMapPath::Vvar => "[vvar]".into(),
        _ => "[other]".into(),
    }
}

fn print_matches(matches: &[(u64, String)]) {
    for (addr, region) in matches {
        println!("{addr:#018x}  {region}");
    }
}
