//! Wire types shared by the Linux tracker (`cheat-runtime`) and the
//! in-process Windows DLL backend (`cheat-runtime-dll`).
//!
//! Encoded with bincode 2 + serde, framed on the named-pipe transport
//! described in #102 by a 4-byte little-endian payload-length prefix.
//! Both ends compile against this crate so the wire shape can only
//! drift via a [`PROTO_VERSION`] bump — caught by the handshake reply
//! in [`Request::Ping`] / [`Response::Pong`].

use serde::{Deserialize, Serialize};

/// Wire-protocol version. Bumped whenever any variant changes shape on
/// the wire. The handshake checks this before any real traffic flows
/// so a stale DLL never silently corrupts a newer tracker.
pub const PROTO_VERSION: u32 = 1;

/// One request from the tracker to the in-process DLL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Request {
    /// Heartbeat + version handshake. Always replied with [`Response::Pong`].
    Ping { proto_version: u32 },
}

/// One response from the in-process DLL back to the tracker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Response {
    /// Reply to [`Request::Ping`]. The DLL echoes its own proto version
    /// so the tracker can refuse to talk to an incompatible build.
    Pong { proto_version: u32 },
    /// Closed-shape error. The tracker matches on the variant rather
    /// than parsing a free-form string, so adding a variant is a wire
    /// break (= [`PROTO_VERSION`] bump).
    Err(ProtoError),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, thiserror::Error)]
pub enum ProtoError {
    #[error("protocol version mismatch: tracker speaks {tracker}, DLL speaks {dll}")]
    VersionMismatch { tracker: u32, dll: u32 },
    #[error("the request variant is not implemented in this DLL build")]
    Unimplemented,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// bincode + serde round-trip on every wire shape. Catches accidental
    /// serde-attr drift (`#[serde(rename = "…")]`, tag changes) without
    /// needing a live DLL to talk to.
    #[test]
    fn ping_round_trips() {
        let req = Request::Ping {
            proto_version: PROTO_VERSION,
        };
        let bytes = bincode::serde::encode_to_vec(&req, bincode::config::standard()).unwrap();
        let (back, _): (Request, usize) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn pong_round_trips() {
        let resp = Response::Pong {
            proto_version: PROTO_VERSION,
        };
        let bytes = bincode::serde::encode_to_vec(&resp, bincode::config::standard()).unwrap();
        let (back, _): (Response, usize) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(resp, back);
    }

    #[test]
    fn version_mismatch_carries_both_numbers() {
        let err = ProtoError::VersionMismatch { tracker: 2, dll: 1 };
        let resp = Response::Err(err);
        let bytes = bincode::serde::encode_to_vec(&resp, bincode::config::standard()).unwrap();
        let (back, _): (Response, usize) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(resp, back);
    }
}
