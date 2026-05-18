//! In-process cheat-runtime extension.
//!
//! Loaded into the target Linux process via `dlopen` by
//! `cheat_runtime::inject`. The `ctor::ctor` macro registers a constructor
//! that runs at `dlopen` time, which boots the IPC server thread bound to
//! `/tmp/cheat-runtime-<pid>.sock`. From that point the host
//! (`cheat_runtime::extension::Extension`) can speak the wire protocol in
//! [`protocol`] over a Unix socket to drive in-process operations:
//!
//! - `Ping` — liveness check.
//! - `Alloc` / `Free` — fast in-process `malloc`/`free` (avoids the
//!   ptrace `mmap` ping-pong for small per-cheat scratch buffers).
//! - `WriteState` / `ReadState` / `DeleteState` — key/value store shared
//!   between the host and the extension's own hooks.
//! - `SetSpeedhack { factor }` — engage / adjust / disengage the
//!   clock_gettime / gettimeofday speedhack ([`speedhack`] module).
//! - `Shutdown` — close the IPC channel cleanly.
//!
//! `crate-type = ["cdylib", "rlib"]` so the same crate doubles as the
//! cdylib injected into the target AND the rlib that the host
//! (`cheat-runtime`) depends on to import [`protocol`] types — both ends
//! share one definition of the wire format that way.

pub mod alloc_helper;
pub mod protocol;
pub mod server;
pub mod speedhack;
pub mod state_store;

/// Constructor that runs the first time `dlopen` loads this `.so` into
/// the target process. `ctor::ctor` puts the function pointer in
/// `.init_array` so glibc's loader runs it before `dlopen` returns.
#[cfg(not(test))]
#[ctor::ctor]
fn on_load() {
    server::start();
}

/// Public re-entry for unit tests / hosts that import this as rlib.
/// `#[ctor]` is only registered in cdylib builds (the `not(test)` guard
/// above); when the rlib is consumed by `cheat-runtime` or a test
/// harness the consumer can boot the server manually if it wants to
/// exercise the IPC end-to-end.
pub fn boot_server_for_tests() {
    server::start();
}

#[cfg(test)]
mod smoke {
    use super::*;
    use crate::protocol::{Request, Response, read_frame, write_frame};
    use std::os::unix::net::UnixStream;

    /// Boot the server, connect, ping, shutdown — end-to-end round trip
    /// inside the test process. This isn't the dlopen-injected case, but
    /// it proves the server + protocol + dispatch layers compose without
    /// needing ptrace privileges.
    #[test]
    fn local_round_trip_ping() {
        boot_server_for_tests();
        // Give the accept loop a moment to enter its loop.
        std::thread::sleep(std::time::Duration::from_millis(50));

        let pid = unsafe { libc::getpid() };
        let path = crate::protocol::socket_path_for(pid);
        let mut stream = UnixStream::connect(&path).expect("connect");
        protocol::write_handshake(&mut stream).unwrap();
        protocol::read_handshake(&mut stream).unwrap();

        write_frame(&mut stream, &Request::Ping).unwrap();
        let resp: Response = read_frame(&mut stream).unwrap();
        assert_eq!(resp, Response::Pong);

        write_frame(&mut stream, &Request::Shutdown).unwrap();
        let resp: Response = read_frame(&mut stream).unwrap();
        assert_eq!(resp, Response::ShutdownAck);
    }
}
