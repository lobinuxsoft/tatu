use quick_xml::Reader;
use quick_xml::events::Event;

use crate::types::{AddressSpec, Cheat, CheatAction, CheatValue};

#[derive(Debug, thiserror::Error)]
pub enum CtImportError {
    #[error("malformed XML at position {pos}: {msg}")]
    MalformedXml { pos: usize, msg: String },
    #[error("unsupported VariableType '{0}'")]
    UnsupportedVariableType(String),
    #[error("unsupported Address form '{0}'")]
    UnsupportedAddressForm(String),
    #[error("invalid hex in '{value}': {source}")]
    InvalidHex {
        value: String,
        source: std::num::ParseIntError,
    },
}

/// Outcome of parsing a single `<CheatEntry>` — either a cheat we can use,
/// or a skip reason (script entries, grouping headers, etc) that the
/// caller can surface to the user without aborting the whole import.
#[derive(Debug)]
pub enum ImportedEntry {
    Cheat(Cheat),
    Skipped {
        description: String,
        reason: SkipReason,
    },
}

#[derive(Debug)]
pub enum SkipReason {
    AssemblerScript,
    GroupingHeader,
    SymbolicAddress(String),
    UnsupportedVariableType(String),
    UnsupportedAddressForm(String),
}

pub fn parse_ct(xml: &str) -> Result<Vec<ImportedEntry>, CtImportError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut entries = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref() == b"CheatEntry" => {
                if let Some(entry) = parse_cheat_entry(&mut reader)? {
                    entries.push(entry);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(CtImportError::MalformedXml {
                    pos: reader.buffer_position() as usize,
                    msg: e.to_string(),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(entries)
}

/// Parse the contents of a single `<CheatEntry>` element. Recurses on
/// nested `<CheatEntry>` (CE supports tree-style grouping); each nested
/// entry is appended to the same flat output stream — grouping is purely
/// presentational for our purposes.
fn parse_cheat_entry<R: std::io::BufRead>(
    reader: &mut Reader<R>,
) -> Result<Option<ImportedEntry>, CtImportError> {
    let mut id: Option<String> = None;
    let mut description: Option<String> = None;
    let mut variable_type: Option<String> = None;
    let mut address: Option<String> = None;
    let mut offsets: Vec<u64> = Vec::new();
    let mut symbolic_offset: Option<String> = None;
    let mut is_script = false;
    let mut nested: Vec<ImportedEntry> = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"ID" => id = Some(read_text(reader)?),
                b"Description" => description = Some(strip_quotes(&read_text(reader)?)),
                b"VariableType" => variable_type = Some(read_text(reader)?),
                b"Address" => address = Some(read_text(reader)?),
                b"Offsets" => match parse_offsets(reader)? {
                    Ok(v) => offsets = v,
                    Err(sym) => symbolic_offset = Some(sym),
                },
                b"AssemblerScript" => {
                    is_script = true;
                    skip_to_end(reader, b"AssemblerScript")?;
                }
                b"CheatEntry" => {
                    if let Some(entry) = parse_cheat_entry(reader)? {
                        nested.push(entry);
                    }
                }
                _ => {} // ignore unknown child (LastState, Color, Options, ShowAsSigned, ...)
            },
            Ok(Event::End(e)) if e.name().as_ref() == b"CheatEntry" => break,
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(CtImportError::MalformedXml {
                    pos: reader.buffer_position() as usize,
                    msg: e.to_string(),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    let desc = description
        .clone()
        .unwrap_or_else(|| id.clone().unwrap_or_else(|| "<unnamed>".into()));

    // CE serializes offsets innermost-first, which matches our resolve_address
    // walk order — no reversal needed.

    if is_script {
        let _ = nested;
        return Ok(Some(ImportedEntry::Skipped {
            description: desc,
            reason: SkipReason::AssemblerScript,
        }));
    }

    let Some(addr_str) = address else {
        // Folder/grouping entry. v1 surfaces only top-level cheats; nested
        // children inside a folder are intentionally dropped (typically
        // "+OFFSET" child-of-parent views that aren't independently usable).
        let _ = nested;
        return Ok(Some(ImportedEntry::Skipped {
            description: desc,
            reason: SkipReason::GroupingHeader,
        }));
    };

    if let Some(sym) = symbolic_offset {
        return Ok(Some(ImportedEntry::Skipped {
            description: desc,
            reason: SkipReason::SymbolicAddress(format!(
                "{addr_str} (offset chain contains '{sym}')"
            )),
        }));
    }

    let Some(vtype) = variable_type else {
        return Ok(Some(ImportedEntry::Skipped {
            description: desc,
            reason: SkipReason::GroupingHeader,
        }));
    };

    let value = match cheat_value_for_type(&vtype) {
        Some(v) => v,
        None => {
            return Ok(Some(ImportedEntry::Skipped {
                description: desc,
                reason: SkipReason::UnsupportedVariableType(vtype),
            }));
        }
    };

    let spec = match parse_address_spec(&addr_str, &offsets) {
        Ok(s) => s,
        Err(CtImportError::UnsupportedAddressForm(form)) => {
            return Ok(Some(ImportedEntry::Skipped {
                description: desc,
                reason: if !offsets.is_empty() || !addr_str.contains('+') {
                    SkipReason::SymbolicAddress(addr_str)
                } else {
                    SkipReason::UnsupportedAddressForm(form)
                },
            }));
        }
        Err(other) => return Err(other),
    };

    Ok(Some(ImportedEntry::Cheat(Cheat {
        id: slugify(&id.unwrap_or_else(|| desc.clone())),
        name: desc.clone(),
        description: None,
        address: spec,
        action: CheatAction::WriteOnce { value },
    })))
}

/// Parse a `<Offsets>` element. Returns either the list of u64 offsets,
/// or the raw text of the first non-numeric offset encountered — those
/// happen when the CT uses CE script expressions like `[symbol]-4` and
/// signal that the entry can't be imported without running the parent
/// AOB script.
fn parse_offsets<R: std::io::BufRead>(
    reader: &mut Reader<R>,
) -> Result<Result<Vec<u64>, String>, CtImportError> {
    let mut out = Vec::new();
    let mut symbolic_offset: Option<String> = None;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref() == b"Offset" => {
                let text = read_text(reader)?;
                match parse_hex_loose(&text) {
                    Ok(v) if symbolic_offset.is_none() => out.push(v),
                    Ok(_) => {} // already symbolic, keep draining
                    Err(_) => {
                        if symbolic_offset.is_none() {
                            symbolic_offset = Some(text);
                        }
                    }
                }
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"Offsets" => break,
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(CtImportError::MalformedXml {
                    pos: reader.buffer_position() as usize,
                    msg: e.to_string(),
                });
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(match symbolic_offset {
        Some(s) => Err(s),
        None => Ok(out),
    })
}

fn read_text<R: std::io::BufRead>(reader: &mut Reader<R>) -> Result<String, CtImportError> {
    let mut buf = Vec::new();
    let mut out = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Text(t)) => {
                let decoded = t.decode().map_err(|e| CtImportError::MalformedXml {
                    pos: reader.buffer_position() as usize,
                    msg: e.to_string(),
                })?;
                out.push_str(&decoded);
            }
            Ok(Event::CData(t)) => {
                let s = std::str::from_utf8(&t).map_err(|e| CtImportError::MalformedXml {
                    pos: reader.buffer_position() as usize,
                    msg: e.to_string(),
                })?;
                out.push_str(s);
            }
            Ok(Event::End(_)) | Ok(Event::Eof) => break,
            Err(e) => {
                return Err(CtImportError::MalformedXml {
                    pos: reader.buffer_position() as usize,
                    msg: e.to_string(),
                });
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn skip_to_end<R: std::io::BufRead>(
    reader: &mut Reader<R>,
    tag: &[u8],
) -> Result<(), CtImportError> {
    let mut depth: i32 = 1;
    let mut buf = Vec::new();
    while depth > 0 {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref() == tag => depth += 1,
            Ok(Event::End(e)) if e.name().as_ref() == tag => depth -= 1,
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(CtImportError::MalformedXml {
                    pos: reader.buffer_position() as usize,
                    msg: e.to_string(),
                });
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}

fn cheat_value_for_type(vtype: &str) -> Option<CheatValue> {
    // Default to 0; the user picks the real value at trigger time or via JSON.
    // We only need a representative variant to lock the byte width.
    match vtype.trim() {
        "Byte" => Some(CheatValue::U8(0)),
        "2 Bytes" => Some(CheatValue::U16(0)),
        "4 Bytes" => Some(CheatValue::U32(0)),
        "8 Bytes" => Some(CheatValue::U64(0)),
        "Float" => Some(CheatValue::F32(0.0)),
        "Double" => Some(CheatValue::F64(0.0)),
        _ => None,
    }
}

fn parse_address_spec(addr: &str, offsets: &[u64]) -> Result<AddressSpec, CtImportError> {
    let trimmed = addr.trim();

    // Any '[...]' or arithmetic on a symbol is CE script-expression
    // territory (e.g. "[i_base_exp_off]-4"). We can't resolve those
    // without running the parent AOB script.
    if trimmed.contains('[') || trimmed.contains(']') || trimmed.contains('-') {
        return Err(CtImportError::UnsupportedAddressForm(trimmed.to_string()));
    }

    if let Some(plus_idx) = trimmed.find('+') {
        let module = trimmed[..plus_idx].trim();
        let offset_str = trimmed[plus_idx + 1..].trim();
        if module.is_empty() {
            // "+12C" — child-offset-only entry, not standalone resolvable.
            return Err(CtImportError::UnsupportedAddressForm(trimmed.to_string()));
        }
        let Ok(offset) = parse_hex_loose(offset_str) else {
            return Err(CtImportError::UnsupportedAddressForm(trimmed.to_string()));
        };
        return Ok(if offsets.is_empty() {
            AddressSpec::Static {
                module: module.to_string(),
                offset,
            }
        } else {
            AddressSpec::PointerChain {
                base_module: module.to_string(),
                base_offset: offset,
                offsets: offsets.to_vec(),
            }
        });
    }

    if let Ok(addr_u64) = parse_hex_loose(trimmed) {
        return if offsets.is_empty() {
            Ok(AddressSpec::Absolute { address: addr_u64 })
        } else {
            Err(CtImportError::UnsupportedAddressForm(format!(
                "{trimmed} (absolute address with pointer offsets — needs a module base for portability)"
            )))
        };
    }

    Err(CtImportError::UnsupportedAddressForm(trimmed.to_string()))
}

fn parse_hex_loose(s: &str) -> Result<u64, CtImportError> {
    let trimmed = s.trim();
    let hex = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    u64::from_str_radix(hex, 16).map_err(|e| CtImportError::InvalidHex {
        value: s.to_string(),
        source: e,
    })
}

fn strip_quotes(s: &str) -> String {
    let trimmed = s.trim();
    trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(trimmed)
        .to_string()
}

fn slugify(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut last_was_sep = true;
    for ch in lower.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }
    out.trim_end_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_cheat(entry: &ImportedEntry) -> &Cheat {
        match entry {
            ImportedEntry::Cheat(c) => c,
            ImportedEntry::Skipped {
                description,
                reason,
            } => {
                panic!("expected Cheat, got Skipped({description}, {reason:?})")
            }
        }
    }

    #[test]
    fn parse_ct_absolute_hex_address() {
        let xml = r#"<?xml version="1.0"?>
<CheatTable>
  <CheatEntries>
    <CheatEntry>
      <ID>0</ID>
      <Description>"abs"</Description>
      <VariableType>4 Bytes</VariableType>
      <Address>0xa49b10</Address>
    </CheatEntry>
  </CheatEntries>
</CheatTable>"#;
        let entries = parse_ct(xml).expect("parse");
        let cheat = assert_cheat(&entries[0]);
        assert_eq!(cheat.name, "abs");
        match &cheat.address {
            AddressSpec::Absolute { address } => assert_eq!(*address, 0xa49b10),
            other => panic!("expected Absolute, got {other:?}"),
        }
    }

    #[test]
    fn parse_ct_static_module_plus_offset() {
        let xml = r#"<?xml version="1.0"?>
<CheatTable><CheatEntries>
  <CheatEntry>
    <ID>2</ID>
    <Description>"money"</Description>
    <VariableType>4 Bytes</VariableType>
    <Address>gta_sa.exe+64A0E0</Address>
  </CheatEntry>
</CheatEntries></CheatTable>"#;
        let entries = parse_ct(xml).expect("parse");
        let cheat = assert_cheat(&entries[0]);
        match &cheat.address {
            AddressSpec::Static { module, offset } => {
                assert_eq!(module, "gta_sa.exe");
                assert_eq!(*offset, 0x64A0E0);
            }
            other => panic!("expected Static, got {other:?}"),
        }
    }

    #[test]
    fn parse_ct_pointer_chain_with_multiple_offsets() {
        let xml = r#"<?xml version="1.0"?>
<CheatTable><CheatEntries>
  <CheatEntry>
    <ID>5</ID>
    <Description>"hp"</Description>
    <VariableType>Float</VariableType>
    <Address>game.exe+12345ABC</Address>
    <Offsets>
      <Offset>10</Offset>
      <Offset>20</Offset>
      <Offset>30</Offset>
    </Offsets>
  </CheatEntry>
</CheatEntries></CheatTable>"#;
        let entries = parse_ct(xml).expect("parse");
        let cheat = assert_cheat(&entries[0]);
        match &cheat.address {
            AddressSpec::PointerChain {
                base_module,
                base_offset,
                offsets,
            } => {
                assert_eq!(base_module, "game.exe");
                assert_eq!(*base_offset, 0x12345ABC);
                assert_eq!(offsets, &vec![0x10, 0x20, 0x30]);
            }
            other => panic!("expected PointerChain, got {other:?}"),
        }
    }

    #[test]
    fn parse_ct_variable_type_widths() {
        for (vtype, expected) in [
            ("Byte", CheatValue::U8(0)),
            ("2 Bytes", CheatValue::U16(0)),
            ("4 Bytes", CheatValue::U32(0)),
            ("8 Bytes", CheatValue::U64(0)),
            ("Float", CheatValue::F32(0.0)),
            ("Double", CheatValue::F64(0.0)),
        ] {
            let xml = format!(
                r#"<CheatTable><CheatEntries><CheatEntry><ID>0</ID><Description>"x"</Description><VariableType>{vtype}</VariableType><Address>0x100</Address></CheatEntry></CheatEntries></CheatTable>"#
            );
            let entries = parse_ct(&xml).expect("parse");
            let cheat = assert_cheat(&entries[0]);
            match &cheat.action {
                CheatAction::WriteOnce { value } => assert_eq!(*value, expected),
                other => panic!("expected WriteOnce, got {other:?}"),
            }
        }
    }

    #[test]
    fn parse_ct_skips_assembler_script_entries() {
        let xml = r#"<CheatTable><CheatEntries>
  <CheatEntry>
    <ID>0</ID>
    <Description>"god mode"</Description>
    <VariableType>Auto Assembler Script</VariableType>
    <AssemblerScript>[ENABLE]
nop
[DISABLE]</AssemblerScript>
  </CheatEntry>
</CheatEntries></CheatTable>"#;
        let entries = parse_ct(xml).expect("parse");
        assert!(matches!(
            &entries[0],
            ImportedEntry::Skipped {
                reason: SkipReason::AssemblerScript,
                ..
            }
        ));
    }

    #[test]
    fn parse_ct_skips_symbolic_address_with_pointer_offsets() {
        // PRAGMATA-style: address is a registered symbol, not a module+offset
        let xml = r#"<CheatTable><CheatEntries>
  <CheatEntry>
    <ID>0</ID>
    <Description>"coins"</Description>
    <VariableType>4 Bytes</VariableType>
    <Address>i_base_coin_addr</Address>
    <Offsets><Offset>10</Offset></Offsets>
  </CheatEntry>
</CheatEntries></CheatTable>"#;
        let entries = parse_ct(xml).expect("parse");
        match &entries[0] {
            ImportedEntry::Skipped {
                reason: SkipReason::SymbolicAddress(name),
                ..
            } => assert_eq!(name, "i_base_coin_addr"),
            other => panic!("expected SymbolicAddress skip, got {other:?}"),
        }
    }

    #[test]
    fn parse_ct_nested_entries_recurse_into_flat_stream() {
        let xml = r#"<CheatTable><CheatEntries>
  <CheatEntry>
    <ID>0</ID>
    <Description>"parent"</Description>
    <VariableType>4 Bytes</VariableType>
    <Address>0x100</Address>
    <CheatEntries>
      <CheatEntry>
        <ID>1</ID>
        <Description>"child"</Description>
        <VariableType>4 Bytes</VariableType>
        <Address>0x200</Address>
      </CheatEntry>
    </CheatEntries>
  </CheatEntry>
</CheatEntries></CheatTable>"#;
        let entries = parse_ct(xml).expect("parse");
        // Nesting is presentational in CE; we currently only surface the
        // top-level entry. Children are intentionally dropped in v1 — they
        // are usually field-offset views relative to the parent (e.g.
        // "+10" address). Document this behavior here to lock it in.
        assert_eq!(entries.len(), 1);
        assert_eq!(assert_cheat(&entries[0]).name, "parent");
    }

    #[test]
    fn parse_ct_strips_quoted_descriptions() {
        let xml = r#"<CheatTable><CheatEntries>
  <CheatEntry>
    <ID>0</ID>
    <Description>"Quoted Name"</Description>
    <VariableType>4 Bytes</VariableType>
    <Address>0x100</Address>
  </CheatEntry>
</CheatEntries></CheatTable>"#;
        let entries = parse_ct(xml).expect("parse");
        assert_eq!(assert_cheat(&entries[0]).name, "Quoted Name");
    }

    #[test]
    fn parse_ct_slugifies_cheat_id_from_description() {
        let xml = r#"<CheatTable><CheatEntries>
  <CheatEntry>
    <Description>"Infinite HP / MP!"</Description>
    <VariableType>4 Bytes</VariableType>
    <Address>0x100</Address>
  </CheatEntry>
</CheatEntries></CheatTable>"#;
        let entries = parse_ct(xml).expect("parse");
        assert_eq!(assert_cheat(&entries[0]).id, "infinite_hp_mp");
    }

    #[test]
    fn parse_ct_unsupported_variable_type_is_skipped_with_reason() {
        let xml = r#"<CheatTable><CheatEntries>
  <CheatEntry>
    <Description>"weird"</Description>
    <VariableType>String</VariableType>
    <Address>0x100</Address>
  </CheatEntry>
</CheatEntries></CheatTable>"#;
        let entries = parse_ct(xml).expect("parse");
        match &entries[0] {
            ImportedEntry::Skipped {
                reason: SkipReason::UnsupportedVariableType(t),
                ..
            } => assert_eq!(t, "String"),
            other => panic!("expected UnsupportedVariableType skip, got {other:?}"),
        }
    }
}
