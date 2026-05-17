use crate::attach::AttachedProcess;
use crate::memory::{self, MemoryError};
use crate::types::AddressSpec;

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("module '{0}' not found in attached process")]
    ModuleNotFound(String),
    #[error("pointer chain dereference failed: {0}")]
    Memory(#[from] MemoryError),
}

pub fn resolve_address(
    spec: &AddressSpec,
    attached: &AttachedProcess,
) -> Result<u64, ResolveError> {
    match spec {
        AddressSpec::Static { module, offset } => {
            let range = attached
                .modules
                .get(module)
                .ok_or_else(|| ResolveError::ModuleNotFound(module.clone()))?;
            Ok(range.base + offset)
        }
        AddressSpec::PointerChain {
            base_module,
            base_offset,
            offsets,
        } => {
            let range = attached
                .modules
                .get(base_module)
                .ok_or_else(|| ResolveError::ModuleNotFound(base_module.clone()))?;
            let mut current = range.base + base_offset;
            for &offset in offsets {
                let ptr: u64 = memory::read_typed(attached.pid, current)?;
                current = ptr.wrapping_add(offset);
            }
            Ok(current)
        }
        AddressSpec::Absolute { address } => Ok(*address),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attach::ModuleRange;
    use std::collections::HashMap;

    fn fake_process(module: &str, base: u64) -> AttachedProcess {
        let mut modules = HashMap::new();
        modules.insert(
            module.to_string(),
            ModuleRange {
                base,
                end: base + 0x10000,
                path: format!("/fake/{module}"),
            },
        );
        AttachedProcess { pid: 1, modules }
    }

    #[test]
    fn resolve_static_adds_offset_to_base() {
        let proc = fake_process("game.exe", 0x140000000);
        let spec = AddressSpec::Static {
            module: "game.exe".into(),
            offset: 0x1234,
        };
        let addr = resolve_address(&spec, &proc).expect("resolve");
        assert_eq!(addr, 0x140001234);
    }

    #[test]
    fn resolve_zero_offset_returns_base() {
        let proc = fake_process("game.exe", 0x140000000);
        let spec = AddressSpec::Static {
            module: "game.exe".into(),
            offset: 0,
        };
        assert_eq!(resolve_address(&spec, &proc).unwrap(), 0x140000000);
    }

    #[test]
    fn resolve_missing_module_errors() {
        let proc = fake_process("game.exe", 0x140000000);
        let spec = AddressSpec::Static {
            module: "missing.dll".into(),
            offset: 0x10,
        };
        let result = resolve_address(&spec, &proc);
        assert!(matches!(result, Err(ResolveError::ModuleNotFound(_))));
    }

    #[test]
    fn resolve_absolute_returns_address_as_is() {
        let proc = fake_process("game.exe", 0x140000000);
        let spec = AddressSpec::Absolute {
            address: 0x7FFE_1234_ABCD,
        };
        assert_eq!(resolve_address(&spec, &proc).unwrap(), 0x7FFE_1234_ABCD);
    }

    #[test]
    fn resolve_pointer_chain_missing_module_errors() {
        let proc = fake_process("game.exe", 0x140000000);
        let spec = AddressSpec::PointerChain {
            base_module: "missing.dll".into(),
            base_offset: 0,
            offsets: vec![0x10],
        };
        let result = resolve_address(&spec, &proc);
        assert!(matches!(result, Err(ResolveError::ModuleNotFound(_))));
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0; uses own-process memory as victim"]
    fn resolve_pointer_chain_one_level() {
        let target_value: u32 = 0xCAFE_BABE;
        let target_addr = std::ptr::addr_of!(target_value) as u64;

        let pointer_storage: u64 = target_addr;
        let pointer_addr = std::ptr::addr_of!(pointer_storage) as u64;

        let mut modules = HashMap::new();
        modules.insert(
            "fake".to_string(),
            ModuleRange {
                base: pointer_addr,
                end: pointer_addr + 8,
                path: "/fake".into(),
            },
        );
        let attached = AttachedProcess {
            pid: std::process::id() as i32,
            modules,
        };

        let spec = AddressSpec::PointerChain {
            base_module: "fake".into(),
            base_offset: 0,
            offsets: vec![0],
        };

        let resolved = resolve_address(&spec, &attached).expect("resolve");
        assert_eq!(resolved, target_addr);

        let read: u32 = crate::memory::read_typed(attached.pid, resolved).expect("read");
        assert_eq!(read, 0xCAFE_BABE);
    }

    #[test]
    #[ignore = "requires kernel.yama.ptrace_scope=0; uses own-process memory as victim"]
    fn resolve_pointer_chain_two_levels_with_field_offset() {
        #[repr(C)]
        struct Inner {
            _pad: [u8; 24],
            field: u32,
        }
        let inner = Inner {
            _pad: [0; 24],
            field: 0xDEAD_BEEF,
        };
        let inner_addr = std::ptr::addr_of!(inner) as u64;
        let field_offset = 24_u64;

        let ptr_to_inner: u64 = inner_addr;
        let storage_addr = std::ptr::addr_of!(ptr_to_inner) as u64;

        let mut modules = HashMap::new();
        modules.insert(
            "fake".to_string(),
            ModuleRange {
                base: storage_addr,
                end: storage_addr + 8,
                path: "/fake".into(),
            },
        );
        let attached = AttachedProcess {
            pid: std::process::id() as i32,
            modules,
        };

        let spec = AddressSpec::PointerChain {
            base_module: "fake".into(),
            base_offset: 0,
            offsets: vec![field_offset],
        };

        let resolved = resolve_address(&spec, &attached).expect("resolve");
        assert_eq!(resolved, inner_addr + field_offset);

        let read: u32 = crate::memory::read_typed(attached.pid, resolved).expect("read");
        assert_eq!(read, 0xDEAD_BEEF);
    }
}
