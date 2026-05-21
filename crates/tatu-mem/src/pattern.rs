//! AOB pattern parser + scanner. Two layers:
//!
//! - [`Pattern`] + [`Pattern::parse`] + [`Pattern::scan`]: pure-Rust
//!   over a host-owned `&[u8]`. No backend coupling. Algorithm: memchr
//!   on the first literal byte for the fast path, masked-byte
//!   verification of each candidate.
//! - [`scan_range`] / [`SCAN_CHUNK_SIZE`]: generic over any
//!   [`crate::MemoryAccess`]. Chunks remote reads at 4 MiB with a
//!   `pattern.len() - 1` overlap so a match crossing the chunk
//!   boundary still hits exactly once.
//!
//! Pattern syntax (CE-compatible):
//! - Hex pairs `"48 8B"` match exact bytes `0x48 0x8B`.
//! - `"??"` (or `"?"`) is a wildcard for any byte.
//! - Case-insensitive; whitespace is the only token separator.

use crate::MemoryAccess;
use memchr::memmem;

pub const SCAN_CHUNK_SIZE: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    bytes: Vec<u8>,
    mask: Vec<bool>,
    first_literal_idx: Option<usize>,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("empty pattern")]
    Empty,
    #[error("token {token:?} is not a valid hex byte or wildcard")]
    BadToken { token: String },
}

impl Pattern {
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        let mut bytes = Vec::new();
        let mut mask = Vec::new();
        for token in input.split_ascii_whitespace() {
            match token {
                "??" | "?" => {
                    bytes.push(0);
                    mask.push(false);
                }
                hex if hex.len() == 2 && hex.chars().all(|c| c.is_ascii_hexdigit()) => {
                    bytes.push(u8::from_str_radix(hex, 16).map_err(|_| ParseError::BadToken {
                        token: token.to_string(),
                    })?);
                    mask.push(true);
                }
                _ => {
                    return Err(ParseError::BadToken {
                        token: token.to_string(),
                    });
                }
            }
        }
        if bytes.is_empty() {
            return Err(ParseError::Empty);
        }
        let first_literal_idx = mask.iter().position(|&m| m);
        Ok(Self {
            bytes,
            mask,
            first_literal_idx,
        })
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Offsets into `haystack` where the pattern matches.
    pub fn scan(&self, haystack: &[u8]) -> Vec<usize> {
        let mut out = Vec::new();
        if haystack.len() < self.bytes.len() {
            return out;
        }

        match self.first_literal_idx {
            Some(literal_idx) => {
                let literal_byte = self.bytes[literal_idx];
                let search_end = haystack.len().saturating_sub(self.bytes.len() - literal_idx);
                let needle = [literal_byte];
                for hit in memmem::find_iter(&haystack[literal_idx..=search_end], &needle) {
                    if self.matches_at(haystack, hit) {
                        out.push(hit);
                    }
                }
            }
            None => {
                for i in 0..=haystack.len() - self.bytes.len() {
                    out.push(i);
                }
            }
        }
        out
    }

    fn matches_at(&self, haystack: &[u8], start: usize) -> bool {
        if start + self.bytes.len() > haystack.len() {
            return false;
        }
        for (i, (&literal, &is_literal)) in self.bytes.iter().zip(self.mask.iter()).enumerate() {
            if is_literal && haystack[start + i] != literal {
                return false;
            }
        }
        true
    }
}

