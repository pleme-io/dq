//! MessagePack format via rmp-serde.

use crate::Format;
use dq_core::{Error, Value};

pub struct MsgPackFormat;

impl Format for MsgPackFormat {
    fn parse(&self, input: &[u8]) -> Result<Value, Error> {
        let json: serde_json::Value = rmp_serde::from_slice(input)
            .map_err(|e| Error::Parse(format!("MessagePack: {e}")))?;
        Ok(Value::from(json))
    }

    fn serialize(&self, value: &Value) -> Result<Vec<u8>, Error> {
        let json = serde_json::Value::from(value);
        rmp_serde::to_vec(&json)
            .map_err(|e| Error::Format(format!("MessagePack serialize: {e}")))
    }
}
