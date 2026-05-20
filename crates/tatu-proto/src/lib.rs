//! IPC wire protocol between the tracker (Linux host) and the in-process
//! extension or in-prefix bridge.
//!
//! Two server-side implementations consume the same protocol:
//!
//! - `cheat-runtime-extension` — a Linux `.so` `dlopen`'d into the target
//!   game process via `cheat_runtime::inject`. Talks back via Unix
//!   socket at `socket_path_for(pid)`.
//! - `tatu-bridge --connect` — a Win32 PE running inside the same
//!   wineprefix as a Proton-launched game. Binds an AF_UNIX socket
//!   under `Z:\\tmp\\...` (Wine forwards AF_UNIX to the Linux kernel)
//!   so the Linux tracker can dial the same wire format across the
//!   Wine boundary.
//!
//! Wire format: length-prefixed `bincode` messages. A frame is
//! `[u32 len big-endian][bincode bytes]`. Both sides must read the
//! length first to know how many bytes follow. Bincode wins over JSON
//! / postcard because it's the default zero-config Rust serialiser,
//! has fixed-size encoding for our small enums, and the producer /
//! consumer are guaranteed-same-version (compiled against this same
//! crate).
//!
//! All types are intentionally `Copy`-friendly where possible so the
//! server loop on the resource-constrained side never allocates on the
//! steady-state path — host-side allocations are fine, the target's
//! heap is the one we want to keep quiet.

use serde::{Deserialize, Serialize};

/// Default Unix socket path for the in-process extension bound to PID
/// `pid`. Both ends must agree; the host derives it from the target
/// PID it injected into.
pub fn socket_path_for(pid: i32) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("/tmp/cheat-runtime-{pid}.sock"))
}

/// Win32-side path the bridge binds its AF_UNIX listener on. Wine
/// translates `C:\users\Public\tatu-bridge.sock` to
/// `<wineprefix>/drive_c/users/Public/tatu-bridge.sock` on the Linux
/// filesystem; the tracker uses [`bridge_socket_path_linux`] to dial
/// the same socket as a regular Unix domain socket.
///
/// One socket per wineprefix (= per game running). The path is
/// stable, no PID suffix, so the tracker can compute it from
/// `STEAM_COMPAT_DATA_PATH` without first scraping a PID.
pub const BRIDGE_SOCKET_WIN_PATH: &str = r"C:\users\Public\tatu-bridge.sock";

/// Linux-side view of the bridge AF_UNIX socket. `prefix` is the
/// game's `STEAM_COMPAT_DATA_PATH` (i.e. `<steamapps>/compatdata/<appid>/pfx`
/// up through the `pfx` directory).
pub fn bridge_socket_path_linux(prefix: &std::path::Path) -> std::path::PathBuf {
    prefix.join("drive_c/users/Public/tatu-bridge.sock")
}

/// Magic + protocol version sent as a preamble on a new connection.
/// Lets either side reject a mismatched build cleanly before parsing
/// anything else.
pub const HANDSHAKE_MAGIC: [u8; 4] = *b"CHRT";
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Request {
    /// Liveness probe — replies with [`Response::Pong`].
    Ping,
    /// In-process `malloc`. Returns the host address as `u64`. The host
    /// is responsible for tracking it and pairing with [`Request::Free`].
    Alloc { size: u64 },
    /// `free(addr)` against a prior `Alloc`. No-op if the address is unknown.
    Free { addr: u64 },
    /// Store a byte vector under `key`. Overwrites any prior value.
    WriteState { key: String, value: Vec<u8> },
    /// Read the value stored under `key`. `None` if absent.
    ReadState { key: String },
    /// Drop the value at `key`. No-op if absent.
    DeleteState { key: String },
    /// Engage / adjust / disengage the speedhack. `factor == 1.0` is
    /// real-time; `factor > 1.0` makes the game perceive time as faster;
    /// `factor < 1.0` slower; `factor == 0.0` pauses.
    /// `None` disengages the hook entirely (restores real `clock_gettime`).
    SetSpeedhack { factor: Option<f64> },
    /// Tell the server to stop accepting new connections and unbind the
    /// socket. The extension stays loaded — only the IPC channel closes.
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Response {
    Pong,
    Alloc { addr: u64 },
    Freed,
    State { value: Option<Vec<u8>> },
    Speedhack { factor: Option<f64> },
    ShutdownAck,
    Err { message: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("bincode encode: {0}")]
    Encode(#[from] bincode::error::EncodeError),
    #[error("bincode decode: {0}")]
    Decode(#[from] bincode::error::DecodeError),
    #[error("handshake magic mismatch: expected {expected:?}, got {got:?}")]
    BadMagic { expected: [u8; 4], got: [u8; 4] },
    #[error("protocol version mismatch: peer={peer} ours={ours}")]
    BadVersion { peer: u32, ours: u32 },
    #[error("frame too large: {0} bytes")]
    FrameTooLarge(u32),
}

/// Maximum bincode payload size we'll accept on a single frame. Mostly a
/// guard against a misbehaving / malicious peer; legitimate cmds are
/// well under 64 KiB.
pub const MAX_FRAME_BYTES: u32 = 64 * 1024;

pub fn write_frame<W: std::io::Write, T: Serialize>(
    w: &mut W,
    msg: &T,
) -> Result<(), ProtocolError> {
    let bytes = bincode::serde::encode_to_vec(msg, bincode::config::standard())?;
    let len = u32::try_from(bytes.len()).map_err(|_| ProtocolError::FrameTooLarge(u32::MAX))?;
    if len > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(len));
    }
    w.write_all(&len.to_be_bytes())?;
    w.write_all(&bytes)?;
    Ok(())
}

pub fn read_frame<R: std::io::Read, T: for<'de> Deserialize<'de>>(
    r: &mut R,
) -> Result<T, ProtocolError> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge(len));
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    let (msg, _) = bincode::serde::decode_from_slice(&buf, bincode::config::standard())?;
    Ok(msg)
}

