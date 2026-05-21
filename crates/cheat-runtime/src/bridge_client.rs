//! Linux-side client for `tatu-bridge.exe --connect`.
//!
//! The bridge runs inside a Proton wineprefix and binds TCP loopback
//! on `127.0.0.1:<kernel-assigned-port>`. The port is written to
//! `C:\users\Public\tatu-bridge.port` inside the prefix, which the
//! Linux side reads from
//! `<wineprefix>/drive_c/users/Public/tatu-bridge.port`. We then dial
//! `127.0.0.1:<port>` over the host kernel's loopback — Wine
//! forwards the in-prefix bind to the host network stack, so the
//! Linux `TcpStream::connect` reaches the in-prefix PE directly.
//!
//! Why TCP localhost over AF_UNIX: AF_UNIX in Wine's Winsock fails
//! `WSAEAFNOSUPPORT` on some Proton builds (notably Proton
//! Experimental). TCP loopback is first-class in Wine since 1.x and
//! works against any Proton.
//!
//! Wire format is the same `CHRT v1` protocol the
//! [`extension::Extension`] client uses against the Linux ptrace
//! backend. Same shape as `Extension` minus the inject step: the
//! bridge is already running when we dial it (spawned by
//! `tatu-bridge.exe --launch` as a sibling of the game inside one
//! Proton invocation; see #106 Phase 1).
//!
//! [`extension::Extension`]: crate::extension::Extension

use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tatu_proto::{
    BRIDGE_HOST, ProtocolError, Request, Response, bridge_port_file_path_linux, read_frame,
    read_handshake, write_frame, write_handshake,
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
    #[error("bridge port file at {path:?} never appeared within {timeout_ms}ms")]
    PortFileTimeout { path: PathBuf, timeout_ms: u64 },
    #[error("bridge port file at {path:?} contains invalid u16: {body:?}")]
    PortFileInvalid { path: PathBuf, body: String },
}

pub struct BridgeClient {
    stream: TcpStream,
}

impl BridgeClient {
    /// Dial the bridge for the wineprefix at `prefix` (a
    /// `STEAM_COMPAT_DATA_PATH`-shaped path, i.e.
    /// `<steamapps>/compatdata/<appid>/pfx`). Reads the port-discovery
    /// file the bridge dropped after binding, then connects to
    /// `127.0.0.1:<port>`. Retries on `ENOENT` to absorb the brief
    /// window between the bridge starting and the port-file write.
    pub fn connect(prefix: &Path) -> Result<Self, BridgeClientError> {
        let port_file = bridge_port_file_path_linux(prefix);
        let port = read_port_with_retries(&port_file, Duration::from_millis(50), 40)?;
        Self::connect_port(port)
    }

    /// Lower-level dial: connect to `127.0.0.1:port`. Useful for
    /// tests that spawn a mock server on a pre-allocated port.
    pub fn connect_port(port: u16) -> Result<Self, BridgeClientError> {
        let mut stream = TcpStream::connect((BRIDGE_HOST, port))?;
        write_handshake(&mut stream)?;
        read_handshake(&mut stream)?;
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

    /// Tell the bridge to stop accepting connections, drop the port
    /// file, and exit. The `--launch` parent will then complete its
    /// `WaitForSingleObject` on the game and tear everything down
    /// anyway.
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
    /// recovery, AA scripts) can issue requests beyond Ping / Shutdown.
    pub fn request(&mut self, req: Request) -> Result<Response, BridgeClientError> {
        write_frame(&mut self.stream, &req)?;
        self.stream.flush()?;
        let resp = read_frame(&mut self.stream)?;
        Ok(resp)
    }
}

fn read_port_with_retries(
    path: &Path,
    interval: Duration,
    attempts: u32,
) -> Result<u16, BridgeClientError> {
    for _ in 0..attempts {
        match std::fs::read_to_string(path) {
            Ok(body) => {
                return body.trim().parse::<u16>().map_err(|_| {
                    BridgeClientError::PortFileInvalid {
                        path: path.to_path_buf(),
                        body,
                    }
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::thread::sleep(interval);
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(BridgeClientError::PortFileTimeout {
        path: path.to_path_buf(),
        timeout_ms: (interval.as_millis() as u64) * attempts as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    /// Stand-in for `tatu-bridge --connect`'s TCP server, running
    /// natively on Linux so the test doesn't need Wine. Same wire
    /// format (CHRT v1 from `tatu-proto`), same dispatch (`Ping →
    /// Pong`, `Shutdown → ShutdownAck`, anything else → `Err`).
    /// Validates the client + protocol layer end-to-end.
    fn spawn_mock_bridge() -> (u16, thread::JoinHandle<()>) {
        let listener = TcpListener::bind((BRIDGE_HOST, 0u16)).expect("mock bind");
        let port = listener.local_addr().expect("mock local_addr").port();
        let handle = thread::spawn(move || {
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
        });
        (port, handle)
    }

    #[test]
    fn ping_shutdown_round_trip() {
        let (port, server) = spawn_mock_bridge();
        let mut client = BridgeClient::connect_port(port).expect("dial mock");
        client.ping().expect("ping");
        client.shutdown().expect("shutdown");
        server.join().expect("mock server thread");
    }
}
