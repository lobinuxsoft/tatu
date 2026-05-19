//! Out-of-process memory read/write for any Linux PID.
//!
//! Wraps `process_vm_readv` / `process_vm_writev` (since Linux 3.2): a single
//! syscall transfers bytes between our address space and the target process
//! without ptrace-stopping it. Works against any process the calling UID can
//! access — including Proton games, which the kernel sees as ordinary Linux
//! processes regardless of the Wine layer inside.
//!
//! Requires `kernel.yama.ptrace_scope <= 0` for cross-process access (Bazzite
//! default; documented for other distros).

use std::io::{self, IoSlice, IoSliceMut};

use nix::sys::ptrace::{self, AddressType};
use nix::sys::signal::Signal;
use nix::sys::uio::{RemoteIoVec, process_vm_readv, process_vm_writev};
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::Pid;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("nix: {0}")]
    Nix(#[from] nix::Error),
    #[error("short read: addr 0x{addr:x}, requested {requested}B, got {got}B")]
    ShortRead {
        addr: u64,
        requested: usize,
        got: usize,
    },
    #[error("short write: addr 0x{addr:x}, requested {requested}B, wrote {got}B")]
    ShortWrite {
        addr: u64,
        requested: usize,
        got: usize,
    },
}

/// Read `len` bytes starting at `addr` from process `pid`.
///
/// Returns [`RuntimeError::ShortRead`] if the kernel transferred fewer bytes
/// than requested (typical cause: the region crosses an unmapped page).
pub fn read_bytes(pid: Pid, addr: u64, len: usize) -> Result<Vec<u8>, RuntimeError> {
    if len == 0 {
        return Ok(Vec::new());
    }
    let mut buf = vec![0u8; len];
    let got = {
        let mut local = [IoSliceMut::new(&mut buf)];
        let remote = [RemoteIoVec {
            base: addr as usize,
            len,
        }];
        process_vm_readv(pid, &mut local, &remote)?
    };
    if got != len {
        return Err(RuntimeError::ShortRead {
            addr,
            requested: len,
            got,
        });
    }
    Ok(buf)
}

/// Read up to `len` bytes from `pid` at `addr`, returning whatever the
/// kernel managed to transfer.
///
/// `process_vm_readv` can satisfy a partial request when the requested
/// range spans into an unmapped or unreadable page. The exact-length
/// [`read_bytes`] surfaces that as [`RuntimeError::ShortRead`]; this
/// permissive variant truncates the buffer to the transferred size and
/// returns it. CE's scanner does the same (`ceserver/api.c`
/// `ReadProcessMemory` callers ignore the short-read return). Use this in
/// bulk-scan paths where a partial chunk is still useful for pattern
/// matching.
pub fn read_bytes_partial(pid: Pid, addr: u64, len: usize) -> Result<Vec<u8>, RuntimeError> {
    if len == 0 {
        return Ok(Vec::new());
    }
    let mut buf = vec![0u8; len];
    let got = {
        let mut local = [IoSliceMut::new(&mut buf)];
        let remote = [RemoteIoVec {
            base: addr as usize,
            len,
        }];
        process_vm_readv(pid, &mut local, &remote)?
    };
    buf.truncate(got);
    Ok(buf)
}

/// Write `data` to `addr` in process `pid`.
///
/// Tries the fast `process_vm_writev` path first. On `EFAULT` (typical for
/// read-only `.text` pages — Linux refuses cross-process writev into RX
/// mappings) falls back to ptrace `PTRACE_POKEDATA` which the kernel
/// permits for tracer→tracee writes regardless of page perms. The ptrace
/// fallback is ~8 bytes per syscall and requires us to briefly attach and
/// stop the target; that matches CE's own behaviour
/// (`ceserver/api.c::WriteProcessMemory` uses POKEDATA for code patches).
///
/// Returns [`RuntimeError::ShortWrite`] if neither path can transfer the
/// full payload.
pub fn write_bytes(pid: Pid, addr: u64, data: &[u8]) -> Result<(), RuntimeError> {
    if data.is_empty() {
        return Ok(());
    }
    let local = [IoSlice::new(data)];
    let remote = [RemoteIoVec {
        base: addr as usize,
        len: data.len(),
    }];
    match process_vm_writev(pid, &local, &remote) {
        Ok(got) if got == data.len() => Ok(()),
        Ok(got) => Err(RuntimeError::ShortWrite {
            addr,
            requested: data.len(),
            got,
        }),
        Err(nix::errno::Errno::EFAULT) => write_bytes_ptrace(pid, addr, data),
        Err(e) => Err(e.into()),
    }
}

