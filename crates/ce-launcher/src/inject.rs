use std::fmt::Write as _;

const AUTO_ATTACH_PREFIX: &str = "getAutoAttachList().add(\"";
const AUTO_ATTACH_SUFFIX: &str = "\")";
const LUA_OPEN_TAG: &str = "<LuaScript>";
const LUA_CLOSE_TAG: &str = "</LuaScript>";
const TABLE_CLOSE_TAG: &str = "</CheatTable>";

#[derive(Debug, thiserror::Error)]
pub enum InjectError {
    #[error("exe name contains XML/Lua-unsafe characters: {0:?}")]
    UnsafeExeName(String),
    #[error("CT file is malformed: missing </CheatTable> close tag")]
    MalformedCt,
}

/// Return a `.CT` XML string guaranteed to auto-attach to `exe_name` when CE opens it.
///
/// Idempotent: applying twice yields the same output as once. Existing auto-attach
/// entries for other executables are preserved (CE supports multiple targets).
pub fn ensure_auto_attach(table_xml: &str, exe_name: &str) -> Result<String, InjectError> {
    if exe_name.contains('"') || exe_name.contains('<') || exe_name.contains('>') {
        return Err(InjectError::UnsafeExeName(exe_name.to_string()));
    }

    match find_lua_block(table_xml) {
        Some((content_start, content_end)) => {
            let body = &table_xml[content_start..content_end];
            if already_attaches_to(body, exe_name) {
                return Ok(table_xml.to_string());
            }
            Ok(append_inside_lua(
                table_xml,
                content_start,
                content_end,
                exe_name,
            ))
        }
        None => insert_lua_block(table_xml, exe_name),
    }
}

fn find_lua_block(xml: &str) -> Option<(usize, usize)> {
    let open = xml.find(LUA_OPEN_TAG)?;
    let after_open = open + LUA_OPEN_TAG.len();
    let close = xml[after_open..].find(LUA_CLOSE_TAG)? + after_open;
    Some((after_open, close))
}

fn already_attaches_to(body: &str, exe_name: &str) -> bool {
    let mut needle = String::with_capacity(AUTO_ATTACH_PREFIX.len() + exe_name.len() + 2);
    let _ = write!(needle, "{AUTO_ATTACH_PREFIX}{exe_name}{AUTO_ATTACH_SUFFIX}");
    body.lines()
        .any(|line| !line.trim_start().starts_with("--") && line.contains(&needle))
}

fn append_inside_lua(
    xml: &str,
    content_start: usize,
    content_end: usize,
    exe_name: &str,
) -> String {
    let body = &xml[content_start..content_end];
    let line = format!("{AUTO_ATTACH_PREFIX}{exe_name}{AUTO_ATTACH_SUFFIX}");

    let new_body = match strip_cdata(body) {
        Some(inner) => {
            let mut buf = String::with_capacity(body.len() + line.len() + 16);
            buf.push_str("<![CDATA[\n");
            buf.push_str(inner.trim_end_matches('\n'));
            buf.push('\n');
            buf.push_str(&line);
            buf.push_str("\n]]>");
            buf
        }
        None => {
            let mut buf = String::with_capacity(body.len() + line.len() + 4);
            buf.push_str(body.trim_end_matches('\n'));
            buf.push('\n');
            buf.push_str(&line);
            buf.push('\n');
            buf
        }
    };

    let mut result = String::with_capacity(xml.len() + new_body.len());
    result.push_str(&xml[..content_start]);
    result.push_str(&new_body);
    result.push_str(&xml[content_end..]);
    result
}

fn strip_cdata(s: &str) -> Option<&str> {
    s.trim()
        .strip_prefix("<![CDATA[")
        .and_then(|inner| inner.strip_suffix("]]>"))
}

