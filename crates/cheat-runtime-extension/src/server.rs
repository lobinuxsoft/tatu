//! In-process IPC server. Bound to a Unix domain socket whose path is
//! derived from the host PID (see [`crate::protocol::socket_path_for`]).
//! A single background thread accepts connections, reads a single frame
//! per connection, dispatches, writes the reply, and closes — keeps the
//! state thread-safe through a global `Mutex` without needing async or
//! a more elaborate executor inside the target's address space.
//!
//! The dispatcher routes each [`Request`] to:
//! - [`crate::alloc_helper`] for `Alloc`/`Free`
//! - [`crate::state_store`] for `WriteState`/`ReadState`/`DeleteState`
//! - [`crate::speedhack`] for `SetSpeedhack`
//! - itself for `Ping` / `Shutdown`

use std::io::{BufReader, BufWriter, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle};

use crate::alloc_helper;
use crate::protocol::{
    Request, Response, read_frame, read_handshake, socket_path_for, write_frame, write_handshake,
};
use crate::speedhack;
use crate::state_store;

/// Singleton-ish server handle. We only ever expect one extension per
/// target process; if `start()` is called a second time it's a no-op and
/// returns the existing handle.
static SERVER: OnceLock<ServerHandle> = OnceLock::new();

pub struct ServerHandle {
    pub running: Arc<AtomicBool>,
    pub thread: Option<JoinHandle<()>>,
    pub socket_path: std::path::PathBuf,
}

/// Start the server thread bound to `/tmp/cheat-runtime-<pid>.sock` where
/// `pid` is the current process's PID. Idempotent — subsequent calls
/// return the same handle. Logs to stderr on failure; never panics so a
/// glibc constructor call site stays safe.
pub fn start() {
    SERVER.get_or_init(|| {
        let pid = unsafe { libc::getpid() };
        let path = socket_path_for(pid);
        // Best-effort cleanup of a stale socket from a prior crashed run.
        let _ = std::fs::remove_file(&path);

        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!(
                    "cheat-runtime-extension: failed to bind {}: {e}",
                    path.display()
                );
                return ServerHandle {
                    running: Arc::new(AtomicBool::new(false)),
                    thread: None,
                    socket_path: path,
                };
            }
        };
        // Non-blocking so the accept loop can poll the shutdown flag.
        if let Err(e) = listener.set_nonblocking(true) {
            eprintln!("cheat-runtime-extension: set_nonblocking: {e}");
        }

        let running = Arc::new(AtomicBool::new(true));
        let running_thread = Arc::clone(&running);
        let path_for_thread = path.clone();
        let thread = thread::spawn(move || {
            accept_loop(listener, running_thread, path_for_thread);
        });

        ServerHandle {
            running,
            thread: Some(thread),
            socket_path: path,
        }
    });
}

fn accept_loop(listener: UnixListener, running: Arc<AtomicBool>, socket_path: std::path::PathBuf) {
    while running.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _addr)) => {
                let r = Arc::clone(&running);
                thread::spawn(move || {
                    if let Err(e) = handle_connection(stream, &r) {
                        eprintln!("cheat-runtime-extension: connection: {e}");
                    }
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => {
                eprintln!("cheat-runtime-extension: accept: {e}");
                break;
            }
        }
    }
    let _ = std::fs::remove_file(&socket_path);
}

fn handle_connection(
    stream: UnixStream,
    running: &Arc<AtomicBool>,
) -> Result<(), crate::protocol::ProtocolError> {
    stream.set_nonblocking(false)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = BufWriter::new(stream);

    write_handshake(&mut writer)?;
    writer.flush()?;
    read_handshake(&mut reader)?;

    loop {
        let req: Request = match read_frame(&mut reader) {
            Ok(r) => r,
            // EOF / disconnect — graceful close.
            Err(crate::protocol::ProtocolError::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                ) =>
            {
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        let resp = dispatch(req, running);
        write_frame(&mut writer, &resp)?;
        writer.flush()?;
    }
}

fn dispatch(req: Request, running: &Arc<AtomicBool>) -> Response {
    match req {
        Request::Ping => Response::Pong,
        Request::Alloc { size } => match alloc_helper::alloc(size as usize) {
            Some(addr) => Response::Alloc { addr },
            None => Response::Err {
                message: "alloc returned NULL".into(),
            },
        },
        Request::Free { addr } => {
            alloc_helper::free(addr);
            Response::Freed
        }
        Request::WriteState { key, value } => {
            state_store::write(key, value);
            Response::State { value: None }
        }
        Request::ReadState { key } => Response::State {
            value: state_store::read(&key),
        },
        Request::DeleteState { key } => {
            state_store::delete(&key);
            Response::State { value: None }
        }
        Request::SetSpeedhack { factor } => match speedhack::set_factor(factor) {
            Ok(applied) => Response::Speedhack { factor: applied },
            Err(e) => Response::Err {
                message: format!("speedhack: {e}"),
            },
        },
        Request::Shutdown => {
            running.store(false, Ordering::Relaxed);
            Response::ShutdownAck
        }
        // Phase 4 (#106) primitives are Win32-bridge-only — the Linux
        // in-process extension can't fulfil them. Tracker routes them to
        // the bridge backend when the per-game flag selects it.
        other @ (Request::AobScan { .. }
        | Request::PatchBytes { .. }
        | Request::RemoteAlloc { .. }
        | Request::RemoteFree { .. }
        | Request::WalkChain { .. }
        | Request::ReadChainValue { .. }
        | Request::WriteChainValue { .. }) => Response::Err {
            message: format!("{other:?} is not supported by the Linux in-process extension"),
        },
    }
}
