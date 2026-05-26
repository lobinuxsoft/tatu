//! PTRACE_PEEKDATA / POKEDATA fallback path for read/write that
//! `process_vm_readv` / `process_vm_writev` cannot satisfy. 1:1 port
//! of the inner loops in `ceserver/api.c`:
//! - `ReadProcessMemoryDebug` (line 3033) — the `MEMORY_SEARCH_OPTION
//!   != 0` branch that does PTRACE_PEEKDATA in a long-sized loop.
//! - `WriteProcessMemoryDebug` (line 2667) — the loop at lines
//!   2716-2766 with the tail-word PEEK+merge+POKE for unaligned sizes.
//!
//! # Why we need this
//!
//! `process_vm_writev` honours page protections — write to an `r-xp`
//! page → `EFAULT`. PTRACE_POKEDATA ignores them (the same primitive
//! `gdb` uses to set software breakpoints), so it's the only path
//! that can patch a `.text` segment of a game without first calling
//! `mprotect` (which is detectable by anti-cheat and requires
//! ptrace-mediated syscall injection on a hostile target).
//!
//! Reads are similar: `process_vm_readv` short-reads guard pages,
//! tracee-private mappings, and any region the kernel decides to
//! redact; PTRACE_PEEKDATA still works because the tracer has the
//! tracee's permissions.
//!
//! # Long-word arithmetic
//!
//! PTRACE_PEEKDATA / POKEDATA read/write `sizeof(long)` bytes at a
//! time (8 on x86_64). For partial-word tails, CE peeks the existing
//! word, overlays the new bytes, pokes the merged word back — the
//! same dance lives here in [`write_via_ptrace`].
//!
//! # Pre-conditions
//!
//! The caller (typically [`crate::Debugger`]) must have the target
//! TID ptrace-stopped under the current process. The functions
//! return [`PtraceError::Errno`] (likely `ESRCH` or `EPERM`) when
//! that contract is broken.

use nix::libc::{PTRACE_PEEKDATA, PTRACE_POKEDATA};
use nix::unistd::Pid;
use std::os::raw::c_void;

use crate::ptrace_helpers::{PtraceError, safe_ptrace};

/// `sizeof(long)` on the target ABI. x86_64-only build, so 8.
const WORD: usize = std::mem::size_of::<usize>();

/// Read `len` bytes from `tid` starting at `addr`, returning whatever
/// was read (which may be < `len` if a PEEK errors partway through).
/// 1:1 with CE `ReadProcessMemoryDebug` line 3148-3197 (the
/// `MEMORY_SEARCH_OPTION != 0` PEEKDATA loop).
pub fn read_via_ptrace(tid: Pid, addr: u64, len: usize) -> Result<Vec<u8>, PtraceError> {
    let mut out = Vec::with_capacity(len);
    let mut offset: usize = 0;
    while offset + WORD <= len {
        let word_ptr = (addr + offset as u64) as *mut c_void;
        let word = safe_ptrace(PTRACE_PEEKDATA, tid, word_ptr, std::ptr::null_mut())?;
        let bytes = (word as usize).to_ne_bytes();
        out.extend_from_slice(&bytes);
        offset += WORD;
    }
    if offset < len {
        // Tail partial word: read the full long, copy only the bytes
        // the caller asked for. PEEK never fails on a sub-word tail
        // unless the whole word straddles an unmapped page boundary —
        // we propagate the error so caller can decide on partials.
        let word_ptr = (addr + offset as u64) as *mut c_void;
        let word = safe_ptrace(PTRACE_PEEKDATA, tid, word_ptr, std::ptr::null_mut())?;
        let bytes = (word as usize).to_ne_bytes();
        out.extend_from_slice(&bytes[..len - offset]);
    }
    Ok(out)
}

