//! Remote `mmap` / `munmap` inside a target Linux process.
//!
//! Ported from Cheat Engine: `ceserver/extensionloader.c:1042`
//! (`allocWithoutExtension`) for the ptrace dance and `ceserver/symbols.c:704`
//! (`FindSymbol`) for the on-disk ELF symbol lookup. The CE original carries
//! 32-bit / 64-bit / ARM / aarch64 conditionals — this port keeps only the
//! `__x86_64__` path because that is the only architecture this project ships
//! to (Bazzite on AMD64, Steam Deck on AMD64).
//!
//! Mechanics (x86_64 System V calling convention):
//! 1. `ptrace::attach` the target → triggers SIGSTOP on the attached thread.
//! 2. `getregs` → snapshot saved register state.
//! 3. Edit a fresh register set: `rip = mmap`, `rsp -= 0x40` aligned + 8 to
//!    emulate the `push` of the return address, write a sentinel return
//!    (`0xCE0`, copied verbatim from CE) at the new `rsp`, populate
//!    `rdi/rsi/rdx/rcx/r8/r9` with the mmap arguments.
//! 4. `setregs` → `cont(SIGCONT)` → `waitpid` until the target stops on the
//!    sentinel return (will fault with SIGSEGV because nothing is mapped at
//!    `0xCE0`). Sibling threads that stop on SIGSTOP / other signals during
//!    the wait are re-continued unhandled.
//! 5. `getregs` → result is in `rax`.
//! 6. Restore the saved register set, `detach` (resumes the target).
//!
//! Caveats accepted for Phase A:
//! - Only the attached thread is paused. Other threads in the target keep
//!   running. If another thread is inside `mmap` and holds the heap arena
//!   lock, our redirected call deadlocks. In practice this is rare during
//!   the moment a user clicks "enable cheat"; we accept the risk and add
//!   a timeout layer later if it surfaces.
//! - Sentinel return `0xCE0` matches CE for parity. Low memory is almost
//!   always unmapped on Linux because of `mmap_min_addr` (default 65536),
//!   so the SIGSEGV is reliable.
//! - `mmap` is found in the target's libc on disk (via `goblin`); a process
//!   that statically links libc has no `libc.so` mapping and will fail with
//!   `SymbolNotFound`. Real-world games invariably link glibc dynamically.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use goblin::elf::Elf;
use nix::libc::{MAP_ANONYMOUS, MAP_PRIVATE, PROT_EXEC, PROT_READ, PROT_WRITE, user_regs_struct};
use nix::sys::ptrace::{self, AddressType};
use nix::sys::signal::Signal;
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::Pid;

const SENTINEL_RETURN: u64 = 0x0ce0;
const PROT_RWX: i32 = PROT_READ | PROT_WRITE | PROT_EXEC;

#[derive(Debug, thiserror::Error)]
pub enum AllocError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("ptrace/errno: {0}")]
    Ptrace(#[from] nix::errno::Errno),
    #[error("symbol {symbol:?} not found in any module of pid {pid}")]
    SymbolNotFound { symbol: String, pid: i32 },
    #[error("elf parse error in {path}: {source}")]
    Elf {
        path: PathBuf,
        #[source]
        source: goblin::error::Error,
    },
    #[error("waitpid returned unexpected status: {0:?}")]
    UnexpectedWait(WaitStatus),
    #[error("remote mmap returned MAP_FAILED ({raw:#x}, errno {errno})")]
    RemoteMmapFailed { raw: u64, errno: i32 },
    #[error("remote munmap returned non-zero ({raw:#x}, errno {errno})")]
    RemoteMunmapFailed { raw: u64, errno: i32 },
}

/// Allocate `size` bytes of memory inside the target process.
///
/// `prot` defaults to `PROT_READ|PROT_WRITE|PROT_EXEC` so codecaves are
/// executable out of the box; pass `Some(prot)` to override. `hint` is the
/// first argument to `mmap` — pass `None` to let the kernel choose, or a
/// nearby address when patching is rip-relative.
pub fn alloc_remote(
    pid: Pid,
    size: usize,
    prot: Option<i32>,
    hint: Option<u64>,
) -> Result<u64, AllocError> {
    let mmap_addr = find_libc_symbol(pid, "mmap")?;
    let prot = prot.unwrap_or(PROT_RWX);
    let flags = MAP_PRIVATE | MAP_ANONYMOUS;

    attach_and_wait(pid)?;
    let original = ptrace::getregs(pid)?;

    let raw = (|| {
        let mut new = original;
        prepare_call_stack(pid, &mut new)?;
        new.rip = mmap_addr;
        new.rax = 0;
        new.rdi = hint.unwrap_or(0);
        new.rsi = size as u64;
        new.rdx = prot as u64;
        new.rcx = flags as u64;
        new.r8 = u64::MAX; // fd = -1; MAP_ANONYMOUS ignores it but -1 is canonical
        new.r9 = 0; // offset
        ptrace::setregs(pid, new)?;
        ptrace::cont(pid, Signal::SIGCONT)?;
        wait_for_sentinel_stop(pid)?;
        let after = ptrace::getregs(pid)?;
        Ok::<u64, AllocError>(after.rax)
    })();

    let _ = ptrace::setregs(pid, original);
    let _ = ptrace::detach(pid, None);
    let raw = raw?;
    interpret_mmap_result(raw)
}

