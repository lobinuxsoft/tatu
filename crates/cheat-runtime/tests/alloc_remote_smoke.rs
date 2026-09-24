//! Empirical smoke test for [`alloc_remote`] / [`dealloc_remote`] against a
//! live wine game process. Closes #130 once it passes on the user's box.
//!
//! Why this exists: every primitive in `cheat-runtime` after Phase A assumes
//! `alloc_remote` works (codecave allocation backs the AA executor, the
//! Aurora loader, persistent hooks). The function is ported 1:1 from CE's
//! `ceserver/extensionloader.c:1042` but was never validated against a real
//! Proton-launched x86_64 game until now. Unit tests in `alloc.rs` cover
//! the bit-twiddling helpers (mmap result decoding, sentinel return); this
//! file covers the syscall-driving ptrace dance end-to-end.
//!
//! How to run: launch the target wine game, then
//! ```
//! cargo test -p cheat-runtime --test alloc_remote_smoke -- --ignored --nocapture
//! ```
//! Override the target exe with `TATU_SMOKE_EXE=Other.exe`. Default is
//! `EnderMagnoliaSteam-Win64-Shipping.exe` because that is the game the user
//! has used to develop the rest of the runtime.
//!
//! Diagnostics on failure: each step prints what it tried and what it got,
//! plus a couple of environment probes (ptrace_scope, presence of libc in
//! `/proc/<pid>/maps`). The aim is that a red run tells you *which*
//! primitive is broken without re-running with extra logging.

use std::env;
use std::fs;

use cheat_runtime::{
    AllocError, alloc_remote, dealloc_remote, find_pid_by_exe, read_bytes, write_bytes,
};

const DEFAULT_EXE: &str = "EnderMagnoliaSteam-Win64-Shipping.exe";
const PATTERN: [u8; 8] = [0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE];
const ALLOC_SIZE: usize = 4096;

#[test]
#[ignore = "requires a live wine game + kernel.yama.ptrace_scope=0; run with --ignored"]
fn alloc_write_read_dealloc_against_live_game() {
    let exe = env::var("TATU_SMOKE_EXE").unwrap_or_else(|_| DEFAULT_EXE.to_string());
    println!("=== alloc_remote smoke ===");
    println!("target exe: {exe}");

    probe_ptrace_scope();

    let pid = find_pid_by_exe(&exe).unwrap_or_else(|| {
        panic!("no running process matching {exe:?}; launch the game and retry");
    });
    println!("found pid: {pid}");

    probe_libc_in_maps(pid.as_raw());

    // 1+2. alloc_remote → addr
    let addr = match alloc_remote(pid, ALLOC_SIZE, None, None) {
        Ok(a) => a,
        Err(e) => {
            panic!("alloc_remote failed: {e:?}\n{}", diagnose(&e, pid.as_raw()));
        }
    };
    println!("alloc_remote -> {addr:#x}");
    assert!(addr != 0, "mmap must not return NULL");
    assert!(
        addr & 0xfff == 0,
        "mmap must return a page-aligned address (got {addr:#x})"
    );

    // 3. write_bytes(4 KiB of the repeating PATTERN)
    let mut payload = Vec::with_capacity(ALLOC_SIZE);
    while payload.len() < ALLOC_SIZE {
        payload.extend_from_slice(&PATTERN);
    }
    write_bytes(pid, addr, &payload).expect("write_bytes into freshly mmap'd region");
    println!("write_bytes({ALLOC_SIZE} B) ok");

    // 4. read_bytes → pattern intacto
    let echoed = read_bytes(pid, addr, ALLOC_SIZE).expect("read_bytes back");
    assert_eq!(
        echoed, payload,
        "round-trip mismatch — buffer was clobbered or write didn't land"
    );
    println!("read_bytes round-trip ok ({ALLOC_SIZE} B identical)");

    // 5. dealloc_remote → libera
    dealloc_remote(pid, addr, ALLOC_SIZE).expect("dealloc_remote");
    println!("dealloc_remote ok");

    // 6. read post-dealloc debe fallar. process_vm_readv on an unmapped
    //    range yields EFAULT — surfaced as RuntimeError. We accept any
    //    error: the only thing that would be wrong is a *successful* read.
    match read_bytes(pid, addr, ALLOC_SIZE) {
        Ok(_) => panic!("read_bytes succeeded after dealloc — region was not actually freed"),
        Err(e) => println!("read_bytes post-dealloc failed as expected: {e}"),
    }

    println!("=== alloc_remote smoke PASS ===");
}

fn probe_ptrace_scope() {
    match fs::read_to_string("/proc/sys/kernel/yama/ptrace_scope") {
        Ok(s) => {
            let v = s.trim();
            println!("ptrace_scope = {v}");
            if v != "0" {
                eprintln!(
                    "WARN: ptrace_scope is {v}; need 0 (or CAP_SYS_PTRACE) for cross-process ptrace.\n\
                     fix:  echo 0 | sudo tee /proc/sys/kernel/yama/ptrace_scope"
                );
            }
        }
        Err(e) => println!("ptrace_scope probe failed: {e} (likely OK on systems without Yama)"),
    }
}

fn probe_libc_in_maps(pid: i32) {
    let path = format!("/proc/{pid}/maps");
    let Ok(maps) = fs::read_to_string(&path) else {
        eprintln!("could not read {path} — process gone?");
        return;
    };
    let libc_line = maps
        .lines()
        .find(|l| l.contains("libc.so") || l.contains("libc-"));
    match libc_line {
        Some(l) => println!("libc map: {l}"),
        None => eprintln!(
            "WARN: no libc.so in /proc/{pid}/maps — game statically links libc?\n\
             alloc_remote requires a dynamically loaded libc to resolve mmap/munmap symbols."
        ),
    }
}

fn diagnose(err: &AllocError, pid: i32) -> String {
    let mut out = String::new();
    out.push_str("diagnostics:\n");
    match err {
        AllocError::Symbol(_) => {
            out.push_str(
                "  symbol lookup failed: likely libc.so missing from the target maps,\n\
                 or goblin could not parse it. Inspect /proc/<pid>/maps manually.\n",
            );
        }
        AllocError::Ptrace(errno) => {
            out.push_str(&format!(
                "  ptrace errno: {errno:?}. Common causes:\n\
                   - ptrace_scope != 0 (see warn above)\n\
                   - target already traced by another debugger\n\
                   - target is a 32-bit wine process (this port is x86_64 only)\n"
            ));
        }
        AllocError::RemoteMmapFailed { raw, errno } => {
            out.push_str(&format!(
                "  remote mmap returned {raw:#x} (errno {errno}).\n\
                   - EPERM/EACCES: SELinux/AppArmor blocking mmap PROT_EXEC?\n\
                   - ENOMEM: address space exhausted, retry without MAP_32BIT.\n"
            ));
        }
        AllocError::RemoteMunmapFailed { raw, errno } => {
            out.push_str(&format!(
                "  remote munmap returned {raw:#x} (errno {errno}).\n\
                   - EINVAL: addr/size not page-aligned (shouldn't happen here).\n"
            ));
        }
        AllocError::UnexpectedWait(s) => {
            out.push_str(&format!(
                "  waitpid returned {s:?} mid-syscall. The target may have crashed\n\
                 or another tracer interfered. Re-launch the game.\n"
            ));
        }
    }
    out.push_str(&format!("  pid was {pid}\n"));
    out
}