/// Scan `[base, base + size)` in the remote address space for
/// `pattern`. Reads are chunked at [`SCAN_CHUNK_SIZE`] with an
/// overlap of `pattern.len() - 1` bytes between consecutive chunks
/// so a match straddling a boundary is found exactly once. Short
/// reads (unmapped hole inside the range) advance past whatever
/// returned without overlap.
pub fn scan_range<M: MemoryAccess>(
    mem: &mut M,
    base: u64,
    size: u64,
    pattern: &Pattern,
) -> Vec<u64> {
    let mut out = Vec::new();
    if size < pattern.len() as u64 || pattern.is_empty() {
        return out;
    }
    let chunk = SCAN_CHUNK_SIZE;
    let overlap = pattern.len().saturating_sub(1);
    let mut offset: u64 = 0;
    while offset < size {
        let remaining = size - offset;
        let want = (chunk as u64).min(remaining) as usize;
        let bytes = mem.read_partial(base + offset, want);
        if bytes.is_empty() {
            offset = offset.saturating_add(want as u64);
            continue;
        }
        for hit in pattern.scan(&bytes) {
            out.push(base + offset + hit as u64);
        }
        if bytes.len() < want {
            offset += bytes.len() as u64;
            continue;
        }
        if remaining <= chunk as u64 {
            break;
        }
        offset += (chunk - overlap) as u64;
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_literal_pattern() {
        let p = Pattern::parse("48 8B 05").unwrap();
        assert_eq!(p.len(), 3);
        assert_eq!(p.first_literal_idx, Some(0));
    }

    #[test]
    fn parses_pattern_with_wildcards() {
        let p = Pattern::parse("48 ?? 05 ??").unwrap();
        assert_eq!(p.len(), 4);
    }

    #[test]
    fn parses_leading_wildcards_tracks_first_literal() {
        let p = Pattern::parse("?? ?? 48 8B").unwrap();
        assert_eq!(p.first_literal_idx, Some(2));
    }

    #[test]
    fn parse_is_case_insensitive() {
        let a = Pattern::parse("aA bB").unwrap();
        let b = Pattern::parse("AA BB").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn parse_accepts_short_wildcard_marker() {
        let p = Pattern::parse("48 ? 05").unwrap();
        assert_eq!(p.len(), 3);
    }

    #[test]
    fn parse_rejects_empty() {
        assert!(matches!(Pattern::parse(""), Err(ParseError::Empty)));
    }

    #[test]
    fn parse_rejects_garbage_token() {
        assert!(matches!(
            Pattern::parse("48 ZZ"),
            Err(ParseError::BadToken { .. })
        ));
    }

    #[test]
    fn scan_finds_single_match() {
        let pat = Pattern::parse("DE AD BE EF").unwrap();
        let hay = b"\x00\x00\xDE\xAD\xBE\xEF\x00";
        assert_eq!(pat.scan(hay), vec![2]);
    }

    #[test]
    fn scan_finds_multiple_matches() {
        let pat = Pattern::parse("AA BB").unwrap();
        let hay = b"\xAA\xBB\x00\xAA\xBB\x00\xAA\xBB";
        assert_eq!(pat.scan(hay), vec![0, 3, 6]);
    }

    #[test]
    fn scan_respects_wildcards() {
        let pat = Pattern::parse("48 ?? 05").unwrap();
        let hay = b"\x48\xAB\x05\x48\x99\x05\x48\x00\x06";
        assert_eq!(pat.scan(hay), vec![0, 3]);
    }

    #[test]
    fn scan_with_leading_wildcards_anchors_on_first_literal() {
        let pat = Pattern::parse("?? ?? 48 8B").unwrap();
        let hay = b"\x00\x00\x48\x8B\x99\x99\xAA\x48\x8B";
        assert_eq!(pat.scan(hay), vec![0, 5]);
    }

    #[test]
    fn scan_returns_empty_on_no_match() {
        let pat = Pattern::parse("AA BB").unwrap();
        assert!(pat.scan(b"\x01\x02\x03\x04").is_empty());
    }

    #[test]
    fn scan_returns_empty_when_pattern_longer_than_haystack() {
        let pat = Pattern::parse("AA BB CC DD").unwrap();
        assert!(pat.scan(b"\xAA\xBB").is_empty());
    }

    #[test]
    fn scan_all_wildcards_matches_every_position() {
        let pat = Pattern::parse("?? ?? ??").unwrap();
        assert_eq!(pat.scan(b"hello"), vec![0, 1, 2]);
    }

    #[test]
    fn scan_range_against_in_memory_buffer() {
        // Backend that mirrors a flat Vec<u8> at base = 0x1000.
        struct InMemBackend {
            base: u64,
            data: Vec<u8>,
        }

        #[derive(Debug, thiserror::Error)]
        #[error("oob")]
        struct Oob;

        impl MemoryAccess for InMemBackend {
            type Error = Oob;
            fn read(&mut self, addr: u64, len: usize) -> Result<Vec<u8>, Oob> {
                let start = addr.checked_sub(self.base).ok_or(Oob)? as usize;
                self.data
                    .get(start..start + len)
                    .map(<[u8]>::to_vec)
                    .ok_or(Oob)
            }
            fn read_partial(&mut self, addr: u64, len: usize) -> Vec<u8> {
                let start = match addr.checked_sub(self.base) {
                    Some(v) => v as usize,
                    None => return Vec::new(),
                };
                self.data
                    .get(start..(start + len).min(self.data.len()))
                    .unwrap_or(&[])
                    .to_vec()
            }
            fn write(&mut self, _: u64, _: &[u8]) -> Result<(), Oob> {
                unimplemented!()
            }
        }

        let pat = Pattern::parse("DE AD ?? EF").unwrap();
        let mut data = vec![0u8; 64];
        data[10..14].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        data[40..44].copy_from_slice(&[0xDE, 0xAD, 0x99, 0xEF]);
        let mut mem = InMemBackend { base: 0x1000, data };

        let hits = scan_range(&mut mem, 0x1000, 64, &pat);
        assert_eq!(hits, vec![0x1000 + 10, 0x1000 + 40]);
    }
}
