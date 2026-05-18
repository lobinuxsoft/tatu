//! Inject a shared object into a target Linux process via ptrace + dlopen.
//!
//! Ported from CE `ceserver/extensionloader.c:392` (`loadExtension`),
//! keeping only the `__x86_64__` path (the only architecture this project
//! ships to). The Pascal+C original carries ARM / aarch64 / i386 branches
//! plus an "alloc on the stack" path that scales the path buffer up the
//! target's stack; this port allocates a fresh page through [`crate::alloc`]
//! instead. The tradeoff is one extra ptrace mmap round-trip for the path
//! page; the upside is no risk of clobbering the target's stack data when
//! the path is unexpectedly long.
//!
//! Pipeline:
//!
//! 1. Find `dlopen` in the target's loaded libc (Phase D's
//!    [`crate::elfsym::find_libc_symbol`]). Modern glibc (>= 2.34)
//!    exports `dlopen` directly from libc.so.6; older glibc has it in
//!    libdl.so.2. We try `dlopen` first, fall back to `__libc_dlopen_mode`
//!    if the public symbol isn't visible (matches CE's `finddlopen`
//!    fallback chain).
//! 2. Allocate a small page in the target via [`crate::alloc::alloc_remote`]
//!    (Phase A) to hold the C string of the `.so` path.
//! 3. Write the path bytes into the allocated page.
//! 4. Attach ptrace, save GP regs, set up a fresh register set:
//!    - `rip = dlopen`
//!    - `rdi = path_addr` (the page from step 2)
//!    - `rsi = RTLD_NOW`  (= 2)
//!    - `rdx = 0`         (no caller info — Linux ignores it)
//!    - `rsp` aligned with the implicit push of the sentinel return.
//!    Write the sentinel `0xCE0` at rsp.
//! 5. PTRACE_CONT(SIGCONT), wait for the target to fault on the sentinel.
//! 6. Read `rax` — that's the `dlopen` return value (`void *handle`, or
//!    `NULL` on failure).
//! 7. Restore GP regs, detach. Free the path page best-effort
//!    (the loaded library is already mapped; the path string isn't
//!    needed anymore).
//!
//! Caveats accepted for the initial implementation:
//! - Only the attached thread is paused. If another thread is mid-`dlopen`
//!   we deadlock the same way CE does; the workaround there is the same
//!   ("don't inject during heavy load"). Real-world games injected at
//!   menus / pause screens never hit this.
//! - We don't pass a `dlopencaller` (CE's Android-only hack). On Linux
//!   glibc that argument is unused.
//! - `RTLD_NOW` is preferred over `RTLD_LAZY` so any unresolved symbols
//!   in our extension surface immediately as a NULL return from dlopen,
//!   not at first use deep inside the extension.

use nix::libc::user_regs_struct;
use nix::sys::ptrace::{self, AddressType};
use nix::sys::signal::Signal;
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::Pid;

use crate::alloc::{self, AllocError};
use crate::elfsym::{self, ElfSymError};
use crate::memory;

const SENTINEL_RETURN: u64 = 0x0ce0;
/// `dlopen` flag — resolve everything immediately so a missing symbol in
/// our extension surfaces as a NULL handle, not as a crash at first use.
const RTLD_NOW: u64 = 2;

