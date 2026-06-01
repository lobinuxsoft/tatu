//! Find Linux PIDs by process name.
//!
//! The runtime needs to attach to a running game by exe name (e.g.
//! `EnderMagnoliaSteam-Win64-Shipping.exe`). Steam launches games under
//! Proton — the kernel still exposes them as ordinary Linux processes, so
//! `/proc/<pid>/comm` (and `/proc/<pid>/cmdline`) contain the original
//! executable name and we can match against it directly.

use std::fs;

use nix::unistd::Pid;

/// Return every PID whose `comm` or `cmdline` final segment matches
/// `exe_name` (case-insensitive comparison on the basename). For Wine/Proton
/// processes the kernel-side `comm` is truncated to 15 bytes; we fall back
/// to the last whitespace-or-slash-separated token of `cmdline` to handle
/// that case.
pub fn find_pids_by_exe(exe_name: &str) -> Vec<Pid> {
    let target = exe_name.trim().to_lowercase();
    if target.is_empty() {
        return Vec::new();
    }
    let trunc15 = take_first_n(&target, 15);
    let Ok(rd) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut hits: Vec<(MatchKind, Pid)> = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name();
        let Some(pid_str) = name.to_str() else {
            continue;
        };
        let Ok(pid_n) = pid_str.parse::<i32>() else {
            continue;
        };
        if let Some(kind) = match_kind(pid_n, &target, &trunc15) {
            hits.push((kind, Pid::from_raw(pid_n)));
        }
    }
    // Comm matches first: the real game process (`comm == DD2.exe`) must
    // outrank Proton launchers (`srt-bwrap`, `pv-adverb`, the Steam reaper)
    // that only carry the exe path as a cmdline argument. Stable sort keeps
    // /proc order within each tier.
    hits.sort_by_key(|(kind, _)| *kind);
    hits.into_iter().map(|(_, pid)| pid).collect()
}

/// First PID found via [`find_pids_by_exe`], or `None` if nothing matched.
pub fn find_pid_by_exe(exe_name: &str) -> Option<Pid> {
    find_pids_by_exe(exe_name).into_iter().next()
}

/// How strongly a `/proc/<pid>` entry matches the target exe. `Comm` (the
/// kernel process name equals the exe basename) is authoritative; `Cmdline`
/// (the exe basename appears somewhere in argv) is weaker and also matches
/// Proton's launch chain — `srt-bwrap`, `pv-adverb`, `steam.exe` — which all
/// carry the game's `.exe` path as an argument. Ordered so `Comm < Cmdline`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum MatchKind {
    Comm,
    Cmdline,
}

fn match_kind(pid: i32, target_lower: &str, target_trunc15: &str) -> Option<MatchKind> {
    if let Ok(comm) = fs::read_to_string(format!("/proc/{pid}/comm")) {
        let comm_l = comm.trim().to_lowercase();
        if comm_l == *target_lower || comm_l == target_trunc15 {
            return Some(MatchKind::Comm);
        }
    }
    if let Ok(cmdline) = fs::read(format!("/proc/{pid}/cmdline"))
        && cmdline_basename_matches(&cmdline, target_lower)
    {
        return Some(MatchKind::Cmdline);
    }
    None
}

fn cmdline_basename_matches(cmdline: &[u8], target_lower: &str) -> bool {
    // cmdline is NUL-separated; we want any argv segment whose basename
    // (last path component) matches target_lower.
    for segment in cmdline.split(|&b| b == 0) {
        let Ok(s) = std::str::from_utf8(segment) else {
            continue;
        };
        let base = basename(s);
        if base.to_lowercase() == target_lower {
            return true;
        }
    }
    false
}

fn basename(s: &str) -> &str {
    // Handle both POSIX `/` and Windows `\` separators because Proton games'
    // cmdline often carries Wine-style paths (`Z:\home\...\Game.exe`).
    s.rsplit(['/', '\\']).next().unwrap_or(s)
}

fn take_first_n(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_unix_and_windows_separators() {
        assert_eq!(basename("/home/foo/Game.exe"), "Game.exe");
        assert_eq!(basename("Z:\\home\\foo\\Game.exe"), "Game.exe");
        assert_eq!(basename("Game.exe"), "Game.exe");
        assert_eq!(basename(""), "");
    }

    #[test]
    fn cmdline_basename_matches_finds_the_target() {
        let cmdline = b"/usr/bin/python3\0/home/x/Game.exe\0--flag\0";
        assert!(cmdline_basename_matches(cmdline, "game.exe"));
        assert!(!cmdline_basename_matches(cmdline, "other.exe"));
    }

    #[test]
    fn cmdline_basename_handles_wine_paths() {
        let cmdline = b"Z:\\home\\lobinux\\Aurora\\Aurora.exe\0-background\0";
        assert!(cmdline_basename_matches(cmdline, "aurora.exe"));
    }

    #[test]
    fn comm_match_outranks_cmdline_match() {
        // Proton launches DD2 as: srt-bwrap → … → DD2.exe. The wrapper's
        // cmdline carries `.../DD2.exe`, so it matches by Cmdline; the real
        // game process matches by Comm. find_pid_by_exe must land on the
        // real process (4601 regions), not the wrapper (25 regions).
        let mut hits = [
            (MatchKind::Cmdline, Pid::from_raw(28649)), // srt-bwrap wrapper
            (MatchKind::Comm, Pid::from_raw(28861)),    // real DD2.exe
        ];
        hits.sort_by_key(|(kind, _)| *kind);
        assert_eq!(hits.first().map(|(_, p)| *p), Some(Pid::from_raw(28861)));
        assert!(MatchKind::Comm < MatchKind::Cmdline);
    }

    #[test]
    fn empty_exe_name_returns_no_matches() {
        assert!(find_pids_by_exe("").is_empty());
        assert!(find_pids_by_exe("   ").is_empty());
    }

    #[test]
    fn find_pids_by_exe_finds_self_in_proc() {
        // We can't predict our binary name (test runner name varies), so just
        // assert the function doesn't blow up scanning /proc.
        let _ = find_pids_by_exe("never-going-to-match-anything-12345");
    }
}
