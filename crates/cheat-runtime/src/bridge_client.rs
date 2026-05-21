//! Linux-side client for `tatu-bridge.exe --connect`.
//!
//! The bridge runs inside a Proton wineprefix and binds an AF_UNIX
//! socket at `C:\users\Public\tatu-bridge.sock`. Wine forwards
//! AF_UNIX to the Linux kernel, so the socket file appears at
//! `<wineprefix>/drive_c/users/Public/tatu-bridge.sock` and we dial
//! it with a regular `std::os::unix::net::UnixStream` — the wire
//! format is the same `CHRT v1` protocol the [`extension::Extension`]
//! client uses against the Linux ptrace backend.
//!
//! Same shape as [`extension::Extension`] minus the inject step:
//! the bridge is already running when we dial it (spawned by
//! `tatu-bridge.exe --launch` as a sibling of the game inside one
//! Proton invocation; see #106 Phase 1).
//!
//! [`extension::Extension`]: crate::extension::Extension

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tatu_proto::{
    ProtocolError, Request, Response, bridge_socket_path_linux, read_frame, read_handshake,
    write_frame, write_handshake,
};

#[derive(Debug, thiserror::Error)]
pub enum BridgeClientError {
    #[error("protocol: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("bridge returned error: {0}")]
    Remote(String),
    #[error("unexpected response for {expected}: got {got:?}")]
    UnexpectedResponse {
        expected: &'static str,
        got: Response,
    },
    #[error("bridge socket at {path:?} never appeared within {timeout_ms}ms")]
    SocketTimeout { path: PathBuf, timeout_ms: u64 },
}

pub struct BridgeClient {
    stream: UnixStream,
}

impl BridgeClient {
    /// Dial the bridge socket inside `prefix` (a `STEAM_COMPAT_DATA_PATH`-
    /// shaped path, i.e. `<steamapps>/compatdata/<appid>/pfx`).
    /// Retries on `ENOENT` to absorb the brief window between the
    /// bridge starting and `bind()` completing.
    pub fn connect(prefix: &Path) -> Result<Self, BridgeClientError> {
        Self::connect_path(&bridge_socket_path_linux(prefix))
    }

    /// Lower-level dial: connect to a Unix socket at exactly `path`.
    /// Useful for tests that spawn a mock server on a tempdir path.
    pub fn connect_path(path: &Path) -> Result<Self, BridgeClientError> {
        let stream = connect_with_retries(path, Duration::from_millis(50), 40)?;
        Ok(Self { stream })
    }

    pub fn ping(&mut self) -> Result<(), BridgeClientError> {
        match self.request(Request::Ping)? {
            Response::Pong => Ok(()),
            got => Err(BridgeClientError::UnexpectedResponse {
                expected: "Pong",
                got,
            }),
        }
    }

    /// Tell the bridge to stop accepting connections, unbind the
    /// socket, and exit. The `--launch` parent will then complete
    /// its `WaitForSingleObject` on the game and tear everything
    /// down anyway.
    pub fn shutdown(&mut self) -> Result<(), BridgeClientError> {
        match self.request(Request::Shutdown)? {
            Response::ShutdownAck => Ok(()),
            got => Err(BridgeClientError::UnexpectedResponse {
                expected: "ShutdownAck",
                got,
            }),
        }
    }

    /// Send an arbitrary CHRT v1 [`Request`] and wait for the matching
    /// [`Response`]. Exposed publicly so Phase 6+ callers (value-cheats,
    /// recovery) can issue requests beyond Ping / Shutdown without each
    /// of them needing a sugar method on this client.
    pub fn request(&mut self, req: Request) -> Result<Response, BridgeClientError> {
        write_frame(&mut self.stream, &req)?;
        self.stream.flush()?;
        let resp = read_frame(&mut self.stream)?;
        Ok(resp)
    }
}

fn connect_with_retries(
    path: &Path,
    interval: Duration,
    attempts: u32,
) -> Result<UnixStream, BridgeClientError> {
    for _ in 0..attempts {
        if let Ok(mut s) = UnixStream::connect(path) {
            write_handshake(&mut s)?;
            read_handshake(&mut s)?;
            return Ok(s);
        }
        std::thread::sleep(interval);
    }
    Err(BridgeClientError::SocketTimeout {
        path: path.to_path_buf(),
        timeout_ms: (interval.as_millis() as u64) * attempts as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::thread;
    use tempfile::tempdir;

    /// Stand-in for `tatu-bridge --connect`'s AF_UNIX server, but
    /// running natively on Linux so the test doesn't need Wine. Same
    /// wire format (CHRT v1 from `tatu-proto`), same dispatch
    /// (`Ping → Pong`, `Shutdown → ShutdownAck`, anything else →
    /// `Err`). Validates the client + protocol layer end-to-end.
    fn spawn_mock_bridge(path: PathBuf) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let listener = UnixListener::bind(&path).expect("mock bind");
            let (mut stream, _) = listener.accept().expect("mock accept");
            write_handshake(&mut stream).unwrap();
            read_handshake(&mut stream).unwrap();
            loop {
                let req: Request = match read_frame(&mut stream) {
                    Ok(r) => r,
                    Err(ProtocolError::Io(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                        break;
                    }
                    Err(e) => panic!("mock read: {e}"),
                };
                let resp = match req {
                    Request::Ping => Response::Pong,
                    Request::Shutdown => Response::ShutdownAck,
                    other => Response::Err {
                        message: format!("{other:?} not implemented in mock"),
                    },
                };
                let stop = matches!(resp, Response::ShutdownAck);
                write_frame(&mut stream, &resp).unwrap();
                if stop {
                    break;
                }
            }
        })
    }

    #[test]
    fn ping_shutdown_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("tatu-bridge.sock");
        let server = spawn_mock_bridge(path.clone());

        let mut client = BridgeClient::connect_path(&path).expect("dial mock");
        client.ping().expect("ping");
        client.shutdown().expect("shutdown");

        server.join().expect("mock server thread");
    }
}
