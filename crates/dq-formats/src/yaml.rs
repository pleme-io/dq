//! YAML format via serde-saphyr (panic-free serde_yaml replacement).

use crate::Format;
use dq_core::{Error, Value};

pub struct YamlFormat;

impl Format for YamlFormat {
    fn parse(&self, input: &[u8]) -> Result<Value, Error> {
        let s = std::str::from_utf8(input)
            .map_err(|e| Error::Parse(format!("YAML: invalid UTF-8: {e}")))?;
        let json: serde_json::Value = serde_saphyr::from_str(s)
            .map_err(|e| Error::Parse(format!("YAML: {e}")))?;
        Ok(Value::from(json))
    }

    fn serialize(&self, value: &Value) -> Result<Vec<u8>, Error> {
        let json = serde_json::Value::from(value);
        serde_saphyr::to_string(&json)
            .map(|s| s.into_bytes())
            .map_err(|e| Error::Format(format!("YAML serialize: {e}")))
    }
}
