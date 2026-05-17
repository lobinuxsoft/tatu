use nix::sys::uio::{RemoteIoVec, process_vm_readv, process_vm_writev};
use nix::unistd::Pid;
use std::io::{IoSlice, IoSliceMut};

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("syscall failed: {0}")]
    Syscall(#[from] nix::Error),
    #[error("partial read at {address:#x}: expected {expected} bytes, got {got}")]
    PartialRead {
        address: u64,
        expected: usize,
        got: usize,
    },
    #[error("partial write at {address:#x}: expected {expected} bytes, got {got}")]
    PartialWrite {
        address: u64,
        expected: usize,
        got: usize,
    },
}

pub fn write_bytes(pid: i32, address: u64, data: &[u8]) -> Result<(), MemoryError> {
    let local = [IoSlice::new(data)];
    let remote = [RemoteIoVec {
        base: address as usize,
        len: data.len(),
    }];

    let written = process_vm_writev(Pid::from_raw(pid), &local, &remote)?;

    if written != data.len() {
        return Err(MemoryError::PartialWrite {
            address,
            expected: data.len(),
            got: written,
        });
    }

    Ok(())
}

pub fn read_bytes(pid: i32, address: u64, len: usize) -> Result<Vec<u8>, MemoryError> {
    let mut buf = vec![0u8; len];
    let mut local = [IoSliceMut::new(&mut buf)];
    let remote = [RemoteIoVec {
        base: address as usize,
        len,
    }];

    let read = process_vm_readv(Pid::from_raw(pid), &mut local, &remote)?;

    if read != len {
        return Err(MemoryError::PartialRead {
            address,
            expected: len,
            got: read,
        });
    }

    Ok(buf)
}

pub fn write_typed<T: bytemuck::Pod>(pid: i32, address: u64, value: T) -> Result<(), MemoryError> {
    write_bytes(pid, address, bytemuck::bytes_of(&value))
}

pub fn read_typed<T: bytemuck::Pod>(pid: i32, address: u64) -> Result<T, MemoryError> {
    let bytes = read_bytes(pid, address, std::mem::size_of::<T>())?;
    Ok(bytemuck::pod_read_unaligned(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn write_typed_u32_into_own_process() {
        let mut target: u32 = 0xABCD_0000;
        let addr = std::ptr::addr_of_mut!(target) as u64;
        let pid = std::process::id() as i32;

        write_typed::<u32>(pid, addr, 0xDEAD_BEEF).expect("write u32");
        assert_eq!(target, 0xDEAD_BEEF);
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn read_typed_u32_from_own_process() {
        let target: u32 = 0xCAFE_BABE;
        let addr = std::ptr::addr_of!(target) as u64;
        let pid = std::process::id() as i32;

        let read: u32 = read_typed(pid, addr).expect("read u32");
        assert_eq!(read, 0xCAFE_BABE);
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn read_typed_f32_from_own_process() {
        let target: f32 = 1234.5678;
        let addr = std::ptr::addr_of!(target) as u64;
        let pid = std::process::id() as i32;

        let read: f32 = read_typed(pid, addr).expect("read f32");
        assert_eq!(read, 1234.5678);
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0 or CAP_SYS_PTRACE; run with --ignored"]
    fn write_to_unmapped_address_fails() {
        let pid = std::process::id() as i32;
        let result = write_bytes(pid, 0x1, &[0xAB]);
        assert!(result.is_err(), "expected write to address 0x1 to fail");
    }
}