#[derive(Debug, thiserror::Error)]
pub enum InjectError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("memory io: {0}")]
    Memory(#[from] memory::RuntimeError),
    #[error("alloc: {0}")]
    Alloc(#[from] AllocError),
    #[error("symbol lookup: {0}")]
    Symbol(#[from] ElfSymError),
    #[error("ptrace/errno: {0}")]
    Ptrace(#[from] nix::errno::Errno),
    #[error("waitpid returned unexpected status: {0:?}")]
    UnexpectedWait(WaitStatus),
    #[error("dlopen returned NULL in target pid {pid} — extension at {path:?} not loaded")]
    DlopenFailed { pid: i32, path: String },
}

/// Load `so_path` into the target via ptrace-driven `dlopen`. Returns the
/// `void *handle` the target's `dlopen` produced (non-zero on success).
///
/// `so_path` must be the absolute path on the host's filesystem; the
/// target sees the same path (we don't trampoline through `/proc/<pid>/root`).
/// The file must be readable by the target's UID — same constraint as
/// any normal `dlopen`.
pub fn inject_so(pid: Pid, so_path: &str) -> Result<u64, InjectError> {
    let dlopen_addr = resolve_dlopen(pid)?;

    // Allocate target-side scratch for the path string (null-terminated).
    let path_bytes = std::ffi::CString::new(so_path)
        .map_err(|_| InjectError::DlopenFailed {
            pid: pid.as_raw(),
            path: so_path.into(),
        })?
        .into_bytes_with_nul();
    let path_size = round_up_page(path_bytes.len());
    let path_addr = alloc::alloc_remote(pid, path_size, None, None)?;

    // Write the path through process_vm_writev. Failure here means the
    // alloc returned an address we can't actually write — propagate.
    if let Err(e) = memory::write_bytes(pid, path_addr, &path_bytes) {
        let _ = alloc::dealloc_remote(pid, path_addr, path_size);
        return Err(InjectError::Memory(e));
    }

    let raw = run_remote_dlopen(pid, dlopen_addr, path_addr);
    let _ = alloc::dealloc_remote(pid, path_addr, path_size);
    let handle = raw?;
    if handle == 0 {
        return Err(InjectError::DlopenFailed {
            pid: pid.as_raw(),
            path: so_path.into(),
        });
    }
    Ok(handle)
}

/// Look up the target's `dlopen`. Tries the canonical libc export first
/// (glibc >= 2.34) and falls back to the historical `__libc_dlopen_mode`
/// symbol when the public one isn't visible. Returning early on the
/// first hit is intentional — CE's `finddlopen` does the same.
fn resolve_dlopen(pid: Pid) -> Result<u64, ElfSymError> {
    if let Ok(addr) = elfsym::find_libc_symbol(pid, "dlopen") {
        return Ok(addr);
    }
    if let Ok(addr) = elfsym::find_libc_symbol(pid, "__libc_dlopen_mode") {
        return Ok(addr);
    }
    // Last resort: libdl.so.2 explicit (pre-glibc-2.34 split).
    elfsym::find_module_symbol(pid, "libdl.so", "dlopen")
}

fn run_remote_dlopen(pid: Pid, dlopen_addr: u64, path_addr: u64) -> Result<u64, InjectError> {
    attach_and_wait(pid)?;
    let original = ptrace::getregs(pid)?;
    let raw = (|| {
        let mut new = original;
        prepare_call_stack(pid, &mut new)?;
        new.rip = dlopen_addr;
        new.rax = 0;
        new.rdi = path_addr;
        new.rsi = RTLD_NOW;
        new.rdx = 0;
        new.r8 = 0;
        new.r9 = 0;
        ptrace::setregs(pid, new)?;
        ptrace::cont(pid, Signal::SIGCONT)?;
        wait_for_sentinel_stop(pid)?;
        let after = ptrace::getregs(pid)?;
        Ok::<u64, InjectError>(after.rax)
    })();
    let _ = ptrace::setregs(pid, original);
    let _ = ptrace::detach(pid, None);
    raw
}

fn attach_and_wait(pid: Pid) -> Result<(), InjectError> {
    ptrace::attach(pid)?;
    match waitpid(pid, None)? {
        WaitStatus::Stopped(_, Signal::SIGSTOP) => Ok(()),
        other => Err(InjectError::UnexpectedWait(other)),
    }
}

fn prepare_call_stack(pid: Pid, regs: &mut user_regs_struct) -> Result<(), InjectError> {
    // Mirror CE's x86_64 alignment dance: pull rsp down, align to 16,
    // OR 8 so the first 4 bits are `1000` — that emulates the push the
    // hypothetical `call` would have done before the function ran.
    regs.rsp = ((regs.rsp - 0x40) & !0xf) | 8;
    ptrace::write(pid, regs.rsp as AddressType, SENTINEL_RETURN as i64)?;
    Ok(())
}

fn wait_for_sentinel_stop(pid: Pid) -> Result<(), InjectError> {
    loop {
        match waitpid(pid, None)? {
            WaitStatus::Stopped(stopped_pid, sig) if stopped_pid == pid => match sig {
                Signal::SIGSEGV | Signal::SIGTRAP | Signal::SIGBUS => return Ok(()),
                other => ptrace::cont(pid, other)?,
            },
            WaitStatus::Stopped(other_pid, sig) => {
                let _ = ptrace::cont(other_pid, sig);
            }
            other => return Err(InjectError::UnexpectedWait(other)),
        }
    }
}

fn round_up_page(n: usize) -> usize {
    const PAGE: usize = 4096;
    n.div_ceil(PAGE) * PAGE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_up_page_works() {
        assert_eq!(round_up_page(0), 0);
        assert_eq!(round_up_page(1), 4096);
        assert_eq!(round_up_page(4096), 4096);
        assert_eq!(round_up_page(4097), 8192);
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 and a buildable .so at target/.../libcheat_runtime_extension.so"]
    fn inject_extension_into_forked_child_and_ping() {
        use crate::elfsym;
        use cheat_runtime_extension::protocol::{self, Request, Response};
        use nix::sys::signal::kill;
        use nix::sys::wait::waitpid;
        use nix::unistd::{ForkResult, fork};
        use std::os::unix::net::UnixStream;
        use std::time::Duration;

        // Build artifact path. Cargo places the cdylib at
        // target/debug/libcheat_runtime_extension.so (or target/<profile>).
        let so_path = std::env::var("CARGO_MANIFEST_DIR")
            .map(|d| {
                std::path::PathBuf::from(d).join("../../target/debug/libcheat_runtime_extension.so")
            })
            .expect("CARGO_MANIFEST_DIR")
            .canonicalize()
            .expect("libcheat_runtime_extension.so must be built first via `cargo build`");
        let so_str = so_path.to_string_lossy().into_owned();

        match unsafe { fork() }.expect("fork") {
            ForkResult::Child => {
                // Sleep long enough for the parent to inject + ping.
                std::thread::sleep(Duration::from_secs(8));
                std::process::exit(0);
            }
            ForkResult::Parent { child } => {
                std::thread::sleep(Duration::from_millis(200));

                let _handle = inject_so(child, &so_str).expect("inject_so");
                // Constructor runs synchronously inside dlopen — server is
                // up by the time inject_so returns. Give it a beat for the
                // accept loop to enter `set_nonblocking` mode.
                std::thread::sleep(Duration::from_millis(150));

                let path = protocol::socket_path_for(child.as_raw());
                let mut stream = UnixStream::connect(&path).expect("connect");
                protocol::write_handshake(&mut stream).unwrap();
                protocol::read_handshake(&mut stream).unwrap();

                protocol::write_frame(&mut stream, &Request::Ping).unwrap();
                let resp: Response = protocol::read_frame(&mut stream).unwrap();
                assert_eq!(resp, Response::Pong);

                // Clean up — shutdown server then kill child.
                protocol::write_frame(&mut stream, &Request::Shutdown).unwrap();
                let _: Response = protocol::read_frame(&mut stream).unwrap();

                let _ = kill(child, Signal::SIGKILL);
                let _ = waitpid(child, None);
                // Silence dead_code on the imported helper.
                let _ = elfsym::find_libc_symbol(Pid::this(), "mmap");
            }
        }
    }
}
