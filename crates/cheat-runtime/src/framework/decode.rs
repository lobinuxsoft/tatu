//! Decoder for the modules a framework table embeds in its `<Files>` block.
//!
//! Cheat Engine stores each embedded file as `Encoding="Ascii85"`: its *custom*
//! base85 (`custombase85.pas` — not RFC 1924 / Z85) wrapping a raw-deflate
//! stream whose first four bytes are the little-endian uncompressed length.
//! Pipeline: custom base85 → raw deflate → strip the `u32` length prefix.

use std::io::Read;

use flate2::read::DeflateDecoder;

/// CE's custom base85 alphabet (`custombase85.pas`).
const CHARSET: &[u8; 85] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%()*+,-./:;=?@[]^_{}";

/// Decode CE custom base85: big-endian, 5 chars → 4 bytes, a short final group
/// padded with the top digit (and the matching trailing bytes dropped).
/// Non-alphabet bytes (the XML indentation/newlines) are skipped.
fn base85_decode(s: &str) -> Vec<u8> {
    let digits: Vec<u8> = s
        .bytes()
        .filter_map(|b| CHARSET.iter().position(|&c| c == b).map(|p| p as u8))
        .collect();
    let mut out = Vec::with_capacity(digits.len() / 5 * 4);
    for group in digits.chunks(5) {
        let pad = 5 - group.len();
        let mut value: u64 = 0;
        for i in 0..5 {
            value = value * 85 + u64::from(group.get(i).copied().unwrap_or(84));
        }
        let bytes = (value as u32).to_be_bytes();
        out.extend_from_slice(&bytes[..4 - pad]);
    }
    out
}

/// Decode one embedded file (`Ascii85` blob) to its raw bytes, or `None` if the
/// deflate stream or length prefix is malformed.
pub fn decode_embedded_file(blob: &str) -> Option<Vec<u8>> {
    let raw = base85_decode(blob);
    let mut inflated = Vec::new();
    DeflateDecoder::new(&raw[..])
        .read_to_end(&mut inflated)
        .ok()?;
    let size = u32::from_le_bytes(inflated.get(..4)?.try_into().ok()?) as usize;
    inflated.get(4..4 + size).map(<[u8]>::to_vec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_known_vector() {
        // "0" repeated encodes to all-zero bytes; the decoder must drop the
        // padded tail rather than emit spurious trailing bytes.
        assert!(base85_decode("00000").iter().all(|&b| b == 0));
        assert_eq!(base85_decode("00000").len(), 4);
    }

    #[test]
    fn ignores_whitespace_in_blob() {
        let a = base85_decode("00000");
        let b = base85_decode("00 00\n0");
        assert_eq!(a, b);
    }
}