/// Write `buffer` to `tid` at `addr`, returning the number of bytes
/// actually written. 1:1 with CE `WriteProcessMemoryDebug` line
/// 2716-2766: PTRACE_POKEDATA for every aligned long, then PEEK+merge+
/// POKE for the unaligned tail.
pub fn write_via_ptrace(tid: Pid, addr: u64, buffer: &[u8]) -> Result<usize, PtraceError> {
    let mut written = 0usize;
    let mut offset = 0usize;
    let len = buffer.len();
    while offset + WORD <= len {
        let mut bytes = [0u8; WORD];
        bytes.copy_from_slice(&buffer[offset..offset + WORD]);
        let word = usize::from_ne_bytes(bytes) as i64;
        let word_ptr = (addr + offset as u64) as *mut c_void;
        safe_ptrace(PTRACE_POKEDATA, tid, word_ptr, word as *mut c_void)?;
        offset += WORD;
        written += WORD;
    }
    if offset < len {
        // Tail: PEEK old long, merge new tail bytes on top, POKE back.
        let word_ptr = (addr + offset as u64) as *mut c_void;
        let old = safe_ptrace(PTRACE_PEEKDATA, tid, word_ptr, std::ptr::null_mut())?;
        let mut merged = (old as usize).to_ne_bytes();
        let tail = len - offset;
        merged[..tail].copy_from_slice(&buffer[offset..]);
        let new_word = usize::from_ne_bytes(merged) as i64;
        safe_ptrace(PTRACE_POKEDATA, tid, word_ptr, new_word as *mut c_void)?;
        written += tail;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Child, Command, Stdio};

    fn spawn_sleep() -> Child {
        Command::new("sleep")
            .arg("5")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sleep")
    }

    #[test]
    fn word_size_is_8_on_x86_64() {
        assert_eq!(WORD, 8);
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE"]
    fn read_via_ptrace_round_trips_against_child() {
        use crate::thread_control::{resume_thread, suspend_thread};
        let mut child = spawn_sleep();
        let pid = Pid::from_raw(child.id() as i32);
        let tid = pid;
        std::thread::sleep(std::time::Duration::from_millis(50));

        suspend_thread(pid, tid).expect("suspend");
        // Read the first 16 bytes at a random address that should be
        // mapped — the binary's entry point. Use /proc/<pid>/maps to
        // find any executable mapping.
        let maps = crate::maps::read_maps(pid).expect("read maps");
        let exec_region = maps.iter().find(|r| r.perms.execute).expect("exec region");
        let bytes = read_via_ptrace(tid, exec_region.start, 16).expect("peek");
        assert_eq!(bytes.len(), 16);
        resume_thread(pid, tid).expect("resume");

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE"]
    fn write_then_read_via_ptrace_round_trip_unaligned() {
        // Spawn a child that maps writable memory, suspend it, write
        // 3 bytes (sub-word), read back, verify. We need a target
        // with a known-writable address — use the child's stack via
        // GetThreadContext (#140) to read RSP.
        use crate::thread_context::{ContextFlags, get_thread_context};
        use crate::thread_control::{resume_thread, suspend_thread};
        let mut child = spawn_sleep();
        let pid = Pid::from_raw(child.id() as i32);
        let tid = pid;
        std::thread::sleep(std::time::Duration::from_millis(50));

        suspend_thread(pid, tid).expect("suspend");
        let ctx = get_thread_context(tid, ContextFlags::INTEGER).expect("ctx");
        // Stack is writable. Pick an address well below RSP so we
        // don't clobber actual frame data, but still in the mapped
        // stack range. 1024 bytes down is safe headroom.
        let scratch = ctx.regs.rsp - 1024;

        let original = read_via_ptrace(tid, scratch, 16).expect("peek");
        let payload: [u8; 3] = [0xAA, 0xBB, 0xCC];
        let n = write_via_ptrace(tid, scratch, &payload).expect("poke");
        assert_eq!(n, 3);

        let read_back = read_via_ptrace(tid, scratch, 3).expect("peek after poke");
        assert_eq!(read_back, payload);

        // Restore so the child doesn't crash on resume.
        write_via_ptrace(tid, scratch, &original).expect("restore");
        resume_thread(pid, tid).expect("resume");

        let _ = child.kill();
        let _ = child.wait();
    }
}