pub fn write_handshake<W: std::io::Write>(w: &mut W) -> Result<(), ProtocolError> {
    w.write_all(&HANDSHAKE_MAGIC)?;
    w.write_all(&PROTOCOL_VERSION.to_be_bytes())?;
    Ok(())
}

pub fn read_handshake<R: std::io::Read>(r: &mut R) -> Result<(), ProtocolError> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if magic != HANDSHAKE_MAGIC {
        return Err(ProtocolError::BadMagic {
            expected: HANDSHAKE_MAGIC,
            got: magic,
        });
    }
    let mut version = [0u8; 4];
    r.read_exact(&mut version)?;
    let peer = u32::from_be_bytes(version);
    if peer != PROTOCOL_VERSION {
        return Err(ProtocolError::BadVersion {
            peer,
            ours: PROTOCOL_VERSION,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trip_request_response() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &Request::Ping).unwrap();
        write_frame(&mut buf, &Request::Alloc { size: 1024 }).unwrap();
        write_frame(
            &mut buf,
            &Request::WriteState {
                key: "hp".into(),
                value: vec![1, 2, 3, 4],
            },
        )
        .unwrap();

        let mut cur = Cursor::new(buf);
        let r1: Request = read_frame(&mut cur).unwrap();
        let r2: Request = read_frame(&mut cur).unwrap();
        let r3: Request = read_frame(&mut cur).unwrap();
        assert_eq!(r1, Request::Ping);
        assert_eq!(r2, Request::Alloc { size: 1024 });
        assert_eq!(
            r3,
            Request::WriteState {
                key: "hp".into(),
                value: vec![1, 2, 3, 4],
            }
        );
    }

    #[test]
    fn handshake_round_trip() {
        let mut buf = Vec::new();
        write_handshake(&mut buf).unwrap();
        let mut cur = Cursor::new(buf);
        read_handshake(&mut cur).unwrap();
    }

    #[test]
    fn handshake_rejects_bad_magic() {
        let bad = b"XXXX\x00\x00\x00\x01";
        let mut cur = Cursor::new(bad.to_vec());
        let err = read_handshake(&mut cur).unwrap_err();
        assert!(matches!(err, ProtocolError::BadMagic { .. }));
    }

    #[test]
    fn handshake_rejects_bad_version() {
        let mut bad = HANDSHAKE_MAGIC.to_vec();
        bad.extend_from_slice(&999_u32.to_be_bytes());
        let mut cur = Cursor::new(bad);
        let err = read_handshake(&mut cur).unwrap_err();
        assert!(matches!(err, ProtocolError::BadVersion { peer: 999, .. }));
    }

    #[test]
    fn oversized_frame_is_rejected() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(MAX_FRAME_BYTES + 1).to_be_bytes());
        let mut cur = Cursor::new(buf);
        let err: Result<Request, _> = read_frame(&mut cur);
        assert!(matches!(err, Err(ProtocolError::FrameTooLarge(_))));
    }

    #[test]
    fn socket_path_includes_pid() {
        assert_eq!(
            socket_path_for(1234),
            std::path::PathBuf::from("/tmp/cheat-runtime-1234.sock")
        );
    }
}