fn insert_lua_block(xml: &str, exe_name: &str) -> Result<String, InjectError> {
    let close = xml.rfind(TABLE_CLOSE_TAG).ok_or(InjectError::MalformedCt)?;
    let line = format!("{AUTO_ATTACH_PREFIX}{exe_name}{AUTO_ATTACH_SUFFIX}");
    let block = format!("  <LuaScript><![CDATA[\n{line}\n]]></LuaScript>\n");

    let mut result = String::with_capacity(xml.len() + block.len());
    result.push_str(&xml[..close]);
    result.push_str(&block);
    result.push_str(&xml[close..]);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_CT: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<CheatTable>
  <CheatEntries />
</CheatTable>
"#;

    const CT_WITH_LUA: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<CheatTable>
  <LuaScript><![CDATA[
print("hello")
]]></LuaScript>
  <CheatEntries />
</CheatTable>
"#;

    const CT_WITH_AUTO_ATTACH: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<CheatTable>
  <LuaScript><![CDATA[
getAutoAttachList().add("game.exe")
]]></LuaScript>
  <CheatEntries />
</CheatTable>
"#;

    #[test]
    fn inserts_lua_block_into_table_without_one() {
        let out = ensure_auto_attach(MINIMAL_CT, "game.exe").expect("should inject");
        assert!(out.contains("<LuaScript>"));
        assert!(out.contains(r#"getAutoAttachList().add("game.exe")"#));
        assert!(out.contains("</CheatTable>"));
        // CheatEntries preserved
        assert!(out.contains("<CheatEntries />"));
    }

    #[test]
    fn appends_into_existing_lua_block_without_duplicating() {
        let out = ensure_auto_attach(CT_WITH_LUA, "game.exe").expect("should append");
        assert!(out.contains(r#"print("hello")"#));
        assert!(out.contains(r#"getAutoAttachList().add("game.exe")"#));
        assert_eq!(out.matches("<LuaScript>").count(), 1);
        assert_eq!(out.matches("</LuaScript>").count(), 1);
    }

    #[test]
    fn idempotent_when_exact_match_present() {
        let once = ensure_auto_attach(CT_WITH_AUTO_ATTACH, "game.exe").expect("noop");
        assert_eq!(
            once.matches(r#"getAutoAttachList().add("game.exe")"#)
                .count(),
            1
        );
        let twice = ensure_auto_attach(&once, "game.exe").expect("idempotent");
        assert_eq!(twice, once);
    }

    #[test]
    fn appends_distinct_exe_alongside_existing() {
        let out = ensure_auto_attach(CT_WITH_AUTO_ATTACH, "other.exe").expect("multi-target");
        assert!(out.contains(r#"getAutoAttachList().add("game.exe")"#));
        assert!(out.contains(r#"getAutoAttachList().add("other.exe")"#));
    }

    #[test]
    fn ignores_commented_out_match() {
        let xml = r#"<?xml version="1.0"?>
<CheatTable>
  <LuaScript><![CDATA[
--getAutoAttachList().add("game.exe")
print("real logic")
]]></LuaScript>
</CheatTable>
"#;
        let out = ensure_auto_attach(xml, "game.exe").expect("treat comment as absent");
        let count = out
            .matches(r#"getAutoAttachList().add("game.exe")"#)
            .count();
        // comment still in body + new uncommented line
        assert_eq!(count, 2);
    }

    #[test]
    fn rejects_exe_name_with_quote() {
        let result = ensure_auto_attach(MINIMAL_CT, r#"bad"name.exe"#);
        assert!(matches!(result, Err(InjectError::UnsafeExeName(_))));
    }

    #[test]
    fn rejects_exe_name_with_angle_brackets() {
        let result = ensure_auto_attach(MINIMAL_CT, "bad<name>.exe");
        assert!(matches!(result, Err(InjectError::UnsafeExeName(_))));
    }

    #[test]
    fn rejects_malformed_ct_without_close_tag() {
        let xml = r#"<?xml version="1.0"?><CheatTable>"#;
        let result = ensure_auto_attach(xml, "game.exe");
        assert!(matches!(result, Err(InjectError::MalformedCt)));
    }

    #[test]
    fn preserves_cheat_entries_outside_lua_block() {
        let xml = r#"<?xml version="1.0"?>
<CheatTable>
  <CheatEntries>
    <CheatEntry>
      <ID>1</ID>
      <Description>"Health"</Description>
      <VariableType>4 Bytes</VariableType>
      <Address>game.exe+1234</Address>
    </CheatEntry>
  </CheatEntries>
</CheatTable>
"#;
        let out = ensure_auto_attach(xml, "game.exe").expect("ok");
        assert!(out.contains("<ID>1</ID>"));
        assert!(out.contains(r#"<Description>"Health"</Description>"#));
        assert!(out.contains("game.exe+1234"));
        assert!(out.contains(r#"getAutoAttachList().add("game.exe")"#));
    }

    #[test]
    fn preserves_existing_lua_logic_intact() {
        let xml = r#"<?xml version="1.0"?>
<CheatTable>
  <LuaScript><![CDATA[
local foo = 42
function onAttach()
  print("hooked at " .. tostring(foo))
end
]]></LuaScript>
</CheatTable>
"#;
        let out = ensure_auto_attach(xml, "x.exe").expect("ok");
        assert!(out.contains("local foo = 42"));
        assert!(out.contains("function onAttach()"));
        assert!(out.contains(r#"print("hooked at " .. tostring(foo))"#));
        assert!(out.contains(r#"getAutoAttachList().add("x.exe")"#));
    }

    #[test]
    fn handles_lua_block_without_cdata_wrapper() {
        let xml = r#"<?xml version="1.0"?>
<CheatTable>
  <LuaScript>print("naked lua")</LuaScript>
</CheatTable>
"#;
        let out = ensure_auto_attach(xml, "x.exe").expect("ok");
        assert!(out.contains(r#"print("naked lua")"#));
        assert!(out.contains(r#"getAutoAttachList().add("x.exe")"#));
    }
}
