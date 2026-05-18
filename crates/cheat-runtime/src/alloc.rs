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

use nix::libc::{
    MAP_32BIT, MAP_ANONYMOUS, MAP_FIXED_NOREPLACE, MAP_PRIVATE, PROT_EXEC, PROT_READ, PROT_WRITE,
    user_regs_struct,
};
use nix::sys::ptrace::{self, AddressType};
use nix::sys::signal::Signal;
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::Pid;

use crate::elfsym::{self, ElfSymError};

const SENTINEL_RETURN: u64 = 0x0ce0;
const PROT_RWX: i32 = PROT_READ | PROT_WRITE | PROT_EXEC;

#[derive(Debug, thiserror::Error)]
pub enum AllocError {
    #[error("ptrace/errno: {0}")]
    Ptrace(#[from] nix::errno::Errno),
    #[error("symbol lookup: {0}")]
    Symbol(#[from] ElfSymError),
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
    let mmap_addr = elfsym::find_libc_symbol(pid, "mmap")?;
    let prot = prot.unwrap_or(PROT_RWX);
    // Two distinct address-placement requirements:
    //
    // - `hint == None`: caller wants any address but mov [imm], reg has
    //   to encode in disp32 — force MAP_32BIT (first 2 GiB).
    // - `hint == Some(near)`: caller has a trampoline at `near` and needs
    //   the codecave within ±2 GiB so a 5-byte `jmp rel32` fits. mmap
    //   without MAP_FIXED treats the hint as advisory and routinely
    //   returns high-half addresses, breaking the trampoline. Use
    //   MAP_FIXED_NOREPLACE and scan upward in 64 KiB strides until we
    //   land in a free page within range — this is the same dance CE
    //   does with VirtualAllocEx loops on Windows.
    if let Some(near) = hint {
        return alloc_near(pid, mmap_addr, size, prot, near);
    }
    let flags = MAP_PRIVATE | MAP_ANONYMOUS | MAP_32BIT;

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

/// Allocate a region within ±2 GiB of `near` so a 5-byte `jmp rel32` from
/// the trampoline site can reach it. Uses `MAP_FIXED_NOREPLACE` and walks
/// page-aligned candidates outward — first above `near`, then below — until
/// a free slot is found.
///
/// CE-AA scripts assume this placement (`alloc(newmem, $1000, pBase)`) but
/// Linux's `mmap` without `MAP_FIXED_NOREPLACE` treats the hint as advisory
/// and routinely places the codecave in the high-half, which forces
/// iced-x86 to fall back to a 14-byte `jmp [rip+disp32]` indirect — that
/// overflows the 10 bytes the AOB site has reserved for the trampoline and
/// stomps the next instruction (the EM master toggle ran into this and
/// crashed the game).
fn alloc_near(
    pid: Pid,
    mmap_addr: u64,
    size: usize,
    prot: i32,
    near: u64,
) -> Result<u64, AllocError> {
    // mmap-aligned page size; conservative 64 KiB stride (matches Windows
    // VirtualAlloc granularity and stays safely above any reasonable page
    // size — 4 KiB on x86_64 today).
    const PAGE_STRIDE: u64 = 0x1000;
    // ±1.75 GiB scan window — enough to land within the 2 GiB `jmp rel32`
    // reach while leaving slack for the resulting allocation size.
    const SCAN_WINDOW: u64 = 0x7000_0000;
    const FLAGS: i32 = MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE;

    attach_and_wait(pid)?;
    let original = ptrace::getregs(pid)?;

    let result = (|| -> Result<u64, AllocError> {
        // Try a sequence of candidate addresses: near, near + stride,
        // near - stride, near + 2*stride, near - 2*stride, … Each attempt
        // costs one remote mmap syscall, which is cheap (~µs) compared to
        // the seconds-long AOB scan that just ran.
        let near_page = near & !(PAGE_STRIDE - 1);
        let mut up = near_page;
        let mut down = near_page;
        for _step in 0..(SCAN_WINDOW / PAGE_STRIDE) {
            for &candidate in &[up, down] {
                let raw = remote_mmap(pid, mmap_addr, &original, candidate, size, prot, FLAGS)?;
                // `interpret_mmap_result` errors on MAP_FAILED; with
                // MAP_FIXED_NOREPLACE that includes EEXIST when the page
                // is already mapped — retry the next candidate.
                match interpret_mmap_result(raw) {
                    Ok(addr) => return Ok(addr),
                    Err(AllocError::RemoteMmapFailed { .. }) => continue,
                    Err(e) => return Err(e),
                }
            }
            up = up.saturating_add(PAGE_STRIDE);
            down = down.saturating_sub(PAGE_STRIDE);
        }
        Err(AllocError::RemoteMmapFailed {
            raw: u64::MAX,
            errno: nix::libc::ENOMEM,
        })
    })();

    let _ = ptrace::setregs(pid, original);
    let _ = ptrace::detach(pid, None);
    result
}

/// Inner helper: one remote `mmap` syscall with a fully prepared register
/// frame. Used only by [`alloc_near`] — the bare `alloc_remote` path
/// inlines this because it owns the attach/detach lifecycle differently
/// (one syscall per call).
fn remote_mmap(
    pid: Pid,
    mmap_addr: u64,
    original: &user_regs_struct,
    hint: u64,
    size: usize,
    prot: i32,
    flags: i32,
) -> Result<u64, AllocError> {
    let mut new = *original;
    prepare_call_stack(pid, &mut new)?;
    new.rip = mmap_addr;
    new.rax = 0;
    new.rdi = hint;
    new.rsi = size as u64;
    new.rdx = prot as u64;
    new.rcx = flags as u64;
    new.r8 = u64::MAX;
    new.r9 = 0;
    ptrace::setregs(pid, new)?;
    ptrace::cont(pid, Signal::SIGCONT)?;
    wait_for_sentinel_stop(pid)?;
    let after = ptrace::getregs(pid)?;
    Ok(after.rax)
}

/// Release a previously allocated region.
pub fn dealloc_remote(pid: Pid, addr: u64, size: usize) -> Result<(), AllocError> {
    let munmap_addr = elfsym::find_libc_symbol(pid, "munmap")?;

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

// `find_libc_symbol` moved to `crate::elfsym`; the rest of this module just
// uses it via `elfsym::find_libc_symbol` in `alloc_remote` / `dealloc_remote`.

#[cfg(test)]
mod tests {
    use super::*;

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

    // `find_libc_symbol_locates_mmap_in_self` moved to `crate::elfsym` along
    // with the function itself.

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
