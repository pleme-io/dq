//! JSON format: parse and serialize via serde_json.

use crate::Format;
use dq_core::{Error, Value};

pub struct JsonFormat;

impl Format for JsonFormat {
    fn parse(&self, input: &[u8]) -> Result<Value, Error> {
        let json: serde_json::Value = serde_json::from_slice(input)
            .map_err(|e| Error::Parse(format!("JSON: {e}")))?;
        Ok(Value::from(json))
    }

    fn serialize(&self, value: &Value) -> Result<Vec<u8>, Error> {
        let json = serde_json::Value::from(value);
        serde_json::to_vec_pretty(&json)
            .map_err(|e| Error::Format(format!("JSON serialize: {e}")))
    }
}
