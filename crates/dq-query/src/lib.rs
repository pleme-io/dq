//! # dq-query -- jq Query Engine for dq Values
//!
//! Provides jq-style query expressions against [`dq_core::Value`].
//! Uses jaq-core 2.x as the underlying jq implementation, with native
//! extensions for dq-specific filters like `find_blocks`.
//!
//! ## Architecture
//!
//! - [`jaq_bridge`] -- parse, compile, and execute jq expressions via jaq-core
//! - [`native`] -- dq-specific filters (`find_blocks`) handled before jaq delegation
//! - [`query`] -- public entry point that dispatches to native or jaq

mod jaq_bridge;
mod native;

use dq_core::{Error, Value};

/// Execute a jq-style expression against a Value.
/// Returns all output values (expressions can produce multiple outputs).
///
/// Expressions containing dq-specific filters like `find_blocks(...)` are
/// handled natively. If a pipe chain contains `find_blocks`, the chain is
/// split: the native portion runs first, then the remainder is passed to jaq.
/// All other expressions are delegated directly to jaq-core.
pub fn query(input: &Value, expression: &str) -> Result<Vec<Value>, Error> {
    let expr = expression.trim();

    if expr.is_empty() || expr == "." {
        return Ok(vec![input.clone()]);
    }

    // Check if the expression contains any native filters.
    // If so, we need to handle pipe splitting ourselves.
    if contains_native(expr) {
        return eval_with_native(input, expr);
    }

    // Pure jq expression: delegate entirely to jaq
    jaq_bridge::run_jaq(input, expr)
}

/// Check whether an expression (possibly a pipe chain) contains native filters.
fn contains_native(expr: &str) -> bool {
    // Scan for find_blocks outside of strings
    let mut in_string = false;
    let mut escape = false;
    let bytes = expr.as_bytes();
    let target = b"find_blocks(";

    for i in 0..bytes.len() {
        if escape {
            escape = false;
            continue;
        }
        if bytes[i] == b'\\' {
            escape = true;
            continue;
        }
        if bytes[i] == b'"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if bytes[i..].starts_with(target) {
            return true;
        }
    }
    false
}

/// Evaluate an expression that contains native dq filters.
/// Handles pipe chains by splitting at top-level pipes and dispatching
/// each segment to either native or jaq evaluation.
fn eval_with_native(input: &Value, expr: &str) -> Result<Vec<Value>, Error> {
    let segments = split_pipe_chain(expr);

    let mut current = vec![input.clone()];

    for segment in &segments {
        let seg = segment.trim();
        let mut next = Vec::new();
        for val in &current {
            if native::is_native(seg) {
                next.extend(native::eval_native(val, seg)?);
            } else {
                next.extend(jaq_bridge::run_jaq(val, seg)?);
            }
        }
        current = next;
    }

    Ok(current)
}

