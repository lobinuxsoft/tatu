use crate::attach::AttachedProcess;
use crate::types::AddressSpec;

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("module '{0}' not found in attached process")]
    ModuleNotFound(String),
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
}
