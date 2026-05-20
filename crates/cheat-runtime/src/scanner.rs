//! Array-of-bytes (AOB) scanner with mask support.
//!
//! Pattern syntax (CE Auto-Assembler convention): hex byte pairs separated
//! by whitespace, with `??` (or `?`) marking wildcard positions:
//!
//! ```text
//! 6F 4C ?? 4A 88 D4 22 ?? F2 DC 64 C7 0B
//! ```
//!
//! The scanner exposes two entry points:
//! - [`scan`]: pure function over an in-memory haystack.
//! - [`scan_in_process`]: reads a target process's [`MemoryRegion`] in 4 MiB
//!   chunks with a `pattern.len() - 1` byte overlap so that patterns
//!   straddling chunk boundaries are still matched.
//!
//! The fast path uses `memchr` (SIMD) to find candidates for the first
//! literal byte of the pattern; the remaining (possibly masked) bytes are
//! verified in a tight loop. On a 100 MiB synthetic buffer this completes
//! in well under 500 ms on a modern x86_64 CPU.

use memchr::memchr;
use nix::unistd::Pid;

use crate::maps::MemoryRegion;
use crate::memory::{self, RuntimeError};

const SCAN_CHUNK_SIZE: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    bytes: Vec<u8>,
    mask: Vec<bool>,
    first_literal_idx: Option<usize>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("empty pattern")]
    Empty,
    #[error("invalid token {0:?}; expected two hex digits or '??'")]
    InvalidToken(String),
}

