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

/// Type tag for [`WireValue`] payloads — the wire-format mirror of
/// `cheat_runtime::manifest::VType`. Replicated here so `tatu-bridge`
/// (cross-compiled to `x86_64-pc-windows-gnu`, no nix dep) and the
/// Linux tracker speak the same vocabulary without importing
/// `cheat-runtime`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WireVType {
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
}

impl WireVType {
    pub const fn size_bytes(self) -> usize {
        match self {
            WireVType::U32 | WireVType::I32 | WireVType::F32 => 4,
            WireVType::U64 | WireVType::I64 | WireVType::F64 => 8,
        }
    }
}

/// Type-tagged numeric value — wire-format mirror of
/// `cheat_runtime::chain::Value`. See [`WireVType`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum WireValue {
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl WireValue {
    pub const fn vtype(&self) -> WireVType {
        match self {
            WireValue::U32(_) => WireVType::U32,
            WireValue::I32(_) => WireVType::I32,
            WireValue::U64(_) => WireVType::U64,
            WireValue::I64(_) => WireVType::I64,
            WireValue::F32(_) => WireVType::F32,
            WireValue::F64(_) => WireVType::F64,
        }
    }

    /// Little-endian serialised form. Length matches `vtype().size_bytes()`.
    pub fn to_le_bytes(self) -> Vec<u8> {
        match self {
            WireValue::U32(v) => v.to_le_bytes().to_vec(),
            WireValue::I32(v) => v.to_le_bytes().to_vec(),
            WireValue::U64(v) => v.to_le_bytes().to_vec(),
            WireValue::I64(v) => v.to_le_bytes().to_vec(),
            WireValue::F32(v) => v.to_le_bytes().to_vec(),
            WireValue::F64(v) => v.to_le_bytes().to_vec(),
        }
    }

    /// Decode an LE slice of the right width into a typed value. Returns
    /// `None` if `bytes.len() != vtype.size_bytes()`.
    pub fn from_le_bytes(vtype: WireVType, bytes: &[u8]) -> Option<Self> {
        if bytes.len() != vtype.size_bytes() {
            return None;
        }
        Some(match vtype {
            WireVType::U32 => WireValue::U32(u32::from_le_bytes(bytes.try_into().ok()?)),
            WireVType::I32 => WireValue::I32(i32::from_le_bytes(bytes.try_into().ok()?)),
            WireVType::U64 => WireValue::U64(u64::from_le_bytes(bytes.try_into().ok()?)),
            WireVType::I64 => WireValue::I64(i64::from_le_bytes(bytes.try_into().ok()?)),
            WireVType::F32 => WireValue::F32(f32::from_le_bytes(bytes.try_into().ok()?)),
            WireVType::F64 => WireValue::F64(f64::from_le_bytes(bytes.try_into().ok()?)),
        })
    }
}

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

    // ---- Phase 4 (#106) — Win32 bridge in-process primitives -----------
    /// Scan an AOB (array-of-bytes) pattern across the target. If
    /// `module` is `Some`, restricts the scan to that module's loaded
    /// range; otherwise sweeps every R/X region of the target. Pattern
    /// syntax: pairs of hex with `??` wildcards, whitespace ignored —
    /// e.g. `"48 8B ?? 24 ?? E8"`.
    AobScan { module: Option<String>, pattern: String },
    /// Write `bytes` at `addr` after lifting page protection. If
    /// `suspend_threads` is true, every thread of the target is
    /// suspended before the write and resumed after — the same
    /// atomicity guard CE's autoassembler uses.
    PatchBytes {
        addr: u64,
        bytes: Vec<u8>,
        suspend_threads: bool,
    },
    /// `VirtualAllocEx` against the target. `hint` lets Win32 pick a
    /// region near the hint (handy for jmp-rel32 codecaves);
    /// `executable == true` selects `PAGE_EXECUTE_READWRITE` versus
    /// `PAGE_READWRITE`.
    RemoteAlloc {
        hint: Option<u64>,
        size: u64,
        executable: bool,
    },
    /// `VirtualFreeEx` with `MEM_RELEASE`. Mirror of [`Request::RemoteAlloc`].
    RemoteFree { addr: u64 },
    /// Resolve a pointer chain: starting from `base`, walk `offsets` in
    /// reverse (CE convention — see `cheat_runtime::chain::walk_chain`).
    /// Returns the final pointer address (no value read).
    WalkChain { base: u64, offsets: Vec<u64> },
    /// Walk the chain and read the typed value at the final address.
    ReadChainValue {
        base: u64,
        offsets: Vec<u64>,
        vtype: WireVType,
    },
    /// Walk the chain and write `value` at the final address.
    WriteChainValue {
        base: u64,
        offsets: Vec<u64>,
        value: WireValue,
    },

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

    // ---- Phase 4 (#106) — Win32 bridge in-process primitives -----------
    AobScan { matches: Vec<u64> },
    PatchBytes,
    RemoteAlloc { addr: u64 },
    RemoteFreed,
    WalkChain { addr: u64 },
    ChainValue { value: WireValue },
    ChainWritten,

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

    #[test]
    fn wire_value_roundtrip_le_bytes_per_vtype() {
        let cases: &[(WireValue, &[u8])] = &[
            (WireValue::U32(0x1122_3344), &[0x44, 0x33, 0x22, 0x11]),
            (WireValue::I32(-1), &[0xff, 0xff, 0xff, 0xff]),
            (
                WireValue::U64(0x0102_0304_0506_0708),
                &[0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01],
            ),
            (
                WireValue::F32(1.0),
                &1.0_f32.to_le_bytes(),
            ),
            (
                WireValue::F64(-2.5),
                &(-2.5_f64).to_le_bytes(),
            ),
        ];
        for (value, expected) in cases {
            assert_eq!(value.to_le_bytes(), *expected, "{value:?} → LE bytes");
            let parsed = WireValue::from_le_bytes(value.vtype(), expected)
                .expect("matched width must decode");
            assert_eq!(parsed, *value, "round-trip through LE bytes");
            assert_eq!(value.vtype().size_bytes(), expected.len());
        }
    }

    #[test]
    fn wire_value_from_le_bytes_rejects_wrong_width() {
        assert!(WireValue::from_le_bytes(WireVType::U32, &[1, 2, 3]).is_none());
        assert!(WireValue::from_le_bytes(WireVType::U64, &[1; 9]).is_none());
    }

    #[test]
    fn phase4_requests_roundtrip_over_frames() {
        let frames: &[Request] = &[
            Request::AobScan {
                module: Some("game.exe".into()),
                pattern: "48 8B ?? 24".into(),
            },
            Request::PatchBytes {
                addr: 0x1400_0000,
                bytes: vec![0x90, 0x90, 0xC3],
                suspend_threads: true,
            },
            Request::RemoteAlloc {
                hint: Some(0x1400_0000),
                size: 4096,
                executable: true,
            },
            Request::RemoteFree {
                addr: 0x1400_1000,
            },
            Request::WalkChain {
                base: 0x1400_0000,
                offsets: vec![0x30, 0x8B8, 0x2D0],
            },
            Request::ReadChainValue {
                base: 0x1400_0000,
                offsets: vec![0x30],
                vtype: WireVType::F32,
            },
            Request::WriteChainValue {
                base: 0x1400_0000,
                offsets: vec![],
                value: WireValue::I64(-12345),
            },
        ];

        let mut buf = Vec::new();
        for req in frames {
            write_frame(&mut buf, req).unwrap();
        }
        let mut cur = Cursor::new(buf);
        for expected in frames {
            let got: Request = read_frame(&mut cur).unwrap();
            assert_eq!(&got, expected);
        }
    }

    #[test]
    fn phase4_responses_roundtrip_over_frames() {
        let frames: &[Response] = &[
            Response::AobScan {
                matches: vec![0x1400_1000, 0x1400_2000],
            },
            Response::PatchBytes,
            Response::RemoteAlloc {
                addr: 0x1400_5000,
            },
            Response::RemoteFreed,
            Response::WalkChain {
                addr: 0x1400_DEAD,
            },
            Response::ChainValue {
                value: WireValue::U32(42),
            },
            Response::ChainWritten,
        ];

        let mut buf = Vec::new();
        for resp in frames {
            write_frame(&mut buf, resp).unwrap();
        }
        let mut cur = Cursor::new(buf);
        for expected in frames {
            let got: Response = read_frame(&mut cur).unwrap();
            assert_eq!(&got, expected);
        }
    }
}