/// Release a previously allocated region.
pub fn dealloc_remote(pid: Pid, addr: u64, size: usize) -> Result<(), AllocError> {
    let munmap_addr = find_libc_symbol(pid, "munmap")?;

    attach_and_wait(pid)?;
    let original = ptrace::getregs(pid)?;

    let raw = (|| {
        let mut new = original;
        prepare_call_stack(pid, &mut new)?;
        new.rip = munmap_addr;
        new.rax = 0;
        new.rdi = addr;
        new.rsi = size as u64;
        ptrace::setregs(pid, new)?;
        ptrace::cont(pid, Signal::SIGCONT)?;
        wait_for_sentinel_stop(pid)?;
        let after = ptrace::getregs(pid)?;
        Ok::<u64, AllocError>(after.rax)
    })();

    let _ = ptrace::setregs(pid, original);
    let _ = ptrace::detach(pid, None);
    let raw = raw?;
    interpret_munmap_result(raw)
}

fn attach_and_wait(pid: Pid) -> Result<(), AllocError> {
    ptrace::attach(pid)?;
    match waitpid(pid, None)? {
        WaitStatus::Stopped(_, Signal::SIGSTOP) => Ok(()),
        other => Err(AllocError::UnexpectedWait(other)),
    }
}

fn prepare_call_stack(pid: Pid, regs: &mut user_regs_struct) -> Result<(), AllocError> {
    // CE: rsp -= 0x40, align down to 16, OR 8 to emulate the implicit
    // `push <return>` that a normal `call` would do.
    regs.rsp = ((regs.rsp - 0x40) & !0xf) | 8;
    ptrace::write(pid, regs.rsp as AddressType, SENTINEL_RETURN as i64)?;
    Ok(())
}

fn wait_for_sentinel_stop(pid: Pid) -> Result<(), AllocError> {
    loop {
        match waitpid(pid, None)? {
            WaitStatus::Stopped(stopped_pid, sig) if stopped_pid == pid => match sig {
                Signal::SIGSEGV | Signal::SIGTRAP | Signal::SIGBUS => return Ok(()),
                other => {
                    // Unexpected signal on the traced thread: forward it so
                    // we don't swallow user-visible behaviour.
                    ptrace::cont(pid, other)?;
                }
            },
            WaitStatus::Stopped(other_pid, sig) => {
                // Sibling thread paused. Re-continue it without our intervention.
                let _ = ptrace::cont(other_pid, sig);
            }
            other => return Err(AllocError::UnexpectedWait(other)),
        }
    }
}

fn interpret_mmap_result(raw: u64) -> Result<u64, AllocError> {
    // MAP_FAILED is `(void*)-1`; in practice anything in the top page is an
    // error encoding `-errno`.
    if raw >= (!0u64).wrapping_sub(4095) {
        let errno = -(raw as i64) as i32;
        return Err(AllocError::RemoteMmapFailed { raw, errno });
    }
    Ok(raw)
}

fn interpret_munmap_result(raw: u64) -> Result<(), AllocError> {
    if raw == 0 {
        return Ok(());
    }
    let errno = -(raw as i64) as i32;
    Err(AllocError::RemoteMunmapFailed { raw, errno })
}

