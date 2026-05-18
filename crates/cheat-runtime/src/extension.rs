//! Host-side client for the `cheat-runtime-extension` cdylib.
//!
//! Drives the full lifecycle from outside the target process:
//!
//! 1. [`Extension::attach`] — `inject_so` (Phase E pipeline) loads the
//!    extension `.so` into the target, then opens the Unix socket the
//!    extension binds in its glibc-loader constructor, performs the
//!    handshake, and returns a connected handle.
//! 2. Methods like [`Extension::ping`], [`Extension::alloc`],
//!    [`Extension::set_speedhack`] send a single [`Request`] and read
//!    the corresponding [`Response`] one round-trip at a time.
//! 3. [`Extension::shutdown`] (or `Drop`) tells the in-process server to
//!    stop accepting new connections; the extension itself stays
//!    `dlopen`'d (we don't dlclose — that's a deliberate choice; future
//!    cheats might reuse the same extension).
//!
//! The extension's wire types live in the `cheat-runtime-extension`
//! crate (re-exported as a path dep from this crate) so the protocol
//! has exactly one definition, compiled identically into both sides of
//! the wire.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use nix::unistd::Pid;

use crate::inject::{self, InjectError};
use cheat_runtime_extension::protocol::{
    self, ProtocolError, Request, Response, read_frame, read_handshake, write_frame,
    write_handshake,
};

#[derive(Debug, thiserror::Error)]
pub enum ExtensionError {
    #[error("inject: {0}")]
    Inject(#[from] InjectError),
    #[error("protocol: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("extension returned error: {0}")]
    Remote(String),
    #[error("unexpected response for {expected}: got {got:?}")]
    UnexpectedResponse {
        expected: &'static str,
        got: Response,
    },
    #[error("extension's IPC socket at {path:?} never appeared within {timeout_ms}ms")]
    SocketTimeout {
        path: std::path::PathBuf,
        timeout_ms: u64,
    },
}

pub struct Extension {
    pid: Pid,
    stream: UnixStream,
    /// The handle that `dlopen` returned in the target. Stored mostly
    /// for future `dlclose` support; the current implementation never
    /// closes the library (deliberate — see module docs).
    #[allow(dead_code)]
    handle: u64,
}

impl Extension {
    /// Inject the extension `.so` at `so_path` into `pid`, then connect.
    /// Waits up to ~2s for the in-process socket listener to come up.
    pub fn attach(pid: Pid, so_path: &str) -> Result<Self, ExtensionError> {
        let handle = inject::inject_so(pid, so_path)?;
        let stream = connect_with_retries(pid, Duration::from_millis(50), 40)?;
        Ok(Self {
            pid,
            stream,
            handle,
        })
    }

    /// Connect to an already-injected extension. Useful for tests that
    /// boot the server in-process via `boot_server_for_tests` instead of
    /// going through ptrace.
    pub fn connect_existing(pid: Pid) -> Result<Self, ExtensionError> {
        let stream = connect_with_retries(pid, Duration::from_millis(50), 40)?;
        Ok(Self {
            pid,
            stream,
            handle: 0,
        })
    }

    pub fn pid(&self) -> Pid {
        self.pid
    }

    pub fn ping(&mut self) -> Result<(), ExtensionError> {
        match self.request(Request::Ping)? {
            Response::Pong => Ok(()),
            got => Err(ExtensionError::UnexpectedResponse {
                expected: "Pong",
                got,
            }),
        }
    }

    pub fn alloc(&mut self, size: u64) -> Result<u64, ExtensionError> {
        match self.request(Request::Alloc { size })? {
            Response::Alloc { addr } => Ok(addr),
            Response::Err { message } => Err(ExtensionError::Remote(message)),
            got => Err(ExtensionError::UnexpectedResponse {
                expected: "Alloc",
                got,
            }),
        }
    }

    pub fn free(&mut self, addr: u64) -> Result<(), ExtensionError> {
        match self.request(Request::Free { addr })? {
            Response::Freed => Ok(()),
            got => Err(ExtensionError::UnexpectedResponse {
                expected: "Freed",
                got,
            }),
        }
    }

