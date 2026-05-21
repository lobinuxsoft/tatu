//! AOB (array-of-bytes) pattern matching — the in-bridge port of
//! `cheat_runtime::scanner`. Pure Rust, no Win32 dependency, so the
//! parser and scanning kernel build on Linux for unit tests; the
//! remote-process scanner lives in `super::win` (Win32-only).
//!
//! Pattern syntax mirrors CE's autoassembler `aobscan`:
//!
//! - Hex pairs separated by whitespace: `"48 8B"` matches bytes `[0x48,
//!   0x8B]`.
//! - `??` (or `?`) as a wildcard for any byte: `"48 8B ?? 24"`.
//! - Case-insensitive; whitespace is the only token separator.
//!
//! The scan kernel uses memchr against the first literal byte for the
//! fast path, then verifies the masked tail. Same algorithm
//! `cheat-runtime`'s scanner uses against `process_vm_readv` pages;
//! this one operates on host-allocated buffers filled by
//! `ReadProcessMemory` (see [`super::remote_mem`]).

use memchr::memmem;

const SCAN_CHUNK_SIZE: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
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
                    bytes.push(
                        u8::from_str_radix(hex, 16).map_err(|_| ParseError::BadToken {
                            token: token.to_string(),
                        })?,
                    );
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

    /// Indices into `haystack` where the pattern matches.
    pub fn scan(&self, haystack: &[u8]) -> Vec<usize> {
        let mut out = Vec::new();
        if haystack.len() < self.bytes.len() {
            return out;
        }

        match self.first_literal_idx {
            Some(literal_idx) => {
                let literal_byte = self.bytes[literal_idx];
                // Slide a memmem of one byte starting at the position
                // where that first literal would land inside haystack.
                let search_end = haystack
                    .len()
                    .saturating_sub(self.bytes.len() - literal_idx);
                let needle = [literal_byte];
                for hit in memmem::find_iter(&haystack[literal_idx..=search_end], &needle) {
                    let candidate_start = hit;
                    if self.matches_at(haystack, candidate_start) {
                        out.push(candidate_start);
                    }
                }
            }
            None => {
                // All-wildcard pattern is degenerate — match at every
                // offset where the pattern fits.
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

pub const fn scan_chunk_size() -> usize {
    SCAN_CHUNK_SIZE
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
        assert_eq!(p.first_literal_idx, Some(0));
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
        assert_eq!(a.bytes, b.bytes);
    }

    #[test]
    fn parse_accepts_short_wildcard_marker() {
        let p = Pattern::parse("48 ? 05").unwrap();
        assert_eq!(p.len(), 3);
    }

    #[test]
    fn parse_rejects_empty() {
        assert!(matches!(Pattern::parse(""), Err(ParseError::Empty)));
        assert!(matches!(Pattern::parse("   "), Err(ParseError::Empty)));
    }

    #[test]
    fn parse_rejects_garbage_token() {
        assert!(matches!(
            Pattern::parse("48 ZZ"),
            Err(ParseError::BadToken { .. })
        ));
        assert!(matches!(
            Pattern::parse("4"),
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
    fn scan_does_not_match_when_pattern_longer_than_haystack() {
        let pat = Pattern::parse("48 8B 05 25 30").unwrap();
        assert!(pat.scan(b"\x48\x8B").is_empty());
    }
}
