//! TOML format via the `toml` crate. Preserves datetime types.

use crate::Format;
use dq_core::{Error, Value};
use indexmap::IndexMap;
use std::sync::Arc;

pub struct TomlFormat;

impl Format for TomlFormat {
    fn parse(&self, input: &[u8]) -> Result<Value, Error> {
        let s = std::str::from_utf8(input)
            .map_err(|e| Error::Parse(format!("TOML: invalid UTF-8: {e}")))?;
        let toml_val: toml::Value = toml::from_str(s)
            .map_err(|e| Error::Parse(format!("TOML: {e}")))?;
        Ok(from_toml(toml_val))
    }

    fn serialize(&self, value: &Value) -> Result<Vec<u8>, Error> {
        let toml_val = to_toml(value)?;
        toml::to_string_pretty(&toml_val)
            .map(|s| s.into_bytes())
            .map_err(|e| Error::Format(format!("TOML serialize: {e}")))
    }
}

fn from_toml(v: toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::string(s),
        toml::Value::Integer(i) => Value::Int(i),
        toml::Value::Float(f) => Value::Float(f),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(dt) => Value::datetime(dt.to_string()),
        toml::Value::Array(a) => Value::array(a.into_iter().map(from_toml).collect::<Vec<_>>()),
        toml::Value::Table(t) => {
            let map: IndexMap<Arc<str>, Value> = t.into_iter()
                .map(|(k, v)| (Arc::from(k.as_str()), from_toml(v)))
                .collect();
            Value::map(map)
        }
    }
}

fn to_toml(v: &Value) -> Result<toml::Value, Error> {
    Ok(match v {
        // TOML has no null type — this case is only reached for top-level or
        // array null elements.  Map entries with null values are omitted in
        // to_toml_map below, matching yq behavior.
        Value::Null => toml::Value::String("null".into()),
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::Int(n) => toml::Value::Integer(*n),
        Value::Float(f) => toml::Value::Float(*f),
        Value::String(s) => toml::Value::String(s.to_string()),
        Value::Datetime(s) => {
            s.parse::<toml::value::Datetime>()
                .map(toml::Value::Datetime)
                .unwrap_or_else(|_| toml::Value::String(s.to_string()))
        }
        Value::Bytes(_) => return Err(Error::Format("TOML does not support binary data".into())),
        Value::Array(a) => toml::Value::Array(
            a.iter()
                .filter(|v| !v.is_null())
                .map(to_toml)
                .collect::<Result<_, _>>()?
        ),
        Value::Map(m) => toml::Value::Table(to_toml_map(m)?),
        Value::Block(b) => toml::Value::Table(to_toml_map(&b.body)?),
    })
}

/// Convert a map to TOML table, omitting null-valued keys (TOML has no null).
fn to_toml_map(m: &IndexMap<Arc<str>, Value>) -> Result<toml::map::Map<String, toml::Value>, Error> {
    m.iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| Ok((k.to_string(), to_toml(v)?)))
        .collect::<Result<_, Error>>()
}