    pub fn write_state(&mut self, key: &str, value: Vec<u8>) -> Result<(), ExtensionError> {
        match self.request(Request::WriteState {
            key: key.to_string(),
            value,
        })? {
            Response::State { .. } => Ok(()),
            got => Err(ExtensionError::UnexpectedResponse {
                expected: "State",
                got,
            }),
        }
    }

    pub fn read_state(&mut self, key: &str) -> Result<Option<Vec<u8>>, ExtensionError> {
        match self.request(Request::ReadState {
            key: key.to_string(),
        })? {
            Response::State { value } => Ok(value),
            got => Err(ExtensionError::UnexpectedResponse {
                expected: "State",
                got,
            }),
        }
    }

    pub fn delete_state(&mut self, key: &str) -> Result<(), ExtensionError> {
        match self.request(Request::DeleteState {
            key: key.to_string(),
        })? {
            Response::State { .. } => Ok(()),
            got => Err(ExtensionError::UnexpectedResponse {
                expected: "State",
                got,
            }),
        }
    }

    pub fn set_speedhack(&mut self, factor: Option<f64>) -> Result<Option<f64>, ExtensionError> {
        match self.request(Request::SetSpeedhack { factor })? {
            Response::Speedhack { factor } => Ok(factor),
            Response::Err { message } => Err(ExtensionError::Remote(message)),
            got => Err(ExtensionError::UnexpectedResponse {
                expected: "Speedhack",
                got,
            }),
        }
    }

    /// Tell the server to stop accepting connections and unbind the
    /// socket. The cdylib itself stays loaded.
    pub fn shutdown(&mut self) -> Result<(), ExtensionError> {
        match self.request(Request::Shutdown)? {
            Response::ShutdownAck => Ok(()),
            got => Err(ExtensionError::UnexpectedResponse {
                expected: "ShutdownAck",
                got,
            }),
        }
    }

    fn request(&mut self, req: Request) -> Result<Response, ExtensionError> {
        write_frame(&mut self.stream, &req)?;
        self.stream.flush()?;
        let resp = read_frame(&mut self.stream)?;
        Ok(resp)
    }
}

fn connect_with_retries(
    pid: Pid,
    interval: Duration,
    attempts: u32,
) -> Result<UnixStream, ExtensionError> {
    let path = protocol::socket_path_for(pid.as_raw());
    for _ in 0..attempts {
        if let Ok(mut s) = UnixStream::connect(&path) {
            write_handshake(&mut s)?;
            read_handshake(&mut s)?;
            return Ok(s);
        }
        std::thread::sleep(interval);
    }
    Err(ExtensionError::SocketTimeout {
        path,
        timeout_ms: (interval.as_millis() as u64) * attempts as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cheat_runtime_extension::boot_server_for_tests;

    /// Boot the in-process server (avoiding ptrace) and drive the full
    /// host-side client through every RPC. This is the host-side
    /// equivalent of `cheat_runtime_extension::smoke::local_round_trip_ping`
    /// — exercises every public method on `Extension`.
    #[test]
    fn local_full_command_round_trip() {
        boot_server_for_tests();
        std::thread::sleep(Duration::from_millis(100));

        let mut ext = Extension::connect_existing(Pid::this()).expect("connect");

        ext.ping().expect("ping");

        let addr = ext.alloc(256).expect("alloc");
        assert!(addr != 0);
        ext.free(addr).expect("free");

        ext.write_state("hp", vec![1, 2, 3, 4]).expect("write");
        let value = ext.read_state("hp").expect("read");
        assert_eq!(value.as_deref(), Some(&[1u8, 2, 3, 4][..]));
        ext.delete_state("hp").expect("delete");
        assert!(ext.read_state("hp").expect("read after delete").is_none());

        let applied = ext.set_speedhack(Some(2.0)).expect("speedhack on");
        assert_eq!(applied, Some(2.0));
        let off = ext.set_speedhack(None).expect("speedhack off");
        assert_eq!(off, None);

        ext.shutdown().expect("shutdown");
    }
}
