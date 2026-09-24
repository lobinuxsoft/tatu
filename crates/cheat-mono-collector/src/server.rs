//! TCP loopback server that exposes the Mono resolver to native-Linux tatu.
//!
//! Under Proton the per-game container shares the host network namespace, so a
//! Windows PE running in the game process can bind a loopback socket that a
//! native-Linux process connects to — confirmed cross-boundary IPC that avoids
//! Wine named-pipe quirks. Windows-only (uses the in-process Mono API).

use std::io::BufReader;
use std::net::{Ipv4Addr, TcpListener};

use crate::mono::MonoApi;
use crate::protocol::DEFAULT_PORT;
use crate::resolver::{Flow, handle_one};

/// Bind `127.0.0.1:DEFAULT_PORT` and serve resolver commands until the process
/// exits. Each accepted connection is served to completion (one game = one
/// client at a time); connections are handled sequentially because Mono calls
/// must run on the attached thread.
///
/// # Safety
/// `mono` must reference a live in-process Mono runtime.
pub unsafe fn serve(mono: &MonoApi) {
    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, DEFAULT_PORT)) {
        Ok(l) => l,
        // Another collector instance already owns the port, or sockets are
        // unavailable — nothing useful to do, so the thread just exits.
        Err(_) => return,
    };

    for conn in listener.incoming() {
        let Ok(stream) = conn else { continue };
        // Disable Nagle: requests are tiny and strictly request/response, so
        // latency matters more than batching.
        let _ = stream.set_nodelay(true);

        let mut writer = stream;
        let Ok(read_half) = writer.try_clone() else {
            continue;
        };
        let mut reader = BufReader::new(read_half);

        // SAFETY: forwarded from this function's contract — Mono is live and the
        // thread running `serve` was attached before we got here. Serve commands
        // until the client terminates, the peer closes, or an IO error occurs.
        while let Ok(Flow::Continue) = unsafe {
            handle_one(
                &mut Duplex {
                    r: &mut reader,
                    w: &mut writer,
                },
                mono,
            )
        } {}
    }
}

/// Glue a buffered reader and the raw writer into one `Read + Write` value for
/// [`handle_one`], so reads are buffered without buffering writes (responses
/// must hit the socket promptly).
struct Duplex<'a, R, W> {
    r: &'a mut R,
    w: &'a mut W,
}

impl<R: std::io::Read, W> std::io::Read for Duplex<'_, R, W> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.r.read(buf)
    }
}

impl<R, W: std::io::Write> std::io::Write for Duplex<'_, R, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.w.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.w.flush()
    }
}