/// Ptrace fallback for writes into read-only target pages. Attaches once,
/// writes 8-byte words via `PTRACE_POKEDATA` (reading + masking the tail
/// when the payload isn't 8-aligned to avoid clobbering surrounding
/// bytes), detaches. Slow (~1μs/word) but pages-perms-agnostic.
fn write_bytes_ptrace(pid: Pid, addr: u64, data: &[u8]) -> Result<(), RuntimeError> {
    ptrace::attach(pid)?;
    match waitpid(pid, None) {
        Ok(WaitStatus::Stopped(_, Signal::SIGSTOP)) => {}
        Ok(other) => {
            let _ = ptrace::detach(pid, None);
            return Err(RuntimeError::Nix(
                other_status_to_errno(other).unwrap_or(nix::errno::Errno::ESRCH),
            ));
        }
        Err(e) => {
            let _ = ptrace::detach(pid, None);
            return Err(RuntimeError::Nix(e));
        }
    }
    let result = pokedata_chunks(pid, addr, data);
    let _ = ptrace::detach(pid, None);
    result
}

/// Write `data` to `addr` assuming `pid` is **already ptrace-attached
/// and stopped** by the caller. Uses `PTRACE_POKEDATA` directly without
/// an internal attach/detach dance — that's the fast path callers like
/// [`crate::executor::Engine::enable`] want when they're wrapping a
/// whole batch of writes in a single attach.
///
/// CE on Linux follows the same pattern: `autoassembler.pas:4116` pauses
/// the process once, then writes every assembled block under that one
/// pause so the target never observes a half-applied trampoline. The
/// ceserver Linux backend uses `PTRACE_POKEDATA` for those writes
/// (`api.c::WriteProcessMemory`), which bypasses page protections — no
/// `mprotect` dance needed to touch a read-only `.text` page.
pub fn write_bytes_attached(pid: Pid, addr: u64, data: &[u8]) -> Result<(), RuntimeError> {
    if data.is_empty() {
        return Ok(());
    }
    pokedata_chunks(pid, addr, data)
}

fn pokedata_chunks(pid: Pid, addr: u64, data: &[u8]) -> Result<(), RuntimeError> {
    let word_size = std::mem::size_of::<usize>();
    let mut offset = 0;
    while offset < data.len() {
        let word_addr = addr + offset as u64;
        let remaining = data.len() - offset;
        let word = if remaining >= word_size {
            let mut bytes = [0u8; 8];
            bytes[..word_size].copy_from_slice(&data[offset..offset + word_size]);
            i64::from_le_bytes(bytes)
        } else {
            // Tail: read current word, merge new bytes into the leading
            // positions, write back so we don't clobber adjacent code.
            let current = ptrace::read(pid, word_addr as AddressType)?;
            let mut bytes = current.to_le_bytes();
            bytes[..remaining].copy_from_slice(&data[offset..]);
            i64::from_le_bytes(bytes)
        };
        ptrace::write(pid, word_addr as AddressType, word)?;
        offset += word_size.min(remaining);
    }
    Ok(())
}

fn other_status_to_errno(_status: WaitStatus) -> Option<nix::errno::Errno> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn read_self_buffer_roundtrips_known_bytes() {
        let buf = b"GLaDOS-was-here-12345".to_vec();
        let addr = buf.as_ptr() as u64;
        let read = read_bytes(Pid::this(), addr, buf.len()).expect("read own memory");
        assert_eq!(read, buf);
    }

    #[test]
    fn write_self_buffer_mutates_known_bytes() {
        let mut buf = vec![0u8; 16];
        let addr = buf.as_mut_ptr() as u64;
        let payload = b"hello-payload-OK";
        write_bytes(Pid::this(), addr, payload).expect("write own memory");
        assert_eq!(&buf[..], payload);
    }

    #[test]
    fn read_zero_len_returns_empty_without_syscall() {
        let read = read_bytes(Pid::this(), 0, 0).expect("zero-length read is a no-op");
        assert!(read.is_empty());
    }

    #[test]
    fn write_zero_len_returns_ok_without_syscall() {
        write_bytes(Pid::this(), 0, &[]).expect("zero-length write is a no-op");
    }

    #[test]
    fn read_short_when_crossing_unmapped_page() {
        // Pick a high address that almost certainly is not mapped. A short
        // read manifests as an Nix EFAULT, not a partial transfer — the kernel
        // refuses any of it. Either way it must NOT silently succeed.
        let bad_addr = 0xdead_0000_0000u64;
        let result = read_bytes(Pid::this(), bad_addr, 16);
        assert!(result.is_err());
    }

    /// Integration: spawn `sleep 30` and read the first 4 bytes of its
    /// mapped image, which must be the ELF magic `7F 45 4C 46`. Relies on
    /// `kernel.yama.ptrace_scope <= 0` (Bazzite default) so we can attach
    /// process_vm_readv to a same-uid child.
    #[test]
    fn read_child_process_elf_magic() {
        let mut child = Command::new("sleep")
            .arg("5")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sleep");
        // Give the child a moment to map its image.
        sleep(Duration::from_millis(100));

        let pid = Pid::from_raw(child.id() as i32);
        let regions = crate::maps::read_maps(pid).expect("read child maps");
        let exec = regions
            .iter()
            .find(|r| r.perms.execute && r.path.to_string_lossy().contains("sleep"))
            .expect("expected an executable region pointing at the sleep binary");

        let head = read_bytes(pid, exec.start, 4).expect("read child memory");
        let _ = child.kill();
        let _ = child.wait();

        assert_eq!(
            &head, b"\x7fELF",
            "expected ELF magic at start of text segment"
        );
    }
}
