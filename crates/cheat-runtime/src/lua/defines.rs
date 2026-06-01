//! CE's predefined global constants.
//!
//! Cheat Engine seeds every Lua state with a set of enum globals (variable
//! types, LCL alignment/message-dialog constants, …) from its `defines.lua`.
//! Framework tables index tables with them at load time — e.g.
//! `{[vtString] = true}` — so an undefined `vtString` is `nil` and crashes the
//! bootstrap. Values mirror CE's `defines.lua` / `TVariableType` verbatim so
//! our classification matches what CE writes into a `.CT`.

use mlua::{Lua, Result as LuaResult};

/// Constants taken 1:1 from Cheat Engine's `defines.lua` (and the
/// `TVariableType` enum in `commonTypeDefs`).
const PRELUDE: &str = r#"
-- TVariableType (commonTypeDefs.pas)
vtByte=0; vtWord=1; vtDword=2; vtQword=3; vtSingle=4; vtDouble=5
vtString=6; vtUnicodeString=7; vtByteArray=8; vtBinary=9; vtAll=10
vtAutoAssembler=11; vtPointer=12; vtCustom=13; vtGrouped=14
vtByteArrays=15; vtCodePageString=16

-- TAlign (LCL)
alNone=0; alTop=1; alBottom=2; alLeft=3; alRight=4; alClient=5

-- TMsgDlgType
mtWarning=0; mtError=1; mtInformation=2; mtConfirmation=3; mtCustom=4

-- TMsgDlgBtn
mbYes=0; mbNo=1; mbOK=2; mbCancel=3; mbAbort=4; mbRetry=5; mbIgnore=6
mbAll=7; mbNoToAll=8; mbYesToAll=9; mbHelp=10; mbClose=11

-- Modal results
mrNone=0; mrOK=1; mrCancel=2; mrAbort=3; mrRetry=4; mrIgnore=5
mrYes=6; mrNo=7; mrAll=8; mrNoToAll=9; mrYesToAll=10
"#;

/// Seed the CE constant globals.
pub(super) fn install(lua: &Lua) -> LuaResult<()> {
    lua.load(PRELUDE).set_name("@tatu:ce_defines").exec()
}
