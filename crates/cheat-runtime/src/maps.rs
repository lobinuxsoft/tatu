//! Parser for `/proc/<pid>/maps`. Produces a flat list of `MemoryRegion`
//! entries (start, end, perms, file offset, backing path) describing every
//! VMA the kernel exposes for a process. Used by the scanner to enumerate
//! the executable + writable ranges of a target module.

use std::fs;
use std::io;
use std::path::PathBuf;

use nix::unistd::Pid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRegion {
    pub start: u64,
    pub end: u64,
    pub perms: Perms,
    pub offset: u64,
    pub path: PathBuf,
}

impl MemoryRegion {
    pub fn size(&self) -> u64 {
        self.end - self.start
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Perms {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub shared: bool,
}

impl Perms {
    fn parse(s: &str) -> Self {
        let b = s.as_bytes();
        Self {
            read: b.first().is_some_and(|&c| c == b'r'),
            write: b.get(1).is_some_and(|&c| c == b'w'),
            execute: b.get(2).is_some_and(|&c| c == b'x'),
            shared: b.get(3).is_some_and(|&c| c == b's'),
        }
    }
}

pub fn read_maps(pid: Pid) -> io::Result<Vec<MemoryRegion>> {
    let path = format!("/proc/{}/maps", pid.as_raw());
    let text = fs::read_to_string(&path)?;
    Ok(parse_maps(&text))
}

pub fn parse_maps(text: &str) -> Vec<MemoryRegion> {
    text.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<MemoryRegion> {
    // Format documented in Documentation/filesystems/proc.rst:
    //   ADDR-ADDR PERMS OFFSET DEV INODE PATHNAME
    //   7f0d4f200000-7f0d4f201000 r-xp 00001000 fd:00 1234567 /usr/lib/libfoo.so
    //   555555555000-555555556000 rw-p 00000000 00:00 0       [heap]
    //   7ffe...                   rw-p 00000000 00:00 0
    let mut iter = line.split_whitespace();
    let addr = iter.next()?;
    let perms = iter.next()?;
    let offset = iter.next()?;
    let _dev = iter.next()?;
    let _inode = iter.next()?;
    let path = iter.collect::<Vec<_>>().join(" ");

    let (start_s, end_s) = addr.split_once('-')?;
    let start = u64::from_str_radix(start_s, 16).ok()?;
    let end = u64::from_str_radix(end_s, 16).ok()?;
    let offset = u64::from_str_radix(offset, 16).ok()?;

    Some(MemoryRegion {
        start,
        end,
        perms: Perms::parse(perms),
        offset,
        path: PathBuf::from(path),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_typical_so_line() {
        let line = "7f0d4f200000-7f0d4f201000 r-xp 00001000 fd:00 1234567 /usr/lib/libfoo.so";
        let r = parse_line(line).unwrap();
        assert_eq!(r.start, 0x7f0d4f200000);
        assert_eq!(r.end, 0x7f0d4f201000);
        assert_eq!(r.size(), 0x1000);
        assert_eq!(r.offset, 0x1000);
        assert_eq!(
            r.perms,
            Perms {
                read: true,
                write: false,
                execute: true,
                shared: false
            }
        );
        assert_eq!(r.path, PathBuf::from("/usr/lib/libfoo.so"));
    }

    #[test]
    fn parse_heap_pseudo_path() {
        let line = "555555555000-555555556000 rw-p 00000000 00:00 0 [heap]";
        let r = parse_line(line).unwrap();
        assert!(r.perms.read && r.perms.write && !r.perms.execute);
        assert_eq!(r.path, PathBuf::from("[heap]"));
    }

    #[test]
    fn parse_anon_no_path() {
        let line = "7ffe12340000-7ffe12350000 rw-p 00000000 00:00 0";
        let r = parse_line(line).unwrap();
        assert_eq!(r.path, PathBuf::from(""));
        assert!(r.perms.read && r.perms.write);
    }

    #[test]
    fn parse_shared_mapping() {
        let line = "7f0000000000-7f0000001000 rw-s 00000000 fd:00 42 /dev/shm/foo";
        let r = parse_line(line).unwrap();
        assert!(r.perms.shared);
        assert!(!r.perms.execute);
    }

    #[test]
    fn parse_path_with_space() {
        let line = "7f0000000000-7f0000001000 r--p 00000000 fd:00 42 /tmp/foo bar.so";
        let r = parse_line(line).unwrap();
        assert_eq!(r.path, PathBuf::from("/tmp/foo bar.so"));
    }

    #[test]
    fn parse_maps_skips_malformed_lines() {
        let text = "\
7f0000000000-7f0000001000 r-xp 00000000 fd:00 1 /a.so
garbage line that should be skipped
7f0000002000-7f0000003000 rw-p 00000000 fd:00 2 /b.so
";
        let regions = parse_maps(text);
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].path, PathBuf::from("/a.so"));
        assert_eq!(regions[1].path, PathBuf::from("/b.so"));
    }

    #[test]
    fn read_maps_of_self_returns_at_least_text_segment() {
        let pid = Pid::this();
        let regions = read_maps(pid).expect("/proc/self/maps must be readable");
        assert!(!regions.is_empty());
        // Our own binary must be among the executable regions.
        let has_exec = regions.iter().any(|r| r.perms.execute && r.size() > 0);
        assert!(has_exec, "expected at least one executable region");
    }

    #[test]
    fn perms_parse_handles_short_string() {
        assert_eq!(Perms::parse(""), Perms::default());
        let r = Perms::parse("r");
        assert!(r.read && !r.write);
    }
}
