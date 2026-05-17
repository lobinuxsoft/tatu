use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheatTable {
    pub app_id: u64,
    pub game_name: String,
    pub exe_pattern: String,
    pub cheats: Vec<Cheat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cheat {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub address: AddressSpec,
    pub action: CheatAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AddressSpec {
    Static {
        module: String,
        #[serde(deserialize_with = "deserialize_hex_or_dec")]
        offset: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum CheatAction {
    WriteOnce { value: CheatValue },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum CheatValue {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl CheatValue {
    pub fn to_le_bytes(&self) -> Vec<u8> {
        match self {
            Self::U8(v) => v.to_le_bytes().to_vec(),
            Self::U16(v) => v.to_le_bytes().to_vec(),
            Self::U32(v) => v.to_le_bytes().to_vec(),
            Self::U64(v) => v.to_le_bytes().to_vec(),
            Self::I8(v) => v.to_le_bytes().to_vec(),
            Self::I16(v) => v.to_le_bytes().to_vec(),
            Self::I32(v) => v.to_le_bytes().to_vec(),
            Self::I64(v) => v.to_le_bytes().to_vec(),
            Self::F32(v) => v.to_le_bytes().to_vec(),
            Self::F64(v) => v.to_le_bytes().to_vec(),
        }
    }

    pub fn byte_size(&self) -> usize {
        match self {
            Self::U8(_) | Self::I8(_) => 1,
            Self::U16(_) | Self::I16(_) => 2,
            Self::U32(_) | Self::I32(_) | Self::F32(_) => 4,
            Self::U64(_) | Self::I64(_) | Self::F64(_) => 8,
        }
    }
}

fn deserialize_hex_or_dec<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))
        {
            Some(hex) => u64::from_str_radix(hex, 16).map_err(Error::custom),
            None => s.parse::<u64>().map_err(Error::custom),
        },
        serde_json::Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| Error::custom("offset must fit in u64")),
        _ => Err(Error::custom("offset must be a string or number")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cheat_table_roundtrip_with_hex_offset() {
        let json = r#"{
            "app_id": 49520,
            "game_name": "Borderlands 2",
            "exe_pattern": "Borderlands2.exe",
            "cheats": [{
                "id": "god_mode",
                "name": "God Mode",
                "address": { "kind": "Static", "module": "Borderlands2.exe", "offset": "0xABCD1234" },
                "action": { "kind": "WriteOnce", "value": { "type": "f32", "value": 9999.0 } }
            }]
        }"#;

        let table: CheatTable = serde_json::from_str(json).expect("parse table");
        assert_eq!(table.app_id, 49520);
        assert_eq!(table.cheats.len(), 1);
        assert_eq!(table.cheats[0].id, "god_mode");

        match &table.cheats[0].address {
            AddressSpec::Static { module, offset } => {
                assert_eq!(module, "Borderlands2.exe");
                assert_eq!(*offset, 0xABCD_1234);
            }
        }

        match &table.cheats[0].action {
            CheatAction::WriteOnce { value } => {
                assert_eq!(*value, CheatValue::F32(9999.0));
            }
        }
    }

    #[test]
    fn cheat_value_to_le_bytes() {
        assert_eq!(
            CheatValue::U32(0xDEAD_BEEF).to_le_bytes(),
            vec![0xEF, 0xBE, 0xAD, 0xDE]
        );
        assert_eq!(CheatValue::U8(0xAB).to_le_bytes(), vec![0xAB]);
        assert_eq!(
            CheatValue::F32(1.0).to_le_bytes(),
            vec![0x00, 0x00, 0x80, 0x3F]
        );
        assert_eq!(CheatValue::I16(-1).to_le_bytes(), vec![0xFF, 0xFF]);
    }

    #[test]
    fn cheat_value_byte_size() {
        assert_eq!(CheatValue::U32(0).byte_size(), 4);
        assert_eq!(CheatValue::F64(0.0).byte_size(), 8);
        assert_eq!(CheatValue::I8(0).byte_size(), 1);
        assert_eq!(CheatValue::U16(0).byte_size(), 2);
    }

    #[test]
    fn hex_offset_with_uppercase_prefix() {
        let json = r#"{ "kind": "Static", "module": "x", "offset": "0XCAFE" }"#;
        let spec: AddressSpec = serde_json::from_str(json).expect("parse");
        match spec {
            AddressSpec::Static { offset, .. } => assert_eq!(offset, 0xCAFE),
        }
    }

    #[test]
    fn decimal_offset_as_number() {
        let json = r#"{ "kind": "Static", "module": "x", "offset": 1234 }"#;
        let spec: AddressSpec = serde_json::from_str(json).expect("parse");
        match spec {
            AddressSpec::Static { offset, .. } => assert_eq!(offset, 1234),
        }
    }

    #[test]
    fn decimal_offset_as_string() {
        let json = r#"{ "kind": "Static", "module": "x", "offset": "1234" }"#;
        let spec: AddressSpec = serde_json::from_str(json).expect("parse");
        match spec {
            AddressSpec::Static { offset, .. } => assert_eq!(offset, 1234),
        }
    }
}
