//! Cheat-Engine-compatible Mono data collector wire protocol.
//!
//! The format mirrors CE's `MonoDataCollector` (`PipeServer.cpp` + `Pipe.cpp`):
//! all integers are little-endian, strings are a `u16` little-endian byte count
//! followed by raw UTF-8 (no NUL terminator). The transport differs — CE uses a
//! named pipe / abstract unix socket, we use a TCP loopback socket — but the
//! message framing is identical so the same client logic ports across.
//!
//! Only the subset needed to resolve `Class:Method -> JIT code address` is
//! modelled. The full CE collector has ~70 commands; we implement the ones on
//! the resolution critical path plus `Terminate` for a clean shutdown.

use std::io::{self, Read, Write};

/// Fixed TCP port the collector listens on (loopback only). CE keys its pipe by
/// PID; we use a fixed port because under Proton the per-game container shares
/// the host network namespace and one game is attached at a time. If this ever
/// needs to be per-game, the collector can write the chosen port to a file in
/// the prefix and tatu can read it.
pub const DEFAULT_PORT: u16 = 0xCECE;

/// Commands understood by the collector. Numeric values match CE's
/// `MONOCMD_*` enum so the wire stays compatible with CE tooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Command {
    /// Locate the Mono runtime in-process and attach the calling thread.
    /// Response: `u64` mono module handle (0 = not found / il2cpp).
    InitMono = 0,
    /// `u64 method` -> `u64` native JIT code address (0 on failure).
    CompileMethod = 10,
    /// `u64 domain (0 = root)` + `u64 address` -> jit info (see handler).
    GetJitInfo = 14,
    /// `u64 image` + `string class` + `string namespace` -> `u64` class (0 = miss).
    FindClass = 15,
    /// `u64 class` + `string method` -> `u64` method (0 = miss).
    FindMethod = 16,
    /// Terminate the current connection's command loop cleanly.
    Terminate = 22,
    /// `u64 image` + `string "NS.Class:Method"` -> `u64` method (0 = miss).
    FindMethodByDesc = 29,
    /// Enumerate loaded images. Response: see [`crate`] handler — `u64 image`
    /// + length-prefixed name, repeated, framed by a leading byte count.
    EnumImages = 49,
}

impl Command {
    /// Map a wire command byte to a [`Command`], or `None` if unsupported.
    pub fn from_u8(byte: u8) -> Option<Self> {
        Some(match byte {
            0 => Self::InitMono,
            10 => Self::CompileMethod,
            14 => Self::GetJitInfo,
            15 => Self::FindClass,
            16 => Self::FindMethod,
            22 => Self::Terminate,
            29 => Self::FindMethodByDesc,
            49 => Self::EnumImages,
            _ => return None,
        })
    }
}

/// Little-endian reads matching CE's `Pipe.cpp` primitives, plus the
/// `u16`-length-prefixed UTF-8 string read from `ReadString`.
pub trait WireRead: Read {
    fn read_u8(&mut self) -> io::Result<u8> {
        let mut b = [0u8; 1];
        self.read_exact(&mut b)?;
        Ok(b[0])
    }

    fn read_u16(&mut self) -> io::Result<u16> {
        let mut b = [0u8; 2];
        self.read_exact(&mut b)?;
        Ok(u16::from_le_bytes(b))
    }

    fn read_u32(&mut self) -> io::Result<u32> {
        let mut b = [0u8; 4];
        self.read_exact(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }

    fn read_u64(&mut self) -> io::Result<u64> {
        let mut b = [0u8; 8];
        self.read_exact(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }

    /// `u16` byte count + that many UTF-8 bytes. Invalid UTF-8 is replaced
    /// lossily rather than erroring — class/method names are ASCII in practice
    /// and a hard error would kill the connection over a cosmetic byte.
    fn read_string(&mut self) -> io::Result<String> {
        let len = self.read_u16()? as usize;
        let mut buf = vec![0u8; len];
        self.read_exact(&mut buf)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
}

impl<R: Read> WireRead for R {}

/// Little-endian writes matching CE's `Pipe.cpp` primitives, plus the
/// `u16`-length-prefixed UTF-8 string write from `WriteString`.
pub trait WireWrite: Write {
    fn write_u8(&mut self, v: u8) -> io::Result<()> {
        self.write_all(&[v])
    }

    fn write_u16(&mut self, v: u16) -> io::Result<()> {
        self.write_all(&v.to_le_bytes())
    }

    fn write_u32(&mut self, v: u32) -> io::Result<()> {
        self.write_all(&v.to_le_bytes())
    }

    fn write_u64(&mut self, v: u64) -> io::Result<()> {
        self.write_all(&v.to_le_bytes())
    }

    /// `u16` byte count + UTF-8 bytes. CE caps names at 512; we match so an
    /// overlong name can't desync the stream against a CE-compatible peer.
    fn write_string(&mut self, s: &str) -> io::Result<()> {
        let bytes = s.as_bytes();
        let len = bytes.len().min(512);
        self.write_u16(len as u16)?;
        self.write_all(&bytes[..len])
    }
}

impl<W: Write> WireWrite for W {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn command_roundtrips_known_bytes() {
        for cmd in [
            Command::InitMono,
            Command::CompileMethod,
            Command::GetJitInfo,
            Command::FindClass,
            Command::FindMethod,
            Command::Terminate,
            Command::FindMethodByDesc,
            Command::EnumImages,
        ] {
            assert_eq!(Command::from_u8(cmd as u8), Some(cmd));
        }
    }

    #[test]
    fn unknown_command_byte_is_none() {
        assert_eq!(Command::from_u8(200), None);
    }

    #[test]
    fn integers_roundtrip_little_endian() {
        let mut buf = Vec::new();
        buf.write_u8(0x12).unwrap();
        buf.write_u16(0x3456).unwrap();
        buf.write_u32(0x789a_bcde).unwrap();
        buf.write_u64(0x0123_4567_89ab_cdef).unwrap();

        // Wire bytes are little-endian.
        assert_eq!(&buf[..3], &[0x12, 0x56, 0x34]);

        let mut cur = Cursor::new(buf);
        assert_eq!(cur.read_u8().unwrap(), 0x12);
        assert_eq!(cur.read_u16().unwrap(), 0x3456);
        assert_eq!(cur.read_u32().unwrap(), 0x789a_bcde);
        assert_eq!(cur.read_u64().unwrap(), 0x0123_4567_89ab_cdef);
    }

    #[test]
    fn string_roundtrips_with_length_prefix() {
        let mut buf = Vec::new();
        buf.write_string("Player.Pistol:Shoot").unwrap();

        // First two bytes are the LE length.
        assert_eq!(u16::from_le_bytes([buf[0], buf[1]]) as usize, 19);

        let mut cur = Cursor::new(buf);
        assert_eq!(cur.read_string().unwrap(), "Player.Pistol:Shoot");
    }

    #[test]
    fn empty_string_roundtrips() {
        let mut buf = Vec::new();
        buf.write_string("").unwrap();
        assert_eq!(buf, vec![0, 0]);

        let mut cur = Cursor::new(buf);
        assert_eq!(cur.read_string().unwrap(), "");
    }

    #[test]
    fn overlong_string_is_capped_at_512() {
        let huge = "a".repeat(1000);
        let mut buf = Vec::new();
        buf.write_string(&huge).unwrap();

        let mut cur = Cursor::new(buf);
        let got = cur.read_string().unwrap();
        assert_eq!(got.len(), 512);
    }
}
