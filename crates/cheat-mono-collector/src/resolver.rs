//! Command dispatch: read one request, call Mono, write one response.
//!
//! The framing matches CE's `PipeServer.cpp` handlers byte-for-byte (see
//! [`crate::protocol`]) so a CE-compatible client gets the responses it
//! expects. Windows-only because it drives the in-process Mono API.

use std::io::{self, Read, Write};

use crate::mono::MonoApi;
use crate::protocol::{Command, WireRead, WireWrite};

/// Outcome of handling one command, so the server loop knows when to stop.
pub enum Flow {
    /// Keep serving on this connection.
    Continue,
    /// Client sent `Terminate` — close the connection.
    Stop,
}

/// Read one command and its arguments from `stream`, perform it against `mono`,
/// and write the response. Returns [`Flow::Stop`] on `Terminate` or when the
/// peer closes the stream.
///
/// # Safety
/// `mono` must reference a live in-process Mono runtime and the calling thread
/// must already be attached (see [`MonoApi::attach_current_thread`]).
pub unsafe fn handle_one<S: Read + Write>(stream: &mut S, mono: &MonoApi) -> io::Result<Flow> {
    let byte = match stream.read_u8() {
        Ok(b) => b,
        // Peer closed the connection between commands — treat as a clean stop.
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(Flow::Stop),
        Err(e) => return Err(e),
    };

    let Some(cmd) = Command::from_u8(byte) else {
        // Unknown command: we can't know its argument length, so the stream is
        // desynced. Stop rather than guess and corrupt every later response.
        return Ok(Flow::Stop);
    };

    match cmd {
        Command::InitMono => {
            // The thread was attached before the loop; just report the handle.
            stream.write_u64(mono.module_handle())?;
        }
        Command::EnumImages => {
            let images = unsafe { mono.enum_images() };
            // CE frames this as a byte buffer; we keep it simple and self-
            // describing: u32 count, then (u64 handle, string name) each.
            stream.write_u32(images.len() as u32)?;
            for (handle, name) in images {
                stream.write_u64(handle)?;
                stream.write_string(&name)?;
            }
        }
        Command::FindClass => {
            let image = stream.read_u64()?;
            let class = stream.read_string()?;
            let namespace = stream.read_string()?;
            let klass = unsafe { mono.find_class(image, &namespace, &class) };
            stream.write_u64(klass)?;
        }
        Command::FindMethod => {
            let class = stream.read_u64()?;
            let method = stream.read_string()?;
            let m = unsafe { mono.find_method(class, &method) };
            stream.write_u64(m)?;
        }
        Command::FindMethodByDesc => {
            let image = stream.read_u64()?;
            let desc = stream.read_string()?;
            let m = unsafe { mono.find_method_by_desc(image, &desc) };
            stream.write_u64(m)?;
        }
        Command::CompileMethod => {
            let method = stream.read_u64()?;
            let addr = unsafe { mono.compile_method(method) };
            stream.write_u64(addr)?;
        }
        Command::GetJitInfo => {
            let domain = stream.read_u64()?;
            let address = stream.read_u64()?;
            let (ji, method, code_start, code_size) = unsafe { mono.jit_info(domain, address) };
            stream.write_u64(ji)?;
            if ji != 0 {
                stream.write_u64(method)?;
                stream.write_u64(code_start)?;
                stream.write_u32(code_size)?;
            }
        }
        Command::Terminate => return Ok(Flow::Stop),
    }

    stream.flush()?;
    Ok(Flow::Continue)
}
