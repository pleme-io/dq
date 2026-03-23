//! Native dq-specific filters that extend jq with domain knowledge.
//!
//! These filters are handled before delegating to jaq, because they
//! operate on dq_core::Value features (like HCL Blocks) that have no
//! jq equivalent.

use dq_core::{Error, Value};

/// Check if an expression is a native dq filter (not standard jq).
pub(crate) fn is_native(expr: &str) -> bool {
    expr.starts_with("find_blocks(") && expr.ends_with(')')
}

/// Execute a native dq filter.
pub(crate) fn eval_native(input: &Value, expr: &str) -> Result<Vec<Value>, Error> {
    if expr.starts_with("find_blocks(") && expr.ends_with(')') {
        let inner = expr
            .trim_start_matches("find_blocks(")
            .trim_end_matches(')');
        let block_type = inner.trim_matches('"');
        let blocks: Vec<Value> = input
            .find_blocks(block_type)
            .into_iter()
            .map(|b| Value::Block(b.clone()))
            .collect();
        Ok(vec![Value::array(blocks)])
    } else {
        Err(Error::Other(format!(
            "unknown native filter: {expr}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use std::sync::Arc;

    #[test]
    fn is_native_find_blocks() {
        assert!(is_native("find_blocks(\"resource\")"));
        assert!(!is_native(".name"));
        assert!(!is_native("keys"));
    }

    #[test]
    fn eval_find_blocks() {
        let v = Value::from_pairs([
            (
                "dep",
                Value::block("dependency", ["vpc"], {
                    let mut m = IndexMap::new();
                    m.insert(Arc::from("config_path"), Value::string("../vpc"));
                    m
                }),
            ),
        ]);
        let result = eval_native(&v, "find_blocks(\"dependency\")").unwrap();
        assert_eq!(result.len(), 1);
        let arr = result[0].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert!(arr[0].is_block());
    }
}
