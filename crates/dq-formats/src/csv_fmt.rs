//! CSV format — parses to array of maps (header row as keys).
//! Includes type inference: integers, floats, booleans, nulls.

use crate::Format;
use dq_core::{Error, Value};
use indexmap::IndexMap;
use std::sync::Arc;

pub struct CsvFormat;

impl Format for CsvFormat {
    fn parse(&self, input: &[u8]) -> Result<Value, Error> {
        let mut reader = csv::Reader::from_reader(input);
        let headers: Vec<String> = reader.headers()
            .map_err(|e| Error::Parse(format!("CSV headers: {e}")))?
            .iter().map(String::from).collect();

        let mut rows = Vec::new();
        for result in reader.records() {
            let record = result.map_err(|e| Error::Parse(format!("CSV row: {e}")))?;
            let map: IndexMap<Arc<str>, Value> = headers.iter()
                .zip(record.iter())
                .map(|(h, v)| (Arc::from(h.as_str()), infer_cell_type(v)))
                .collect();
            rows.push(Value::map(map));
        }

        Ok(Value::array(rows))
    }

    fn serialize(&self, value: &Value) -> Result<Vec<u8>, Error> {
        let rows = value.as_array()
            .ok_or_else(|| Error::Format("CSV serialize: expected array of objects".into()))?;

        let mut buf = Vec::new();
        let mut writer = csv::Writer::from_writer(&mut buf);

        // Extract headers from first row
        if let Some(first) = rows.first() {
            let headers: Vec<String> = first.keys().iter().map(|k| k.to_string()).collect();
            writer.write_record(&headers)
                .map_err(|e| Error::Format(format!("CSV write headers: {e}")))?;

            for row in rows {
                let record: Vec<String> = headers.iter()
                    .map(|h| row.get(h).map(|v| v.to_string()).unwrap_or_default())
                    .collect();
                writer.write_record(&record)
                    .map_err(|e| Error::Format(format!("CSV write row: {e}")))?;
            }
        }

        writer.flush().map_err(|e| Error::Format(format!("CSV flush: {e}")))?;
        drop(writer);
        Ok(buf)
    }
}

/// Infer the type of a CSV cell value.
/// Tries i64, then f64, then bool, then empty → Null, else String.
fn infer_cell_type(s: &str) -> Value {
    if s.is_empty() {
        return Value::Null;
    }
    if let Ok(i) = s.parse::<i64>() {
        return Value::Int(i);
    }
    if let Ok(f) = s.parse::<f64>() {
        return Value::Float(f);
    }
    match s.to_lowercase().as_str() {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => Value::string(s),
    }
}
