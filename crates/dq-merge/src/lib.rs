//! # dq-merge — Deep Merge, Patch, Diff, Flatten
//!
//! Operations for combining and transforming [`dq_core::Value`] trees.
//! Implements the merge strategies from Terragrunt (shallow, deep, no_merge)
//! and Helm values composition (layered override).
//!
//! Also provides RFC 7396 (JSON Merge Patch), RFC 6902 (JSON Patch),
//! Kubernetes-style strategic merge, and three-way merge with conflict detection.

use dq_core::{Error, Value};
use indexmap::IndexMap;
use std::sync::Arc;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Conversion helpers (dq Value <-> serde_json::Value)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

fn to_json(v: &Value) -> serde_json::Value {
    serde_json::Value::from(v)
}

fn from_json(v: serde_json::Value) -> Value {
    Value::from(v)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Strategy enum
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Merge strategy (matches Terragrunt include merge semantics).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Strategy {
    /// Child keys override parent. Arrays are replaced, not concatenated.
    Shallow,
    /// Recursive merge: maps are merged, arrays are concatenated.
    Deep,
    /// No merge — return the override as-is.
    Replace,
    /// RFC 7396 JSON Merge Patch: null removes key, objects merge recursively.
    JsonMergePatch,
    /// Kubernetes-style strategic merge: arrays merge by designated key field.
    /// `merge_keys` maps a dotted path prefix to the key field used for array
    /// element identity (e.g., `"containers" -> "name"`).
    Strategic { merge_keys: Vec<(String, String)> },
}

/// Deep merge two values. `override_val` takes precedence.
pub fn merge(base: &Value, override_val: &Value, strategy: Strategy) -> Value {
    match strategy {
        Strategy::Replace => override_val.clone(),
        Strategy::Shallow => shallow_merge(base, override_val),
        Strategy::Deep => deep_merge(base, override_val),
        Strategy::JsonMergePatch => json_merge_patch(base, override_val),
        Strategy::Strategic { ref merge_keys } => {
            strategic_merge(base, override_val, merge_keys)
        }
    }
}

/// Merge a stack of values (e.g., Helm values hierarchy: global -> tenant -> region).
/// First value is lowest priority, last is highest.
pub fn merge_stack(values: &[Value], strategy: Strategy) -> Value {
    values.iter().fold(Value::Null, |acc, v| {
        if acc.is_null() {
            v.clone()
        } else {
            merge(&acc, v, strategy.clone())
        }
    })
}

fn shallow_merge(base: &Value, over: &Value) -> Value {
    match (base, over) {
        (Value::Map(base_map), Value::Map(over_map)) => {
            let mut result = base_map.as_ref().clone();
            for (k, v) in over_map.iter() {
                result.insert(k.clone(), v.clone());
            }
            Value::Map(Arc::new(result))
        }
        _ => over.clone(),
    }
}

fn deep_merge(base: &Value, over: &Value) -> Value {
    match (base, over) {
        (Value::Map(base_map), Value::Map(over_map)) => {
            let mut result = base_map.as_ref().clone();
            for (k, v) in over_map.iter() {
                let merged = if let Some(base_v) = result.get(k) {
                    deep_merge(base_v, v)
                } else {
                    v.clone()
                };
                result.insert(k.clone(), merged);
            }
            Value::Map(Arc::new(result))
        }
        (Value::Array(base_arr), Value::Array(over_arr)) => {
            let mut result = base_arr.as_ref().clone();
            result.extend(over_arr.iter().cloned());
            Value::Array(Arc::new(result))
        }
        _ => over.clone(),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Diff
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Compute the diff between two values.
/// Returns a Value representing only the changes (keys present in `b` but
/// different from `a`).
pub fn diff(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Map(am), Value::Map(bm)) => {
            let mut result = IndexMap::new();
            for (k, bv) in bm.iter() {
                match am.get(k) {
                    Some(av) if av == bv => {} // same, skip
                    Some(av) => {
                        let d = diff(av, bv);
                        if !d.is_null() {
                            result.insert(k.clone(), d);
                        }
                    }
                    None => {
                        result.insert(k.clone(), bv.clone());
                    }
                }
            }
            // Keys removed in b
            for k in am.keys() {
                if !bm.contains_key(k) {
                    result.insert(k.clone(), Value::Null);
                }
            }
            if result.is_empty() {
                Value::Null
            } else {
                Value::Map(Arc::new(result))
            }
        }
        (a, b) if a == b => Value::Null,
        (_, b) => b.clone(),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Flatten / Unflatten
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Flatten a nested map into dot-separated keys.
/// `{"a": {"b": 1, "c": {"d": 2}}}` -> `{"a.b": 1, "a.c.d": 2}`
pub fn flatten_keys(value: &Value, separator: &str) -> Value {
    let mut result = IndexMap::new();
    flatten_inner(value, "", separator, &mut result);
    Value::Map(Arc::new(result))
}

fn flatten_inner(value: &Value, prefix: &str, sep: &str, out: &mut IndexMap<Arc<str>, Value>) {
    match value {
        Value::Map(m) => {
            for (k, v) in m.iter() {
                let key = if prefix.is_empty() {
                    k.to_string()
                } else {
                    format!("{prefix}{sep}{k}")
                };
                flatten_inner(v, &key, sep, out);
            }
        }
        Value::Block(b) => {
            for (k, v) in b.body.iter() {
                let key = if prefix.is_empty() {
                    k.to_string()
                } else {
                    format!("{prefix}{sep}{k}")
                };
                flatten_inner(v, &key, sep, out);
            }
        }
        leaf => {
            out.insert(Arc::from(prefix), leaf.clone());
        }
    }
}

/// Unflatten dot-separated keys back into nested maps.
/// `{"a.b": 1, "a.c.d": 2}` -> `{"a": {"b": 1, "c": {"d": 2}}}`
pub fn unflatten_keys(value: &Value, separator: &str) -> Value {
    let map = match value.as_map() {
        Some(m) => m,
        None => return value.clone(),
    };

    let mut root = IndexMap::new();
    for (key, val) in map.iter() {
        let parts: Vec<&str> = key.split(separator).collect();
        insert_nested(&mut root, &parts, val.clone());
    }
    Value::Map(Arc::new(root))
}

fn insert_nested(map: &mut IndexMap<Arc<str>, Value>, parts: &[&str], val: Value) {
    if parts.len() == 1 {
        map.insert(Arc::from(parts[0]), val);
        return;
    }
    let key = Arc::from(parts[0]);
    let entry = map
        .entry(key)
        .or_insert_with(|| Value::map(IndexMap::new()));
    if let Value::Map(inner) = entry {
        let mut inner_mut = inner.as_ref().clone();
        insert_nested(&mut inner_mut, &parts[1..], val);
        *entry = Value::Map(Arc::new(inner_mut));
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// RFC 7396 — JSON Merge Patch
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Apply an RFC 7396 JSON Merge Patch.
///
/// - If `patch` is an object, recursively merge it into `base`.
/// - A `null` value in `patch` removes the corresponding key from `base`.
/// - Non-object `patch` values replace `base` entirely.
///
/// Delegates to `json_patch::merge` after converting through `serde_json::Value`.
pub fn json_merge_patch(base: &Value, patch: &Value) -> Value {
    let mut target = to_json(base);
    let patch_json = to_json(patch);
    json_patch::merge(&mut target, &patch_json);
    from_json(target)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// RFC 6902 — JSON Patch
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Apply RFC 6902 JSON Patch operations to a target value.
///
/// `operations` should be a `Value::Array` of patch operation objects, each
/// containing `"op"`, `"path"`, and optionally `"value"` / `"from"` fields.
///
/// Example operation: `{"op":"add","path":"/a/b","value":1}`
///
/// Returns the patched value, or an error if any operation fails.
pub fn json_patch(target: &Value, operations: &Value) -> Result<Value, Error> {
    let ops_array = match operations.as_array() {
        Some(arr) => arr,
        None => {
            return Err(Error::TypeError {
                expected: "array",
                actual: operations.type_name(),
            });
        }
    };

    let ops_json: Vec<serde_json::Value> = ops_array.iter().map(|v| to_json(v)).collect();
    let ops_json_array = serde_json::Value::Array(ops_json);

    let patch: json_patch::Patch = serde_json::from_value(ops_json_array).map_err(|e| {
        Error::Parse(format!("invalid JSON Patch operations: {e}"))
    })?;

    let mut doc = to_json(target);
    json_patch::patch(&mut doc, &patch).map_err(|e| {
        Error::Other(format!("JSON Patch application failed: {e}"))
    })?;

    Ok(from_json(doc))
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Strategic Merge (Kubernetes-style)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Kubernetes-style strategic merge.
///
/// For maps: recurse into matching keys, override non-map values.
/// For arrays: if a merge key is specified for the current path segment,
/// merge array elements by matching on that key field (like K8s merges
/// containers by `name`). Without a merge key, arrays replace entirely
/// (Helm behavior).
///
/// `merge_keys` is a list of `(path_segment, key_field)` pairs. When an
/// array is encountered under a map key matching `path_segment`, elements
/// are matched by `key_field` and merged recursively.
pub fn strategic_merge(
    base: &Value,
    override_val: &Value,
    merge_keys: &[(String, String)],
) -> Value {
    strategic_merge_inner(base, override_val, merge_keys, "")
}

fn strategic_merge_inner(
    base: &Value,
    over: &Value,
    merge_keys: &[(String, String)],
    current_key: &str,
) -> Value {
    match (base, over) {
        (Value::Map(base_map), Value::Map(over_map)) => {
            let mut result = base_map.as_ref().clone();
            for (k, v) in over_map.iter() {
                let merged = if let Some(base_v) = result.get(k) {
                    strategic_merge_inner(base_v, v, merge_keys, k)
                } else {
                    v.clone()
                };
                result.insert(k.clone(), merged);
            }
            Value::Map(Arc::new(result))
        }
        (Value::Array(base_arr), Value::Array(over_arr)) => {
            // Check if there is a merge key for the current path segment
            let key_field = merge_keys
                .iter()
                .find(|(segment, _)| segment == current_key)
                .map(|(_, field)| field.as_str());

            match key_field {
                Some(field) => {
                    // Merge arrays by key field: match elements by their key value
                    let mut result: Vec<Value> = base_arr.as_ref().clone();
                    for over_elem in over_arr.iter() {
                        let over_key_val = over_elem.get(field);
                        if let Some(over_kv) = over_key_val {
                            // Find matching element in base by key field
                            let mut found = false;
                            for base_elem in result.iter_mut() {
                                if base_elem.get(field) == Some(over_kv) {
                                    *base_elem = strategic_merge_inner(
                                        base_elem,
                                        over_elem,
                                        merge_keys,
                                        "",
                                    );
                                    found = true;
                                    break;
                                }
                            }
                            if !found {
                                result.push(over_elem.clone());
                            }
                        } else {
                            // Element has no key field, append
                            result.push(over_elem.clone());
                        }
                    }
                    Value::Array(Arc::new(result))
                }
                None => {
                    // No merge key: arrays replace entirely (Helm behavior)
                    over.clone()
                }
            }
        }
        _ => over.clone(),
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Three-Way Merge
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// Three-way merge: given a common `base`, merge `left` and `right` changes.
///
/// Computes `diff(base, left)` and `diff(base, right)`, then applies
/// non-conflicting changes. Returns `Err(MergeConflict)` if both sides
/// modified the same key to different values.
pub fn three_way_merge(base: &Value, left: &Value, right: &Value) -> Result<Value, Error> {
    three_way_inner(base, left, right, "")
}

fn three_way_inner(
    base: &Value,
    left: &Value,
    right: &Value,
    path: &str,
) -> Result<Value, Error> {
    match (base, left, right) {
        (Value::Map(bm), Value::Map(lm), Value::Map(rm)) => {
            // Collect all keys from all three maps
            let mut all_keys: Vec<Arc<str>> = Vec::new();
            for k in bm.keys() {
                all_keys.push(k.clone());
            }
            for k in lm.keys() {
                if !all_keys.iter().any(|existing| existing == k) {
                    all_keys.push(k.clone());
                }
            }
            for k in rm.keys() {
                if !all_keys.iter().any(|existing| existing == k) {
                    all_keys.push(k.clone());
                }
            }

            let mut result = IndexMap::new();
            for k in &all_keys {
                let base_v = bm.get(k);
                let left_v = lm.get(k);
                let right_v = rm.get(k);

                let child_path = if path.is_empty() {
                    k.to_string()
                } else {
                    format!("{path}.{k}")
                };

                let left_changed = left_v != base_v;
                let right_changed = right_v != base_v;

                match (left_changed, right_changed) {
                    (false, false) => {
                        // Neither side changed; keep base value (if it exists)
                        if let Some(v) = base_v {
                            result.insert(k.clone(), v.clone());
                        }
                    }
                    (true, false) => {
                        // Only left changed
                        if let Some(v) = left_v {
                            result.insert(k.clone(), v.clone());
                        }
                        // If left_v is None, key was removed by left
                    }
                    (false, true) => {
                        // Only right changed
                        if let Some(v) = right_v {
                            result.insert(k.clone(), v.clone());
                        }
                        // If right_v is None, key was removed by right
                    }
                    (true, true) => {
                        // Both changed — check if they agree
                        if left_v == right_v {
                            // Both made the same change
                            if let Some(v) = left_v {
                                result.insert(k.clone(), v.clone());
                            }
                        } else {
                            // Both changed differently — try recursive merge if maps
                            match (base_v, left_v, right_v) {
                                (Some(bv), Some(lv), Some(rv))
                                    if bv.is_map() && lv.is_map() && rv.is_map() =>
                                {
                                    let merged = three_way_inner(bv, lv, rv, &child_path)?;
                                    result.insert(k.clone(), merged);
                                }
                                // Base didn't have it, both sides added maps
                                (None, Some(lv), Some(rv))
                                    if lv.is_map() && rv.is_map() =>
                                {
                                    let empty = Value::empty_map();
                                    let merged =
                                        three_way_inner(&empty, lv, rv, &child_path)?;
                                    result.insert(k.clone(), merged);
                                }
                                _ => {
                                    return Err(Error::MergeConflict {
                                        path: child_path,
                                        message: "both sides modified this key differently"
                                            .to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
            Ok(Value::Map(Arc::new(result)))
        }
        _ => {
            // Non-map values: check for conflicts at the scalar/array level
            let left_changed = left != base;
            let right_changed = right != base;

            match (left_changed, right_changed) {
                (false, false) => Ok(base.clone()),
                (true, false) => Ok(left.clone()),
                (false, true) => Ok(right.clone()),
                (true, true) => {
                    if left == right {
                        Ok(left.clone())
                    } else {
                        Err(Error::MergeConflict {
                            path: path.to_string(),
                            message: "both sides modified this value differently".to_string(),
                        })
                    }
                }
            }
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helpers ────────────────────────────────────────────────────

    fn map(pairs: &[(&str, Value)]) -> Value {
        Value::from_pairs(pairs.iter().map(|(k, v)| (*k, v.clone())))
    }

    // ── Existing operations (8 tests) ─────────────────────────────

    #[test]
    fn test_shallow_merge() {
        let base = map(&[
            ("a", Value::int(1)),
            ("b", map(&[("x", Value::int(10))])),
        ]);
        let over = map(&[
            ("b", map(&[("y", Value::int(20))])),
            ("c", Value::int(3)),
        ]);
        let result = merge(&base, &over, Strategy::Shallow);
        // Shallow: "b" is replaced entirely (not recursed into)
        assert_eq!(result.get("a"), Some(&Value::int(1)));
        assert_eq!(
            result.get("b"),
            Some(&map(&[("y", Value::int(20))]))
        );
        assert_eq!(result.get("c"), Some(&Value::int(3)));
    }

    #[test]
    fn test_deep_merge() {
        let base = map(&[
            ("a", Value::int(1)),
            ("b", map(&[("x", Value::int(10))])),
        ]);
        let over = map(&[
            ("b", map(&[("y", Value::int(20))])),
            ("c", Value::int(3)),
        ]);
        let result = merge(&base, &over, Strategy::Deep);
        // Deep: "b" is recursively merged
        assert_eq!(result.get("a"), Some(&Value::int(1)));
        let b = result.get("b").unwrap();
        assert_eq!(b.get("x"), Some(&Value::int(10)));
        assert_eq!(b.get("y"), Some(&Value::int(20)));
        assert_eq!(result.get("c"), Some(&Value::int(3)));
    }

    #[test]
    fn test_replace_merge() {
        let base = map(&[("a", Value::int(1)), ("b", Value::int(2))]);
        let over = map(&[("c", Value::int(3))]);
        let result = merge(&base, &over, Strategy::Replace);
        // Replace: override wins entirely
        assert_eq!(result, over);
    }

    #[test]
    fn test_merge_stack() {
        let global = map(&[("region", Value::string("us-east-1")), ("env", Value::string("prod"))]);
        let tenant = map(&[("env", Value::string("staging"))]);
        let local = map(&[("debug", Value::bool(true))]);
        let result = merge_stack(&[global, tenant, local], Strategy::Shallow);
        assert_eq!(result.get("region"), Some(&Value::string("us-east-1")));
        assert_eq!(result.get("env"), Some(&Value::string("staging")));
        assert_eq!(result.get("debug"), Some(&Value::bool(true)));
    }

    #[test]
    fn test_diff_identical() {
        let a = map(&[("x", Value::int(1)), ("y", Value::int(2))]);
        assert_eq!(diff(&a, &a), Value::Null);
    }

    #[test]
    fn test_diff_added_key() {
        let a = map(&[("x", Value::int(1))]);
        let b = map(&[("x", Value::int(1)), ("y", Value::int(2))]);
        let d = diff(&a, &b);
        assert_eq!(d, map(&[("y", Value::int(2))]));
    }

    #[test]
    fn test_diff_removed_key() {
        let a = map(&[("x", Value::int(1)), ("y", Value::int(2))]);
        let b = map(&[("x", Value::int(1))]);
        let d = diff(&a, &b);
        assert_eq!(d, map(&[("y", Value::Null)]));
    }

    #[test]
    fn test_diff_changed_value() {
        let a = map(&[("x", Value::int(1))]);
        let b = map(&[("x", Value::int(42))]);
        let d = diff(&a, &b);
        assert_eq!(d, map(&[("x", Value::int(42))]));
    }

    // ── Flatten / Unflatten (3 tests) ─────────────────────────────

    #[test]
    fn test_flatten_custom_separator() {
        let v = map(&[("a", map(&[("b", Value::int(1))]))]);
        let flat = flatten_keys(&v, "/");
        assert_eq!(flat.get("a/b"), Some(&Value::int(1)));
    }

    #[test]
    fn test_flatten_unflatten_roundtrip() {
        let original = map(&[
            ("a", map(&[
                ("b", Value::int(1)),
                ("c", map(&[("d", Value::int(2))])),
            ])),
        ]);
        let flat = flatten_keys(&original, ".");
        let restored = unflatten_keys(&flat, ".");
        assert_eq!(original, restored);
    }

    #[test]
    fn test_flatten_block_body() {
        let b = Value::block("resource", ["aws_instance"], {
            let mut m = IndexMap::new();
            m.insert(Arc::from("ami"), Value::string("ami-123"));
            m.insert(Arc::from("type"), Value::string("t3.micro"));
            m
        });
        let flat = flatten_keys(&b, ".");
        assert_eq!(flat.get("ami"), Some(&Value::string("ami-123")));
        assert_eq!(flat.get("type"), Some(&Value::string("t3.micro")));
    }

    // ── RFC 7396 JSON Merge Patch (4 tests) ───────────────────────

    #[test]
    fn test_rfc7396_null_removes_key() {
        let base = map(&[("a", Value::int(1)), ("b", Value::int(2))]);
        let patch = map(&[("a", Value::Null)]);
        let result = json_merge_patch(&base, &patch);
        assert_eq!(result.get("a"), None);
        assert_eq!(result.get("b"), Some(&Value::int(2)));
    }

    #[test]
    fn test_rfc7396_nested() {
        let base = map(&[
            ("a", map(&[("x", Value::int(1)), ("y", Value::int(2))])),
        ]);
        let patch = map(&[
            ("a", map(&[("y", Value::int(99)), ("z", Value::int(3))])),
        ]);
        let result = json_merge_patch(&base, &patch);
        let a = result.get("a").unwrap();
        assert_eq!(a.get("x"), Some(&Value::int(1)));
        assert_eq!(a.get("y"), Some(&Value::int(99)));
        assert_eq!(a.get("z"), Some(&Value::int(3)));
    }

    #[test]
    fn test_rfc7396_preserves_unmodified() {
        let base = map(&[
            ("a", Value::int(1)),
            ("b", Value::string("hello")),
            ("c", Value::bool(true)),
        ]);
        let patch = map(&[("d", Value::int(4))]);
        let result = json_merge_patch(&base, &patch);
        assert_eq!(result.get("a"), Some(&Value::int(1)));
        assert_eq!(result.get("b"), Some(&Value::string("hello")));
        assert_eq!(result.get("c"), Some(&Value::bool(true)));
        assert_eq!(result.get("d"), Some(&Value::int(4)));
    }

    #[test]
    fn test_rfc7396_nested_null() {
        let base = map(&[
            ("a", map(&[("x", Value::int(1)), ("y", Value::int(2))])),
        ]);
        let patch = map(&[("a", map(&[("x", Value::Null)]))]);
        let result = json_merge_patch(&base, &patch);
        let a = result.get("a").unwrap();
        assert_eq!(a.get("x"), None);
        assert_eq!(a.get("y"), Some(&Value::int(2)));
    }

    // ── RFC 6902 JSON Patch (3 tests) ─────────────────────────────

    fn patch_op(op: &str, path: &str, value: Option<Value>) -> Value {
        let mut pairs: Vec<(&str, Value)> = vec![
            ("op", Value::string(op)),
            ("path", Value::string(path)),
        ];
        if let Some(v) = value {
            pairs.push(("value", v));
        }
        map(&pairs)
    }

    #[test]
    fn test_rfc6902_add_operation() {
        let target = map(&[("a", Value::int(1))]);
        let ops = Value::array(vec![patch_op("add", "/b", Some(Value::int(2)))]);
        let result = json_patch(&target, &ops).unwrap();
        assert_eq!(result.get("a"), Some(&Value::int(1)));
        assert_eq!(result.get("b"), Some(&Value::int(2)));
    }

    #[test]
    fn test_rfc6902_remove_operation() {
        let target = map(&[("a", Value::int(1)), ("b", Value::int(2))]);
        let ops = Value::array(vec![patch_op("remove", "/b", None)]);
        let result = json_patch(&target, &ops).unwrap();
        assert_eq!(result.get("a"), Some(&Value::int(1)));
        assert_eq!(result.get("b"), None);
    }

    #[test]
    fn test_rfc6902_replace_operation() {
        let target = map(&[("a", Value::int(1))]);
        let ops = Value::array(vec![patch_op("replace", "/a", Some(Value::int(99)))]);
        let result = json_patch(&target, &ops).unwrap();
        assert_eq!(result.get("a"), Some(&Value::int(99)));
    }

    // ── Strategic Merge (4 tests) ─────────────────────────────────

    #[test]
    fn test_strategic_merge_array_by_key() {
        let base = map(&[(
            "containers",
            Value::array(vec![
                map(&[("name", Value::string("web")), ("image", Value::string("nginx:1.0"))]),
                map(&[("name", Value::string("sidecar")), ("image", Value::string("envoy:1.0"))]),
            ]),
        )]);
        let over = map(&[(
            "containers",
            Value::array(vec![
                map(&[("name", Value::string("web")), ("image", Value::string("nginx:2.0"))]),
            ]),
        )]);
        let merge_keys = vec![("containers".to_string(), "name".to_string())];
        let result = strategic_merge(&base, &over, &merge_keys);
        let containers = result.get("containers").unwrap().as_array().unwrap();
        assert_eq!(containers.len(), 2);
        // "web" container should be updated
        assert_eq!(containers[0].get("image"), Some(&Value::string("nginx:2.0")));
        // "sidecar" should be unchanged
        assert_eq!(containers[1].get("image"), Some(&Value::string("envoy:1.0")));
    }

    #[test]
    fn test_strategic_merge_without_merge_key_replaces_array() {
        let base = map(&[(
            "args",
            Value::array(vec![Value::string("--port=8080")]),
        )]);
        let over = map(&[(
            "args",
            Value::array(vec![Value::string("--port=9090"), Value::string("--debug")]),
        )]);
        let merge_keys: Vec<(String, String)> = vec![]; // no merge key for "args"
        let result = strategic_merge(&base, &over, &merge_keys);
        let args = result.get("args").unwrap().as_array().unwrap();
        // Array is replaced entirely
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], Value::string("--port=9090"));
        assert_eq!(args[1], Value::string("--debug"));
    }

    #[test]
    fn test_strategic_merge_maps_recurse() {
        let base = map(&[
            ("spec", map(&[
                ("replicas", Value::int(3)),
                ("selector", map(&[("app", Value::string("web"))])),
            ])),
        ]);
        let over = map(&[
            ("spec", map(&[
                ("replicas", Value::int(5)),
            ])),
        ]);
        let merge_keys: Vec<(String, String)> = vec![];
        let result = strategic_merge(&base, &over, &merge_keys);
        let spec = result.get("spec").unwrap();
        assert_eq!(spec.get("replicas"), Some(&Value::int(5)));
        // selector preserved from base
        assert_eq!(
            spec.get("selector"),
            Some(&map(&[("app", Value::string("web"))]))
        );
    }

    #[test]
    fn test_strategic_merge_via_strategy_enum() {
        let base = map(&[("a", Value::int(1))]);
        let over = map(&[("b", Value::int(2))]);
        let result = merge(
            &base,
            &over,
            Strategy::Strategic {
                merge_keys: vec![],
            },
        );
        assert_eq!(result.get("a"), Some(&Value::int(1)));
        assert_eq!(result.get("b"), Some(&Value::int(2)));
    }

    // ── Three-Way Merge (4 tests) ─────────────────────────────────

    #[test]
    fn test_three_way_no_conflict() {
        let base = map(&[("a", Value::int(1)), ("b", Value::int(2))]);
        let left = map(&[("a", Value::int(10)), ("b", Value::int(2))]);
        let right = map(&[("a", Value::int(1)), ("b", Value::int(20))]);
        let result = three_way_merge(&base, &left, &right).unwrap();
        assert_eq!(result.get("a"), Some(&Value::int(10)));
        assert_eq!(result.get("b"), Some(&Value::int(20)));
    }

    #[test]
    fn test_three_way_conflict_detection() {
        let base = map(&[("a", Value::int(1))]);
        let left = map(&[("a", Value::int(10))]);
        let right = map(&[("a", Value::int(20))]);
        let result = three_way_merge(&base, &left, &right);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("merge conflict"));
        assert!(err.to_string().contains("a"));
    }

    #[test]
    fn test_three_way_independent_changes_merge() {
        let base = map(&[("x", Value::int(1))]);
        let left = map(&[("x", Value::int(1)), ("y", Value::int(2))]);
        let right = map(&[("x", Value::int(1)), ("z", Value::int(3))]);
        let result = three_way_merge(&base, &left, &right).unwrap();
        assert_eq!(result.get("x"), Some(&Value::int(1)));
        assert_eq!(result.get("y"), Some(&Value::int(2)));
        assert_eq!(result.get("z"), Some(&Value::int(3)));
    }

    #[test]
    fn test_three_way_identical_changes() {
        let base = map(&[("a", Value::int(1))]);
        let left = map(&[("a", Value::int(42))]);
        let right = map(&[("a", Value::int(42))]);
        // Both sides made the same change — no conflict
        let result = three_way_merge(&base, &left, &right).unwrap();
        assert_eq!(result.get("a"), Some(&Value::int(42)));
    }
}
