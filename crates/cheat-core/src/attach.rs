use procfs::process::{MMapPath, Process};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct AttachedProcess {
    pub pid: i32,
    pub modules: HashMap<String, ModuleRange>,
}

#[derive(Debug, Clone)]
pub struct ModuleRange {
    pub base: u64,
    pub end: u64,
    pub path: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AttachError {
    #[error("no process found matching pattern '{0}'")]
    NotFound(String),
    #[error("procfs error: {0}")]
    Procfs(#[from] procfs::ProcError),
}

pub fn find_process_by_exe(pattern: &str) -> Result<AttachedProcess, AttachError> {
    for entry in procfs::process::all_processes()? {
        let proc = match entry {
            Ok(p) => p,
            Err(_) => continue,
        };

        if !process_matches(&proc, pattern) {
            continue;
        }

        let pid = proc.pid;
        let modules = parse_modules(pid)?;
        return Ok(AttachedProcess { pid, modules });
    }

    Err(AttachError::NotFound(pattern.to_string()))
}

pub fn parse_modules(pid: i32) -> Result<HashMap<String, ModuleRange>, AttachError> {
    let proc = Process::new(pid)?;
    let maps = proc.maps()?;
    let mut by_basename: HashMap<String, ModuleRange> = HashMap::new();

    for map in maps {
        let path = match &map.pathname {
            MMapPath::Path(p) => p.clone(),
            _ => continue,
        };

        let basename = match path.file_name().and_then(|n| n.to_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };

        let (start, end) = map.address;

        by_basename
            .entry(basename)
            .and_modify(|range| {
                if start < range.base {
                    range.base = start;
                }
                if end > range.end {
                    range.end = end;
                }
            })
            .or_insert(ModuleRange {
                base: start,
                end,
                path: path.to_string_lossy().into_owned(),
            });
    }

    Ok(by_basename)
}

fn process_matches(proc: &Process, pattern: &str) -> bool {
    if let Ok(cmdline) = proc.cmdline() {
        if cmdline.iter().any(|arg| arg.contains(pattern)) {
            return true;
        }
    }

    if let Ok(exe) = proc.exe() {
        if exe.to_string_lossy().contains(pattern) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_modules_finds_libc_in_own_process() {
        let pid = std::process::id() as i32;
        let modules = parse_modules(pid).expect("parse modules");

        let has_libc = modules.keys().any(|k| k.starts_with("libc"));
        assert!(
            has_libc,
            "expected a libc-prefixed module in own process, got: {:?}",
            modules.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn parse_modules_returns_valid_address_ranges() {
        let pid = std::process::id() as i32;
        let modules = parse_modules(pid).expect("parse modules");

        for (name, range) in &modules {
            assert!(
                range.base < range.end,
                "module {name} has invalid range: base={:#x} end={:#x}",
                range.base,
                range.end
            );
        }
    }

    #[test]
    fn find_process_returns_not_found_for_unknown_pattern() {
        let result = find_process_by_exe("definitely-not-a-real-process-xyzzy-12345");
        assert!(matches!(result, Err(AttachError::NotFound(_))));
    }
}