/// Resolve `name` in the target's libc on disk.
///
/// Walks `/proc/<pid>/maps`, picks the first module whose path contains
/// "libc", parses its on-disk ELF dynamic symbol table with `goblin`, and
/// returns the absolute address by adding the module's load base. Each
/// distinct on-disk path is parsed at most once — a multi-segment libc
/// shows up many times in `/proc/<pid>/maps` but the symbol table is the
/// same file.
pub fn find_libc_symbol(pid: Pid, name: &str) -> Result<u64, AllocError> {
    let maps = fs::read_to_string(format!("/proc/{}/maps", pid.as_raw()))?;
    let mut seen: HashSet<String> = HashSet::new();

    for line in maps.lines() {
        let Some(entry) = parse_maps_line(line) else {
            continue;
        };
        if !entry.path.contains("libc")
            || !(entry.path.contains(".so") || entry.path.ends_with(".so"))
        {
            continue;
        }
        if !seen.insert(entry.path.clone()) {
            continue;
        }

        let bytes = match fs::read(&entry.path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let elf = Elf::parse(&bytes).map_err(|source| AllocError::Elf {
            path: entry.path.clone().into(),
            source,
        })?;

        let load_base = entry.start.saturating_sub(entry.file_offset);
        for sym in elf.dynsyms.iter() {
            if sym.st_value == 0 || !sym.is_function() {
                continue;
            }
            let Some(sym_name) = elf.dynstrtab.get_at(sym.st_name) else {
                continue;
            };
            if sym_name == name {
                return Ok(load_base + sym.st_value);
            }
        }
    }

    Err(AllocError::SymbolNotFound {
        symbol: name.to_string(),
        pid: pid.as_raw(),
    })
}

struct MapsEntry {
    start: u64,
    file_offset: u64,
    path: String,
}

fn parse_maps_line(line: &str) -> Option<MapsEntry> {
    let mut parts = line.split_whitespace();
    let range = parts.next()?;
    let _perms = parts.next()?;
    let offset = parts.next()?;
    let _dev = parts.next()?;
    let _inode = parts.next()?;
    let path = parts.collect::<Vec<_>>().join(" ");
    if path.is_empty() || path.starts_with('[') {
        return None;
    }
    let start = u64::from_str_radix(range.split('-').next()?, 16).ok()?;
    let file_offset = u64::from_str_radix(offset, 16).ok()?;
    Some(MapsEntry {
        start,
        file_offset,
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_maps_line_extracts_libc() {
        let line = "7f1234567000-7f123456a000 r-xp 00000000 fd:01 12345  /usr/lib64/libc.so.6";
        let entry = parse_maps_line(line).expect("parse");
        assert_eq!(entry.start, 0x7f1234567000);
        assert_eq!(entry.file_offset, 0);
        assert_eq!(entry.path, "/usr/lib64/libc.so.6");
    }

    #[test]
    fn parse_maps_line_skips_anonymous_and_pseudo() {
        assert!(parse_maps_line("7f0000000000-7f0000001000 rw-p 00000000 00:00 0").is_none());
        assert!(
            parse_maps_line("7ffff7fdc000-7ffff7ffd000 r-xp 00000000 00:00 0  [vdso]").is_none()
        );
    }

    #[test]
    fn interpret_mmap_result_recognises_map_failed() {
        assert!(interpret_mmap_result(!0u64).is_err());
        assert!(interpret_mmap_result(!0u64 - 4095).is_err());
        assert!(interpret_mmap_result(0x7f1234567000).is_ok());
    }

    #[test]
    fn interpret_munmap_result_distinguishes_success() {
        assert!(interpret_munmap_result(0).is_ok());
        assert!(interpret_munmap_result(!0u64).is_err());
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn find_libc_symbol_locates_mmap_in_self() {
        let me = Pid::this();
        let addr = find_libc_symbol(me, "mmap").expect("mmap in self libc");
        // Sanity: the result should match libc::mmap's own address in our
        // address space (the test process is the target).
        let expected = nix::libc::mmap as *const () as u64;
        assert_eq!(addr, expected);
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn alloc_remote_then_dealloc_in_child() {
        use nix::sys::signal::kill;
        use nix::unistd::{ForkResult, fork, getpid};
        use std::time::Duration;

        match unsafe { fork() }.expect("fork") {
            ForkResult::Child => {
                // Sleep long enough for the parent to attach + run mmap/munmap.
                std::thread::sleep(Duration::from_secs(5));
                std::process::exit(0);
            }
            ForkResult::Parent { child } => {
                std::thread::sleep(Duration::from_millis(150));

                let addr = alloc_remote(child, 4096, None, None).expect("alloc");
                assert!(addr != 0);
                assert!(addr & 0xfff == 0, "mmap must return a page-aligned address");

                dealloc_remote(child, addr, 4096).expect("dealloc");

                // Cleanup.
                let _ = kill(child, Signal::SIGKILL);
                let _ = waitpid(child, None);
                // Suppress unused warning on getpid in non-debug paths.
                let _ = getpid();
            }
        }
    }
}
