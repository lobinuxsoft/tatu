//! Shared mutable state behind every Lua primitive.
//!
//! CE exposes a single *opened process* and a single global *symbol table*
//! that Auto-Assembler scripts, `registerSymbol`, and `getAddress` all read
//! and write. We mirror that: one [`LuaState`] is shared (via `Rc`) by every
//! registered Lua function so a symbol bound by `autoAssemble` is immediately
//! visible to a later `getAddress`, and `openProcess` re-targets every
//! subsequent memory call.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use mlua::Value;
use nix::unistd::Pid;

use crate::elfsym::find_module_base;
use crate::executor::ActiveCheat;

/// Process-wide state shared by all CE primitives in one runtime.
pub(super) struct LuaState {
    /// The currently opened process — mutated by `openProcess`, read by
    /// every memory/assembler primitive.
    target: Cell<Pid>,
    /// Symbols bound by `autoAssemble`/`registerSymbol`. Consulted by
    /// `getAddress` before falling back to module resolution.
    symbols: RefCell<HashMap<String, u64>>,
    /// Enabled AA scripts kept alive so their hooks don't roll back on drop.
    active: RefCell<Vec<ActiveCheat>>,
}

impl LuaState {
    pub(super) fn new(pid: Pid) -> Self {
        Self {
            target: Cell::new(pid),
            symbols: RefCell::new(HashMap::new()),
            active: RefCell::new(Vec::new()),
        }
    }

    pub(super) fn target(&self) -> Pid {
        self.target.get()
    }

    pub(super) fn set_target(&self, pid: Pid) {
        self.target.set(pid);
    }

    /// Bind (or rebind) a symbol — `registerSymbol` and the post-enable
    /// symbol merge both land here.
    pub(super) fn bind_symbol(&self, name: impl Into<String>, addr: u64) {
        self.symbols.borrow_mut().insert(name.into(), addr);
    }

    pub(super) fn unbind_symbol(&self, name: &str) {
        self.symbols.borrow_mut().remove(name);
    }

    /// Snapshot the symbol table so a fresh engine can be seeded with every
    /// symbol bound so far (CE shares one global table across scripts).
    pub(super) fn snapshot_symbols(&self) -> Vec<(String, u64)> {
        self.symbols
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }

    /// Fold an engine's resolved symbols back into the shared table.
    pub(super) fn merge_symbols(&self, more: &HashMap<String, u64>) {
        let mut symbols = self.symbols.borrow_mut();
        for (k, v) in more {
            symbols.insert(k.clone(), *v);
        }
    }

    /// Keep an enabled cheat's undo log alive (otherwise `Drop` reverts it).
    pub(super) fn keep_alive(&self, cheat: ActiveCheat) {
        self.active.borrow_mut().push(cheat);
    }

    /// Resolve a CE address operand: a bare number, or a `symbol`/`module`
    /// optionally suffixed with `+hexoffset`. Registered symbols win over
    /// module names; the offset is hex per CE convention. Returns `None`
    /// (→ Lua `nil`) when nothing resolves, exactly like CE.
    pub(super) fn resolve(&self, value: &Value) -> Option<u64> {
        match value {
            Value::Integer(n) => Some(*n as u64),
            Value::Number(n) => Some(*n as u64),
            Value::String(s) => self.resolve_expr(&s.to_str().ok()?),
            _ => None,
        }
    }

    fn resolve_expr(&self, expr: &str) -> Option<u64> {
        let expr = expr.trim();
        if let Some((head, off)) = expr.split_once('+') {
            let base = self.resolve_base(head.trim())?;
            let offset = parse_hex(off.trim())?;
            return Some(base.wrapping_add(offset));
        }
        self.resolve_base(expr)
    }

    /// Resolve a bare token to an address: registered symbol first, then a
    /// module load base, then a hex literal — CE's `getAddress` order, so a
    /// module name shadows a coincidentally-hex string.
    fn resolve_base(&self, token: &str) -> Option<u64> {
        if let Some(addr) = self.symbols.borrow().get(token).copied() {
            return Some(addr);
        }
        if let Ok(base) = find_module_base(self.target(), token) {
            return Some(base);
        }
        parse_hex(token)
    }
}

/// Parse a CE-style hex literal (`0x`-prefixed or bare) or a decimal.
fn parse_hex(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok();
    }
    u64::from_str_radix(s, 16).ok()
}
