//! Pause every thread of a target process before applying a multi-byte
//! code patch, then resume them all afterwards. Ports the
//! `pauseProcess` / `resumeProcess` pair from CE's ceserver
//! (`extensionloader.c:178`) but extends it to attach every TID, not just
//! the main thread — `process_vm_writev` is atomic per syscall but does
//! NOT halt the target, so a sibling thread that happens to be executing
//! the bytes we're rewriting can fetch a mix of old + new bytes and
//! decode an invalid instruction. CE Windows avoids this with
//! `SuspendThread` loops; this module is the Linux equivalent.
//!
//! Usage:
//!
//! ```ignore
//! let pause = PausedTarget::pause(pid)?;
//! // ... do all the writes ...
//! drop(pause); // resumes
//! ```
//!
//! `Drop` is best-effort: detach errors are ignored so we never leak a
//! stopped game even if the resume fails partway through.

use std::fs;
use std::io;
use std::path::PathBuf;

use nix::sys::ptrace;
use nix::sys::signal::Signal;
use nix::sys::wait::{WaitStatus, waitpid};
use nix::unistd::Pid;

#[derive(Debug, thiserror::Error)]
pub enum ThreadPauseError {
    #[error("io reading /proc/{pid}/task: {source}")]
    Io {
        pid: i32,
        #[source]
        source: io::Error,
    },
    #[error("ptrace/errno: {0}")]
    Ptrace(#[from] nix::Error),
    #[error("waitpid returned unexpected status while pausing {tid}: {status:?}")]
    UnexpectedWait { tid: i32, status: WaitStatus },
}

/// RAII guard over every thread of a target process. While alive, every
/// TID is ptrace-attached and stopped on SIGSTOP. Drop detaches them all,
/// resuming execution.
#[derive(Debug)]
pub struct PausedTarget {
    attached: Vec<Pid>,
}

impl PausedTarget {
    /// Attach to and stop every thread under `pid`. Best-effort: a TID
    /// that vanishes between enumeration and attach (rare race when the
    /// game itself spawns / exits threads) is silently skipped.
    ///
    /// No-op when `pid == Pid::this()` — Linux refuses ptrace-on-self
    /// with EPERM by design, and a single-process unit test isn't racing
    /// itself anyway.
    pub fn pause(pid: Pid) -> Result<Self, ThreadPauseError> {
        if pid == Pid::this() {
            return Ok(Self {
                attached: Vec::new(),
            });
        }
        let mut attached = Vec::new();
        for tid in enumerate_threads(pid)? {
            if let Err(e) = attach_one(tid) {
                // Best-effort: roll back what we already attached.
                for prior in &attached {
                    let _ = ptrace::detach(*prior, None);
                }
                return Err(e);
            }
            attached.push(tid);
        }
        Ok(Self { attached })
    }

    /// How many threads we currently hold paused. Mostly used by tests.
    pub fn thread_count(&self) -> usize {
        self.attached.len()
    }
}

impl Drop for PausedTarget {
    fn drop(&mut self) {
        for tid in &self.attached {
            let _ = ptrace::detach(*tid, None);
        }
    }
}

/// Read `/proc/<pid>/task/*` and return every TID listed. Includes the
/// main thread (whose TID equals the PID).
fn enumerate_threads(pid: Pid) -> Result<Vec<Pid>, ThreadPauseError> {
    let path = PathBuf::from(format!("/proc/{}/task", pid.as_raw()));
    let entries = fs::read_dir(&path).map_err(|source| ThreadPauseError::Io {
        pid: pid.as_raw(),
        source,
    })?;
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| ThreadPauseError::Io {
            pid: pid.as_raw(),
            source,
        })?;
        if let Some(name) = entry.file_name().to_str()
            && let Ok(tid) = name.parse::<i32>()
        {
            out.push(Pid::from_raw(tid));
        }
    }
    Ok(out)
}

/// Attach to one TID and wait for the SIGSTOP that `ptrace::attach`
/// generates. Sibling-thread signals that arrive in the meantime are
/// re-forwarded so we don't swallow user-visible state.
fn attach_one(tid: Pid) -> Result<(), ThreadPauseError> {
    // `ESRCH` here means the TID vanished between enumeration and attach
    // (the game spawned a worker that exited immediately). Treat as
    // already-gone and continue.
    match ptrace::attach(tid) {
        Ok(()) => {}
        Err(nix::errno::Errno::ESRCH) => return Ok(()),
        Err(e) => return Err(e.into()),
    }
    loop {
        match waitpid(tid, None)? {
            WaitStatus::Stopped(stopped, Signal::SIGSTOP) if stopped == tid => return Ok(()),
            WaitStatus::Stopped(other_pid, sig) => {
                // A different thread we already attached delivered a
                // pending signal — forward it.
                let _ = ptrace::cont(other_pid, sig);
            }
            WaitStatus::Exited(exited, _) | WaitStatus::Signaled(exited, _, _) if exited == tid => {
                // Thread exited under us — bail; the caller's attached
                // list will not include this TID.
                return Ok(());
            }
            other => {
                return Err(ThreadPauseError::UnexpectedWait {
                    tid: tid.as_raw(),
                    status: other,
                });
            }
        }
    }
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
    fn pause_then_resume_a_simple_target() {
        let mut child = spawn_sleep();
        let pid = Pid::from_raw(child.id() as i32);
        std::thread::sleep(std::time::Duration::from_millis(50));

        let pause = PausedTarget::pause(pid).expect("pause");
        // sleep is single-threaded → exactly 1 TID.
        assert_eq!(pause.thread_count(), 1);
        drop(pause); // resumes

        let _ = child.kill();
        let _ = child.wait();
    }
}