/// Split a pipe chain at top-level `|` characters (not inside brackets,
/// parens, or strings). Returns the segments.
fn split_pipe_chain(s: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let mut start = 0;

    for (i, ch) in s.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' {
            escape = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '|' if depth == 0 => {
                segments.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    segments.push(&s[start..]);
    segments
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use serde_json::json;
    use std::sync::Arc;

    // ── Basic jq ──────────────────────────────────────────────────────

    #[test]
    fn test_identity() {
        let input = Value::from(json!({"a": 1}));
        let result = query(&input, ".").unwrap();
        assert_eq!(result, vec![input]);
    }

    #[test]
    fn test_field_access() {
        let input = Value::from(json!({"name": "alice", "age": 30}));
        let result = query(&input, ".name").unwrap();
        assert_eq!(result, vec![Value::from(json!("alice"))]);
    }

    #[test]
    fn test_nested_access() {
        let input = Value::from(json!({"a": {"b": {"c": 42}}}));
        let result = query(&input, ".a.b.c").unwrap();
        assert_eq!(result, vec![Value::from(json!(42))]);
    }

    #[test]
    fn test_array_index() {
        let input = Value::from(json!([10, 20, 30]));
        let result = query(&input, ".[1]").unwrap();
        assert_eq!(result, vec![Value::from(json!(20))]);
    }

    #[test]
    fn test_iterator() {
        let input = Value::from(json!([1, 2, 3]));
        let result = query(&input, ".[]").unwrap();
        assert_eq!(
            result,
            vec![
                Value::from(json!(1)),
                Value::from(json!(2)),
                Value::from(json!(3)),
            ]
        );
    }

    #[test]
    fn test_keys() {
        let input = Value::from(json!({"b": 2, "a": 1}));
        let result = query(&input, "keys").unwrap();
        assert_eq!(result.len(), 1);
        // jq keys returns sorted keys
        let arr = result[0].as_array().unwrap();
        let keys: Vec<&str> = arr.iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(keys, vec!["a", "b"]);
    }

    #[test]
    fn test_values() {
        let input = Value::from(json!({"a": 1, "b": 2}));
        let result = query(&input, "[.[] | . ]").unwrap();
        assert_eq!(result.len(), 1);
        let arr = result[0].as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn test_length() {
        let input = Value::from(json!([1, 2, 3, 4]));
        let result = query(&input, "length").unwrap();
        assert_eq!(result, vec![Value::from(json!(4))]);
    }

    #[test]
    fn test_type() {
        let input = Value::from(json!("hello"));
        let result = query(&input, "type").unwrap();
        assert_eq!(result, vec![Value::from(json!("string"))]);
    }

    // ── Pipes ─────────────────────────────────────────────────────────

    #[test]
    fn test_pipe_chain() {
        let input = Value::from(json!({"a": 1, "b": 2}));
        let result = query(&input, ". | keys").unwrap();
        assert_eq!(result.len(), 1);
        let arr = result[0].as_array().unwrap();
        let keys: Vec<&str> = arr.iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(keys, vec!["a", "b"]);
    }

    #[test]
    fn test_select() {
        let input = Value::from(json!([1, 2, 3, 4, 5]));
        let result = query(&input, "[.[] | select(. > 2)]").unwrap();
        assert_eq!(result, vec![Value::from(json!([3, 4, 5]))]);
    }

    #[test]
    fn test_map() {
        let input = Value::from(json!([1, 2, 3]));
        let result = query(&input, "[.[] | . + 1]").unwrap();
        assert_eq!(result, vec![Value::from(json!([2, 3, 4]))]);
    }

    #[test]
    fn test_to_entries() {
        let input = Value::from(json!({"a": 1}));
        let result = query(&input, "to_entries").unwrap();
        assert_eq!(
            result,
            vec![Value::from(json!([{"key": "a", "value": 1}]))]
        );
    }

    #[test]
    fn test_from_entries() {
        let input = Value::from(json!([{"key": "a", "value": 1}]));
        let result = query(&input, "from_entries").unwrap();
        assert_eq!(result, vec![Value::from(json!({"a": 1}))]);
    }

    // ── Advanced ──────────────────────────────────────────────────────

    #[test]
    fn test_object_construction() {
        let input = Value::from(json!({"x": 1, "y": 2}));
        let result = query(&input, "{a: .x, b: .y}").unwrap();
        assert_eq!(result, vec![Value::from(json!({"a": 1, "b": 2}))]);
    }

    #[test]
    fn test_string_interpolation() {
        let input = Value::from(json!({"name": "world"}));
        let result = query(&input, r#""hello \(.name)""#).unwrap();
        assert_eq!(result, vec![Value::from(json!("hello world"))]);
    }

    #[test]
    fn test_try_catch() {
        let input = Value::from(json!(null));
        let result = query(&input, "try error catch .").unwrap();
        assert_eq!(result, vec![Value::from(json!(null))]);
    }

    #[test]
    fn test_reduce() {
        let input = Value::from(json!([1, 2, 3, 4, 5]));
        let result = query(&input, "reduce .[] as $x (0; . + $x)").unwrap();
        assert_eq!(result, vec![Value::from(json!(15))]);
    }

    #[test]
    fn test_group_by() {
        let input = Value::from(json!([
            {"name": "a", "group": 1},
            {"name": "b", "group": 2},
            {"name": "c", "group": 1}
        ]));
        let result = query(&input, "group_by(.group)").unwrap();
        assert_eq!(result.len(), 1);
        let groups = result[0].as_array().unwrap();
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn test_sort_by() {
        let input = Value::from(json!([{"a": 3}, {"a": 1}, {"a": 2}]));
        let result = query(&input, "sort_by(.a)").unwrap();
        assert_eq!(
            result,
            vec![Value::from(json!([{"a": 1}, {"a": 2}, {"a": 3}]))]
        );
    }

    // ── dq extensions ─────────────────────────────────────────────────

    #[test]
    fn test_find_blocks() {
        let v = Value::from_pairs([(
            "dep",
            Value::block("dependency", ["vpc"], {
                let mut m = IndexMap::new();
                m.insert(Arc::from("config_path"), Value::string("../vpc"));
                m
            }),
        )]);
        let result = query(&v, "find_blocks(\"dependency\")").unwrap();
        assert_eq!(result.len(), 1);
        let arr = result[0].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert!(arr[0].is_block());
    }

    #[test]
    fn test_find_blocks_in_pipe() {
        let v = Value::from_pairs([(
            "dep",
            Value::block("dependency", ["vpc"], {
                let mut m = IndexMap::new();
                m.insert(Arc::from("config_path"), Value::string("../vpc"));
                m
            }),
        )]);
        let result = query(&v, "find_blocks(\"dependency\") | length").unwrap();
        assert_eq!(result, vec![Value::from(json!(1))]);
    }

    #[test]
    fn test_invalid_expression() {
        let input = Value::from(json!(null));
        let err = query(&input, ".[[[invalid").unwrap_err();
        match err {
            Error::Parse(_) => {} // expected
            other => panic!("expected Parse error, got: {other}"),
        }
    }
}
