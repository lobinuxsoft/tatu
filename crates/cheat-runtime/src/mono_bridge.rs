//! Native-Linux client for the in-game Mono collector.
//!
//! The other half of the Mono symbol bridge: [`cheat_mono_collector`] runs as a
//! Windows DLL inside a Unity Mono game under Proton and exposes the game's
//! `mono_*` API over a TCP loopback socket. This client connects to it and
//! resolves `Class:Method` cheat-table symbols to JIT-compiled native code
//! addresses, which the executor then uses like any other bound symbol.
//!
//! Resolution mirrors how CE drives its collector: locate Mono, enumerate
//! loaded images, find the method by descriptor in each image until one hits,
//! then JIT-compile it to get the code address. The `+offset` suffix on a
//! symbol is applied by the executor's existing `symbol+offset` handling — this
//! client only resolves the base method address.

use std::io::{self, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use cheat_mono_collector::protocol::{Command, DEFAULT_PORT, WireRead, WireWrite};

/// Default timeout for a single request/response exchange. The collector calls
/// into Mono synchronously, so responses are prompt; a hung collector must not
/// block the caller forever.
const IO_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum MonoError {
    #[error("could not connect to mono collector at {addr}: {source}")]
    Connect { addr: SocketAddr, source: io::Error },
    #[error("mono collector io error: {0}")]
    Io(#[from] io::Error),
    #[error("no mono runtime in target process (il2cpp build, or not loaded yet)")]
    MonoNotFound,
    #[error("mono symbol not resolved: {0}")]
    SymbolNotFound(String),
}

/// A connected session to the collector. One game is attached at a time, so a
/// single connection is reused for all resolutions.
pub struct MonoClient {
    stream: TcpStream,
}

impl MonoClient {
    /// Connect to the collector on `127.0.0.1:DEFAULT_PORT`.
    pub fn connect() -> Result<Self, MonoError> {
        Self::connect_to(SocketAddr::from((Ipv4Addr::LOCALHOST, DEFAULT_PORT)))
    }

    /// Connect to a specific address (used by tests with a mock collector).
    pub fn connect_to(addr: SocketAddr) -> Result<Self, MonoError> {
        let stream = TcpStream::connect_timeout(&addr, IO_TIMEOUT)
            .map_err(|source| MonoError::Connect { addr, source })?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        stream.set_nodelay(true)?;
        Ok(Self { stream })
    }

    /// Resolve a cheat-table Mono symbol to its JIT native code address.
    ///
    /// Accepts `Class:Method`, `Namespace.Class:Method`, and an optional
    /// `[Image]` prefix (`[Assembly-CSharp]Player:Update`) that restricts the
    /// search to that image. Returns the base method address; any `+offset` is
    /// the executor's job.
    pub fn resolve(&mut self, symbol: &str) -> Result<u64, MonoError> {
        if self.init_mono()? == 0 {
            return Err(MonoError::MonoNotFound);
        }

        let (image_filter, desc) = parse_mono_symbol(symbol);
        let images = self.enum_images()?;

        for (handle, name) in &images {
            if let Some(filter) = image_filter
                && !image_matches(name, filter)
            {
                continue;
            }
            let method = self.find_method_by_desc(*handle, desc)?;
            if method == 0 {
                continue;
            }
            let addr = self.compile_method(method)?;
            if addr != 0 {
                return Ok(addr);
            }
        }
        Err(MonoError::SymbolNotFound(symbol.to_string()))
    }

    /// `InitMono`: returns the Mono module handle (0 = not found / il2cpp).
    pub fn init_mono(&mut self) -> Result<u64, MonoError> {
        self.stream.write_u8(Command::InitMono as u8)?;
        self.stream.flush()?;
        Ok(self.stream.read_u64()?)
    }

    /// `EnumImages`: `(handle, name)` for every loaded assembly image.
    pub fn enum_images(&mut self) -> Result<Vec<(u64, String)>, MonoError> {
        self.stream.write_u8(Command::EnumImages as u8)?;
        self.stream.flush()?;
        let count = self.stream.read_u32()? as usize;
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            let handle = self.stream.read_u64()?;
            let name = self.stream.read_string()?;
            out.push((handle, name));
        }
        Ok(out)
    }

    /// `FindMethodByDesc`: resolve `"NS.Class:Method"` in `image` (0 = miss).
    pub fn find_method_by_desc(&mut self, image: u64, desc: &str) -> Result<u64, MonoError> {
        self.stream.write_u8(Command::FindMethodByDesc as u8)?;
        self.stream.write_u64(image)?;
        self.stream.write_string(desc)?;
        self.stream.flush()?;
        Ok(self.stream.read_u64()?)
    }

    /// `CompileMethod`: JIT-compile and return the native code address (0 =
    /// failed / generic method).
    pub fn compile_method(&mut self, method: u64) -> Result<u64, MonoError> {
        self.stream.write_u8(Command::CompileMethod as u8)?;
        self.stream.write_u64(method)?;
        self.stream.flush()?;
        Ok(self.stream.read_u64()?)
    }

    /// Politely tell the collector to close this connection's command loop.
    pub fn terminate(&mut self) -> Result<(), MonoError> {
        self.stream.write_u8(Command::Terminate as u8)?;
        self.stream.flush()?;
        Ok(())
    }
}

/// Heuristic: does `symbol` look like a Mono `Class:Method` (or
/// `[Image]Namespace.Class:Method`) rather than an ELF `module:symbol` or a
/// bare label? Used to decide which unresolved symbols to route through the
/// collector before running a script.
///
/// A Mono descriptor has a `:` whose left side is a managed type name — never a
/// module file (`mono-2.0-bdwgc.dll:fn`, `libfoo.so:sym`) and never a path.
pub fn is_mono_symbol(symbol: &str) -> bool {
    let (_, desc) = parse_mono_symbol(symbol);
    let Some((ty, method)) = desc.split_once(':') else {
        return false;
    };
    if ty.is_empty() || method.is_empty() {
        return false;
    }
    if ty.contains(['/', '\\']) {
        return false;
    }
    let lower = ty.to_ascii_lowercase();
    !(lower.ends_with(".dll") || lower.ends_with(".so") || lower.ends_with(".exe"))
}

/// Split an optional `[Image]` prefix off a Mono symbol, returning
/// `(image_filter, method_descriptor)`. The descriptor is what Mono's
/// `mono_method_desc_new` consumes (`Namespace.Class:Method`).
fn parse_mono_symbol(symbol: &str) -> (Option<&str>, &str) {
    let trimmed = symbol.trim();
    if let Some(rest) = trimmed.strip_prefix('[')
        && let Some(end) = rest.find(']')
    {
        return (Some(&rest[..end]), rest[end + 1..].trim_start());
    }
    (None, trimmed)
}

/// Match an image filter against an image name, tolerating the `.dll` suffix
/// CE tables usually omit (`Assembly-CSharp` vs `Assembly-CSharp.dll`).
fn image_matches(name: &str, filter: &str) -> bool {
    name == filter || name.strip_suffix(".dll") == Some(filter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn is_mono_symbol_classifies_descriptors() {
        // Mono Class:Method forms.
        assert!(is_mono_symbol("Pistol:Shoot"));
        assert!(is_mono_symbol("HPModuleBase:Damage"));
        assert!(is_mono_symbol("UnityEngine.Player:Update"));
        assert!(is_mono_symbol("[Assembly-CSharp]Player:Update"));
        // Not Mono: ELF/PE module:symbol, paths, bare labels.
        assert!(!is_mono_symbol("mono-2.0-bdwgc.dll:mono_compile_method"));
        assert!(!is_mono_symbol("libfoo.so:some_sym"));
        assert!(!is_mono_symbol("game.exe:Foo"));
        assert!(!is_mono_symbol("returnhere"));
        assert!(!is_mono_symbol("newmem"));
        assert!(!is_mono_symbol(""));
    }

    /// Spawn a scripted collector that speaks the real wire protocol (so these
    /// tests exercise the exact serialization the Windows collector uses) and
    /// return a client connected to it. The mock serves one connection and its
    /// thread exits when the client drops (`read_u8` hits EOF), so no explicit
    /// join is needed — a leaked-but-finished thread is cleaned at process exit.
    ///
    /// Scenario: one Mono runtime, two images. `Player:Update` lives in
    /// `Assembly-CSharp` and compiles to 0xC0DE; everything else misses.
    fn mock_client(mono_handle: u64) -> MonoClient {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let Ok((mut s, _)) = listener.accept() else {
                return;
            };
            while let Ok(byte) = s.read_u8() {
                let Some(cmd) = Command::from_u8(byte) else {
                    break;
                };
                match cmd {
                    Command::InitMono => s.write_u64(mono_handle).unwrap(),
                    Command::EnumImages => {
                        s.write_u32(2).unwrap();
                        s.write_u64(0xA1).unwrap();
                        s.write_string("Assembly-CSharp").unwrap();
                        s.write_u64(0xA2).unwrap();
                        s.write_string("mscorlib").unwrap();
                    }
                    Command::FindMethodByDesc => {
                        let image = s.read_u64().unwrap();
                        let desc = s.read_string().unwrap();
                        let method = if image == 0xA1 && desc == "Player:Update" {
                            0xB1
                        } else {
                            0
                        };
                        s.write_u64(method).unwrap();
                    }
                    Command::CompileMethod => {
                        let method = s.read_u64().unwrap();
                        let addr = if method == 0xB1 { 0xC0DE } else { 0 };
                        s.write_u64(addr).unwrap();
                    }
                    Command::Terminate => break,
                    _ => break,
                }
                s.flush().unwrap();
            }
        });
        MonoClient::connect_to(addr).unwrap()
    }

    #[test]
    fn resolves_class_method_to_jit_address() {
        let mut client = mock_client(0x1000);
        assert_eq!(client.resolve("Player:Update").unwrap(), 0xC0DE);
    }

    #[test]
    fn resolves_with_image_prefix() {
        let mut client = mock_client(0x1000);
        assert_eq!(
            client.resolve("[Assembly-CSharp]Player:Update").unwrap(),
            0xC0DE
        );
    }

    #[test]
    fn unknown_symbol_is_symbol_not_found() {
        let mut client = mock_client(0x1000);
        assert!(matches!(
            client.resolve("Enemy:Die"),
            Err(MonoError::SymbolNotFound(_))
        ));
    }

    #[test]
    fn zero_mono_handle_is_mono_not_found() {
        let mut client = mock_client(0);
        assert!(matches!(
            client.resolve("Player:Update"),
            Err(MonoError::MonoNotFound)
        ));
    }

    #[test]
    fn enum_images_reads_all_entries() {
        let mut client = mock_client(0x1000);
        let images = client.enum_images().unwrap();
        assert_eq!(
            images,
            vec![
                (0xA1, "Assembly-CSharp".to_string()),
                (0xA2, "mscorlib".to_string()),
            ]
        );
    }

    #[test]
    fn parse_symbol_splits_image_prefix() {
        assert_eq!(parse_mono_symbol("Player:Update"), (None, "Player:Update"));
        assert_eq!(
            parse_mono_symbol("[Assembly-CSharp]Player:Update"),
            (Some("Assembly-CSharp"), "Player:Update")
        );
        assert_eq!(
            parse_mono_symbol("[mscorlib] System.String:Concat"),
            (Some("mscorlib"), "System.String:Concat")
        );
    }

    #[test]
    fn image_matches_tolerates_dll_suffix() {
        assert!(image_matches("Assembly-CSharp", "Assembly-CSharp"));
        assert!(image_matches("Assembly-CSharp.dll", "Assembly-CSharp"));
        assert!(!image_matches("mscorlib", "Assembly-CSharp"));
    }
}