impl Pattern {
    pub fn parse(s: &str) -> Result<Self, ParseError> {
        let mut bytes = Vec::new();
        let mut mask = Vec::new();
        for token in s.split_whitespace() {
            if token == "??" || token == "?" {
                bytes.push(0);
                mask.push(false);
                continue;
            }
            if token.len() != 2 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(ParseError::InvalidToken(token.to_string()));
            }
            let byte = u8::from_str_radix(token, 16)
                .map_err(|_| ParseError::InvalidToken(token.to_string()))?;
            bytes.push(byte);
            mask.push(true);
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
}

/// Scan `haystack` for every offset where `pattern` matches.
///
/// Returns the starting positions in `haystack`. Empty pattern or pattern
/// longer than haystack both yield an empty result without scanning.
pub fn scan(haystack: &[u8], pattern: &Pattern) -> Vec<usize> {
    let plen = pattern.len();
    if plen == 0 || haystack.len() < plen {
        return Vec::new();
    }

    let Some(first_lit) = pattern.first_literal_idx else {
        // Pattern is all-wildcards: matches at every valid start position.
        return (0..=haystack.len() - plen).collect();
    };

    let first_byte = pattern.bytes[first_lit];
    let max_start = haystack.len() - plen;
    let mut out = Vec::new();
    let mut cursor = first_lit;
    let upper = max_start + first_lit; // inclusive upper bound for memchr's slice
    while cursor <= upper {
        let Some(rel) = memchr(first_byte, &haystack[cursor..=upper]) else {
            break;
        };
        let hit = cursor + rel;
        let base = hit - first_lit;
        if matches_at(haystack, base, pattern) {
            out.push(base);
        }
        cursor = hit + 1;
    }
    out
}

fn matches_at(haystack: &[u8], base: usize, pattern: &Pattern) -> bool {
    for (k, (&pb, &m)) in pattern.bytes.iter().zip(pattern.mask.iter()).enumerate() {
        if m && haystack[base + k] != pb {
            return false;
        }
    }
    true
}

/// Scan a live process region for `pattern`, returning absolute addresses.
///
/// Reads the region in 4 MiB chunks with an overlap of `pattern.len() - 1`
/// bytes so that hits straddling chunk boundaries are matched without
/// double-counting (matches starting inside the overlap of one chunk are
/// re-found in the head of the next chunk and only kept there).
pub fn scan_in_process(
    pid: Pid,
    region: &MemoryRegion,
    pattern: &Pattern,
) -> Result<Vec<u64>, RuntimeError> {
    let plen = pattern.len();
    let region_size = region.size() as usize;
    if plen == 0 || plen > region_size {
        return Ok(Vec::new());
    }
    let overlap = plen - 1;
    let mut out = Vec::new();
    let mut chunk_start: usize = 0;
    while chunk_start < region_size {
        let is_last = chunk_start + SCAN_CHUNK_SIZE >= region_size;
        let read_len = if is_last {
            region_size - chunk_start
        } else {
            SCAN_CHUNK_SIZE + overlap
        };
        // Use the permissive read: a region listed `r` in /proc/<pid>/maps
        // can still span an unmapped sub-page (kernel quirks, lazy
        // mappings). `process_vm_readv` returns the prefix it could
        // deliver and we keep scanning that prefix — CE's
        // `ceserver/api.c` ignores short-read returns the same way.
        let chunk = memory::read_bytes_partial(pid, region.start + chunk_start as u64, read_len)?;
        if chunk.is_empty() {
            break;
        }
        let keep_until = if is_last || chunk.len() < read_len {
            chunk.len()
        } else {
            SCAN_CHUNK_SIZE
        };
        for m in scan(&chunk, pattern) {
            if m < keep_until {
                out.push(region.start + (chunk_start + m) as u64);
            }
        }
        if is_last {
            break;
        }
        chunk_start += SCAN_CHUNK_SIZE;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maps::{MemoryRegion, Perms};
    use std::path::PathBuf;
    use std::time::Instant;

    fn region_over(buf: &[u8]) -> MemoryRegion {
        MemoryRegion {
            start: buf.as_ptr() as u64,
            end: buf.as_ptr() as u64 + buf.len() as u64,
            perms: Perms {
                read: true,
                write: true,
                execute: false,
                shared: false,
            },
            offset: 0,
            path: PathBuf::new(),
        }
    }

    #[test]
    fn parse_simple_literal_pattern() {
        let p = Pattern::parse("6F 4C 22").unwrap();
        assert_eq!(p.len(), 3);
        assert_eq!(p.bytes, vec![0x6f, 0x4c, 0x22]);
        assert_eq!(p.mask, vec![true, true, true]);
        assert_eq!(p.first_literal_idx, Some(0));
    }

    #[test]
    fn parse_with_wildcards() {
        let p = Pattern::parse("6F ?? 22 ? AA").unwrap();
        assert_eq!(p.len(), 5);
        assert_eq!(p.mask, vec![true, false, true, false, true]);
        assert_eq!(p.first_literal_idx, Some(0));
    }

    #[test]
    fn parse_leading_wildcards_tracks_first_literal() {
        let p = Pattern::parse("?? ?? 6F").unwrap();
        assert_eq!(p.first_literal_idx, Some(2));
    }

    #[test]
    fn parse_case_insensitive_hex() {
        let p = Pattern::parse("aa BB Cc dD").unwrap();
        assert_eq!(p.bytes, vec![0xaa, 0xbb, 0xcc, 0xdd]);
    }

    #[test]
    fn parse_rejects_empty() {
        assert_eq!(Pattern::parse("   "), Err(ParseError::Empty));
    }

    #[test]
    fn parse_rejects_bad_token() {
        assert!(matches!(
            Pattern::parse("6F 4C ZZ"),
            Err(ParseError::InvalidToken(_))
        ));
        assert!(matches!(
            Pattern::parse("6F F"),
            Err(ParseError::InvalidToken(_))
        ));
    }

    #[test]
    fn scan_finds_single_match() {
        let haystack = b"....\x6f\x4c\x22....";
        let p = Pattern::parse("6F 4C 22").unwrap();
        assert_eq!(scan(haystack, &p), vec![4]);
    }

    #[test]
    fn scan_finds_multiple_matches() {
        let haystack = b"\xAA\xBB\xCC..\xAA\xBB\xCC..\xAA\xBB\xCC";
        let p = Pattern::parse("AA BB CC").unwrap();
        assert_eq!(scan(haystack, &p), vec![0, 5, 10]);
    }

    #[test]
    fn scan_respects_wildcards() {
        let haystack = b"\xAA\x00\xCC\xAA\xFF\xCC";
        let p = Pattern::parse("AA ?? CC").unwrap();
        assert_eq!(scan(haystack, &p), vec![0, 3]);
    }

    #[test]
    fn scan_no_match_returns_empty() {
        let haystack = b"\x01\x02\x03\x04";
        let p = Pattern::parse("AA BB").unwrap();
        assert!(scan(haystack, &p).is_empty());
    }

    #[test]
    fn scan_pattern_longer_than_haystack() {
        let haystack = b"\xAA\xBB";
        let p = Pattern::parse("AA BB CC DD").unwrap();
        assert!(scan(haystack, &p).is_empty());
    }

    #[test]
    fn scan_all_wildcards_matches_everywhere() {
        let haystack = b"hello";
        let p = Pattern::parse("?? ?? ??").unwrap();
        assert_eq!(scan(haystack, &p), vec![0, 1, 2]);
    }

    #[test]
    fn scan_leading_wildcards_aligned_correctly() {
        let haystack = b"AB\x10\x20\x30CD";
        let p = Pattern::parse("?? ?? 10 20 30").unwrap();
        assert_eq!(scan(haystack, &p), vec![0]);
    }

    #[test]
    fn scan_in_process_against_own_buffer() {
        let mut buf = vec![0u8; 1024];
        buf[100..103].copy_from_slice(&[0xde, 0xad, 0xbe]);
        buf[500..503].copy_from_slice(&[0xde, 0xff, 0xbe]);
        let region = region_over(&buf);
        let p = Pattern::parse("DE ?? BE").unwrap();

        let hits = scan_in_process(Pid::this(), &region, &p).unwrap();
        let base = buf.as_ptr() as u64;
        assert_eq!(hits, vec![base + 100, base + 500]);
    }

    #[test]
    fn scan_in_process_finds_pattern_across_chunk_boundary() {
        // Build a 9 MB buffer with the needle straddling the 4 MiB chunk boundary.
        let mut buf = vec![0u8; 9 * 1024 * 1024];
        let needle = b"\x11\x22\x33\x44\x55\x66\x77\x88";
        let split = SCAN_CHUNK_SIZE - 3; // place 3 bytes before boundary, 5 after
        buf[split..split + needle.len()].copy_from_slice(needle);
        let region = region_over(&buf);
        let p = Pattern::parse("11 22 33 44 55 66 77 88").unwrap();

        let hits = scan_in_process(Pid::this(), &region, &p).unwrap();
        assert_eq!(hits.len(), 1, "expected exactly one match across boundary");
        assert_eq!(hits[0], buf.as_ptr() as u64 + split as u64);
    }

    #[test]
    fn scan_100mb_under_500ms() {
        let mut haystack = vec![0u8; 100 * 1024 * 1024];
        let where_at = 50 * 1024 * 1024;
        haystack[where_at..where_at + 6].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        let p = Pattern::parse("AA BB ?? DD EE FF").unwrap();

        let started = Instant::now();
        let hits = scan(&haystack, &p);
        let elapsed = started.elapsed();
        eprintln!("scan 100 MiB ({:?}): {} hits", elapsed, hits.len());

        assert_eq!(hits, vec![where_at]);
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "scan was too slow: {elapsed:?}"
        );
    }
}
