//! UI stubs so a framework table's bootstrap doesn't crash without CE's LCL
//! toolkit.
//!
//! Manifold-style tables build forms, buttons and timers at load time
//! (`createForm`, `createTimer`, …) and wire them to handlers. tatu ships no
//! GUI, so we register a *universal stub*: one object whose every field access,
//! call, and method invocation returns the stub itself, and whose assignments
//! are swallowed. The bootstrap runs to completion; the visual layer is simply
//! inert. Real periodic behaviour (timers driving freeze loops) is out of scope
//! for phase 1 — see the module roadmap.

use mlua::{Lua, Result as LuaResult};

/// Lua prelude installing the universal stub and binding CE's UI factory
/// globals to it. Kept as source so the catch-all metatable stays readable.
const PRELUDE: &str = r#"
-- One shared inert object: indexing, calling, or method-invoking it yields
-- itself; assigning to it is a no-op. Truthy so `if widget.Visible then`
-- branches don't nil-crash the bootstrap.
local stub = {}
setmetatable(stub, {
  __index = function() return stub end,
  __newindex = function() end,
  __call = function() return stub end,
})

-- Factories CE exposes that return a widget/handle. All hand back the stub.
local factories = {
  "createForm", "createTimer", "createButton", "createLabel", "createPanel",
  "createGroupBox", "createImage", "createPicture", "createCheckBox",
  "createComboBox", "createEdit", "createMemo", "createTrackBar",
  "createListBox", "createListView", "createScrollBox", "createTabControl",
  "createHotkey", "createMenuItem", "createPopupMenu", "createMainMenu",
  "createToggleBox", "createProgressBar", "createOpenDialog",
  "createSaveDialog", "getMainForm", "getApplication", "getAddressList",
  "getFormCount", "getForm",
}
for _, name in ipairs(factories) do
  _G[name] = function() return stub end
end

-- Object globals CE pre-populates.
MainForm = stub
AddressList = stub
TrainerOrigin = stub

-- Highlighting hint CE calls hundreds of times across a framework table.
registerLuaFunctionHighlight = function() end

-- Notification registrars return a cleanup handle (the stub) in CE.
local notifiers = {
  "registerFormAddNotification", "registerMainFormCloseNotification",
  "registerProcessOpenedCallback",
}
for _, name in ipairs(notifiers) do
  _G[name] = function() return stub end
end

-- Dialog/misc primitives with fixed, non-interactive answers (no GUI):
-- messageDialog/showMessage just acknowledge; inputQuery reports "cancelled";
-- getCEVersion hands back a plausible number; autoAssembleCheck claims OK.
showMessage = function() end
messageDialog = function() return mrOK end
inputQuery = function() return false end       -- (cancelled, no value)
getCEVersion = function() return 7.5 end
autoAssembleCheck = function() return true end
"#;

/// Run the UI stub prelude in `lua`.
pub(super) fn install(lua: &Lua) -> LuaResult<()> {
    lua.load(PRELUDE).set_name("@tatu:ui_stub").exec()
}
