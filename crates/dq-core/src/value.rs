//! # Universal Value Type
//!
//! The single data structure that all dq operations work on.
//! Every format deserializes into [`Value`], every query/merge/diff
//! operates on [`Value`], every format serializes from [`Value`].
//!
//! ## Design (informed by MiniJinja, Nushell, Polars)
//!
//! - **Size-constrained**: The enum is kept small via `Arc` indirection
//!   for heap-allocated variants. Clone is O(1) for all compound types.
//!
//! - **Superset of all formats**: Preserves integer/float distinction (TOML,
//!   MessagePack), datetimes (TOML), ordered maps (YAML, HCL), HCL blocks
//!   with labels, binary data (MessagePack, CBOR, Protobuf).
//!
//! - **Composable operations**: All operations follow the jq/Polars
//!   pattern: `Fn(Value) -> impl Iterator<Item = Value>`.
//!
//! - **From/Into conversions**: Bidirectional conversion with serde_json::Value,
//!   toml::Value, and hcl types. Cross-format conversion is inherently lossy
//!   (TOML datetimes → JSON strings) but within-format fidelity is preserved.

use indexmap::IndexMap;
use std::fmt;
use std::sync::Arc;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Core Types
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// The universal value type for all dq operations.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(Arc<str>),
    Bytes(Arc<[u8]>),
    /// ISO 8601 datetime (preserves TOML's native datetime type).
    Datetime(Arc<str>),
    Array(Arc<Vec<Value>>),
    /// Ordered key→value map. Preserves insertion order (critical for YAML/HCL).
    Map(Arc<IndexMap<Arc<str>, Value>>),
    /// HCL labeled block. First-class representation of Terraform resources,
    /// Terragrunt dependency blocks, etc.
    Block(Block),
}

/// An HCL-style labeled block.
///
/// ```hcl
/// resource "aws_instance" "web" {
///   ami = "ami-12345"
/// }
/// ```
/// → `Block { block_type: "resource", labels: ["aws_instance", "web"], body: {ami: "ami-12345"} }`
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub block_type: Arc<str>,
    pub labels: Vec<Arc<str>>,
    pub body: Arc<IndexMap<Arc<str>, Value>>,
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Construction
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl Value {
    pub fn null() -> Self { Value::Null }
    pub fn bool(v: bool) -> Self { Value::Bool(v) }
    pub fn int(v: i64) -> Self { Value::Int(v) }
    pub fn float(v: f64) -> Self { Value::Float(v) }
    pub fn string(s: impl AsRef<str>) -> Self { Value::String(Arc::from(s.as_ref())) }
    pub fn bytes(b: impl Into<Arc<[u8]>>) -> Self { Value::Bytes(b.into()) }
    pub fn datetime(s: impl AsRef<str>) -> Self { Value::Datetime(Arc::from(s.as_ref())) }

    pub fn array(v: impl Into<Vec<Value>>) -> Self {
        Value::Array(Arc::new(v.into()))
    }

    pub fn map(m: impl Into<IndexMap<Arc<str>, Value>>) -> Self {
        Value::Map(Arc::new(m.into()))
    }

    /// Construct an empty map.
    pub fn empty_map() -> Self {
        Value::Map(Arc::new(IndexMap::new()))
    }

    /// Build a map from key-value pairs.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (impl AsRef<str>, Value)>) -> Self {
        let map: IndexMap<Arc<str>, Value> = pairs.into_iter()
            .map(|(k, v)| (Arc::from(k.as_ref()), v))
            .collect();
        Value::Map(Arc::new(map))
    }

    pub fn block(
        block_type: impl AsRef<str>,
        labels: impl IntoIterator<Item = impl AsRef<str>>,
        body: IndexMap<Arc<str>, Value>,
    ) -> Self {
        Value::Block(Block {
            block_type: Arc::from(block_type.as_ref()),
            labels: labels.into_iter().map(|l| Arc::from(l.as_ref())).collect(),
            body: Arc::new(body),
        })
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Type Predicates & Type Name
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl Value {
    pub fn is_null(&self) -> bool { matches!(self, Value::Null) }
    pub fn is_bool(&self) -> bool { matches!(self, Value::Bool(_)) }
    pub fn is_int(&self) -> bool { matches!(self, Value::Int(_)) }
    pub fn is_float(&self) -> bool { matches!(self, Value::Float(_)) }
    pub fn is_number(&self) -> bool { matches!(self, Value::Int(_) | Value::Float(_)) }
    pub fn is_string(&self) -> bool { matches!(self, Value::String(_)) }
    pub fn is_array(&self) -> bool { matches!(self, Value::Array(_)) }
    pub fn is_map(&self) -> bool { matches!(self, Value::Map(_)) }
    pub fn is_block(&self) -> bool { matches!(self, Value::Block(_)) }
    pub fn is_bytes(&self) -> bool { matches!(self, Value::Bytes(_)) }
    pub fn is_datetime(&self) -> bool { matches!(self, Value::Datetime(_)) }

    /// jq-compatible type name.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Int(_) | Value::Float(_) => "number",
            Value::String(_) => "string",
            Value::Bytes(_) => "bytes",
            Value::Datetime(_) => "datetime",
            Value::Array(_) => "array",
            Value::Map(_) => "object",
            Value::Block(_) => "block",
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Accessors (safe extraction)
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl Value {
    pub fn as_bool(&self) -> Option<bool> {
        match self { Value::Bool(b) => Some(*b), _ => None }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self { Value::Int(n) => Some(*n), _ => None }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(n) => Some(*n as f64),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) | Value::Datetime(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&[Value]> {
        match self { Value::Array(a) => Some(a), _ => None }
    }
    pub fn as_map(&self) -> Option<&IndexMap<Arc<str>, Value>> {
        match self { Value::Map(m) => Some(m), _ => None }
    }
    pub fn as_block(&self) -> Option<&Block> {
        match self { Value::Block(b) => Some(b), _ => None }
    }

    /// Index by key (works on Map and Block body).
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(m) => m.get(key),
            Value::Block(b) => b.body.get(key),
            _ => None,
        }
    }

    /// Index by position (works on Array).
    pub fn get_index(&self, idx: usize) -> Option<&Value> {
        match self { Value::Array(a) => a.get(idx), _ => None }
    }

    /// Dot-path navigation: `"spec.template.metadata.name"`.
    /// Handles both map keys and array indices.
    pub fn select(&self, path: &str) -> Option<&Value> {
        if path.is_empty() { return Some(self); }
        let mut current = self;
        for segment in path.split('.') {
            current = current.get(segment)
                .or_else(|| segment.parse::<usize>().ok().and_then(|i| current.get_index(i)))?;
        }
        Some(current)
    }

    /// Number of elements/keys/characters.
    pub fn len(&self) -> usize {
        match self {
            Value::Array(a) => a.len(),
            Value::Map(m) => m.len(),
            Value::Block(b) => b.body.len(),
            Value::String(s) => s.chars().count(),
            Value::Bytes(b) => b.len(),
            _ => 0,
        }
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }

    /// Keys of a map or block body (empty vec for other types).
    pub fn keys(&self) -> Vec<&str> {
        match self {
            Value::Map(m) => m.keys().map(AsRef::as_ref).collect(),
            Value::Block(b) => b.body.keys().map(AsRef::as_ref).collect(),
            _ => vec![],
        }
    }

    /// Values of a map, block body, or array elements.
    pub fn values(&self) -> Vec<&Value> {
        match self {
            Value::Map(m) => m.values().collect(),
            Value::Block(b) => b.body.values().collect(),
            Value::Array(a) => a.iter().collect(),
            _ => vec![],
        }
    }

    /// Key-value entries (Map, Block body).
    pub fn entries(&self) -> Vec<(&str, &Value)> {
        match self {
            Value::Map(m) => m.iter().map(|(k, v)| (k.as_ref(), v)).collect(),
            Value::Block(b) => b.body.iter().map(|(k, v)| (k.as_ref(), v)).collect(),
            _ => vec![],
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Recursive Operations
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl Value {
    /// Recursively flatten nested arrays (jq `flatten`).
    pub fn flatten(&self) -> Value {
        match self {
            Value::Array(a) => {
                let mut result = Vec::new();
                flatten_into(a, &mut result);
                Value::array(result)
            }
            other => other.clone(),
        }
    }

    /// Collect all blocks of a given type, recursively.
    pub fn find_blocks(&self, block_type: &str) -> Vec<&Block> {
        let mut result = Vec::new();
        find_blocks_recursive(self, block_type, &mut result);
        result
    }

    /// Walk the entire tree, calling `f` on each value (pre-order DFS).
    pub fn walk<F: FnMut(&Value)>(&self, f: &mut F) {
        f(self);
        match self {
            Value::Array(a) => a.iter().for_each(|v| v.walk(f)),
            Value::Map(m) => m.values().for_each(|v| v.walk(f)),
            Value::Block(b) => b.body.values().for_each(|v| v.walk(f)),
            _ => {}
        }
    }

    /// Transform every leaf value, preserving structure (map over leaves).
    pub fn map_leaves<F: Fn(&Value) -> Value>(&self, f: &F) -> Value {
        match self {
            Value::Array(a) => Value::array(a.iter().map(|v| v.map_leaves(f)).collect::<Vec<_>>()),
            Value::Map(m) => {
                let mapped: IndexMap<Arc<str>, Value> = m.iter()
                    .map(|(k, v)| (k.clone(), v.map_leaves(f)))
                    .collect();
                Value::Map(Arc::new(mapped))
            }
            Value::Block(b) => {
                let mapped: IndexMap<Arc<str>, Value> = b.body.iter()
                    .map(|(k, v)| (k.clone(), v.map_leaves(f)))
                    .collect();
                Value::Block(Block {
                    block_type: b.block_type.clone(),
                    labels: b.labels.clone(),
                    body: Arc::new(mapped),
                })
            }
            leaf => f(leaf),
        }
    }
}

fn flatten_into(arr: &[Value], out: &mut Vec<Value>) {
    for item in arr {
        match item {
            Value::Array(inner) => flatten_into(inner, out),
            other => out.push(other.clone()),
        }
    }
}

fn find_blocks_recursive<'a>(val: &'a Value, block_type: &str, out: &mut Vec<&'a Block>) {
    match val {
        Value::Block(b) => {
            if b.block_type.as_ref() == block_type { out.push(b); }
            for v in b.body.values() { find_blocks_recursive(v, block_type, out); }
        }
        Value::Map(m) => { for v in m.values() { find_blocks_recursive(v, block_type, out); } }
        Value::Array(a) => { for v in a.iter() { find_blocks_recursive(v, block_type, out); } }
        _ => {}
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// From/Into Conversions
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl From<bool> for Value { fn from(v: bool) -> Self { Value::Bool(v) } }
impl From<i64> for Value { fn from(v: i64) -> Self { Value::Int(v) } }
impl From<i32> for Value { fn from(v: i32) -> Self { Value::Int(v as i64) } }
impl From<f64> for Value { fn from(v: f64) -> Self { Value::Float(v) } }
impl From<&str> for Value { fn from(v: &str) -> Self { Value::String(Arc::from(v)) } }
impl From<String> for Value { fn from(v: String) -> Self { Value::String(Arc::from(v.as_str())) } }
impl From<Vec<Value>> for Value { fn from(v: Vec<Value>) -> Self { Value::Array(Arc::new(v)) } }
impl From<IndexMap<Arc<str>, Value>> for Value { fn from(m: IndexMap<Arc<str>, Value>) -> Self { Value::Map(Arc::new(m)) } }

/// Lossless conversion from serde_json::Value.
impl From<serde_json::Value> for Value {
    fn from(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() { Value::Int(i) }
                else if let Some(f) = n.as_f64() { Value::Float(f) }
                else { Value::string(n.to_string()) }
            }
            serde_json::Value::String(s) => Value::String(Arc::from(s.as_str())),
            serde_json::Value::Array(a) => Value::Array(Arc::new(
                a.into_iter().map(Value::from).collect()
            )),
            serde_json::Value::Object(m) => Value::Map(Arc::new(
                m.into_iter().map(|(k, v)| (Arc::from(k.as_str()), Value::from(v))).collect()
            )),
        }
    }
}

/// Lossy conversion to serde_json::Value.
/// Blocks become objects with `__block_type` and `__labels` metadata keys.
/// Datetimes become strings. Bytes become base64 strings.
impl From<&Value> for serde_json::Value {
    fn from(v: &Value) -> Self {
        match v {
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => serde_json::Value::Bool(*b),
            Value::Int(n) => serde_json::Value::Number((*n).into()),
            Value::Float(f) => serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Value::String(s) | Value::Datetime(s) => serde_json::Value::String(s.to_string()),
            Value::Bytes(b) => {
                use base64::Engine;
                serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(b.as_ref()))
            }
            Value::Array(a) => serde_json::Value::Array(
                a.iter().map(serde_json::Value::from).collect()
            ),
            Value::Map(m) => serde_json::Value::Object(
                m.iter().map(|(k, v)| (k.to_string(), serde_json::Value::from(v))).collect()
            ),
            Value::Block(b) => {
                let mut obj = serde_json::Map::new();
                obj.insert("__block_type".into(), serde_json::Value::String(b.block_type.to_string()));
                if !b.labels.is_empty() {
                    obj.insert("__labels".into(), serde_json::Value::Array(
                        b.labels.iter().map(|l| serde_json::Value::String(l.to_string())).collect()
                    ));
                }
                for (k, v) in b.body.iter() {
                    obj.insert(k.to_string(), serde_json::Value::from(v));
                }
                serde_json::Value::Object(obj)
            }
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Display
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(n) => {
                if n.fract() == 0.0 { write!(f, "{n:.1}") }
                else { write!(f, "{n}") }
            }
            Value::String(s) => write!(f, "{s}"),
            Value::Datetime(s) => write!(f, "{s}"),
            Value::Bytes(b) => write!(f, "<{} bytes>", b.len()),
            Value::Array(_) | Value::Map(_) => {
                let json = serde_json::Value::from(self);
                write!(f, "{}", serde_json::to_string_pretty(&json).unwrap_or_default())
            }
            Value::Block(b) => {
                write!(f, "{}", b.block_type)?;
                for l in &b.labels { write!(f, " \"{l}\"")?; }
                write!(f, " {{ ... }}")
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

    #[test]
    fn test_construction_and_type_predicates() {
        assert!(Value::null().is_null());
        assert!(Value::bool(true).is_bool());
        assert!(Value::int(42).is_int());
        assert!(Value::int(42).is_number());
        assert!(Value::float(3.14).is_float());
        assert!(Value::float(3.14).is_number());
        assert!(Value::string("hello").is_string());
        assert!(Value::bytes(vec![1, 2, 3]).is_bytes());
        assert!(Value::datetime("2024-01-01T00:00:00Z").is_datetime());
        assert!(Value::array(vec![]).is_array());
        assert!(Value::empty_map().is_map());
    }

    #[test]
    fn test_type_names() {
        assert_eq!(Value::null().type_name(), "null");
        assert_eq!(Value::bool(false).type_name(), "boolean");
        assert_eq!(Value::int(0).type_name(), "number");
        assert_eq!(Value::float(0.0).type_name(), "number");
        assert_eq!(Value::string("").type_name(), "string");
        assert_eq!(Value::array(vec![]).type_name(), "array");
        assert_eq!(Value::empty_map().type_name(), "object");
    }

    #[test]
    fn test_accessors() {
        assert_eq!(Value::bool(true).as_bool(), Some(true));
        assert_eq!(Value::int(42).as_i64(), Some(42));
        assert_eq!(Value::int(42).as_f64(), Some(42.0));
        assert_eq!(Value::float(3.14).as_f64(), Some(3.14));
        assert_eq!(Value::string("hi").as_str(), Some("hi"));
        assert_eq!(Value::datetime("2024").as_str(), Some("2024"));
    }

    #[test]
    fn test_map_operations() {
        let m = Value::from_pairs([("a", Value::int(1)), ("b", Value::int(2))]);
        assert_eq!(m.get("a"), Some(&Value::int(1)));
        assert_eq!(m.get("c"), None);
        assert_eq!(m.len(), 2);
        assert_eq!(m.keys(), vec!["a", "b"]);
    }

    #[test]
    fn test_array_operations() {
        let a = Value::array(vec![Value::int(10), Value::int(20)]);
        assert_eq!(a.get_index(0), Some(&Value::int(10)));
        assert_eq!(a.get_index(2), None);
        assert_eq!(a.len(), 2);
    }

    #[test]
    fn test_select_dot_path() {
        let v = Value::from_pairs([
            ("spec", Value::from_pairs([
                ("template", Value::from_pairs([
                    ("name", Value::string("web")),
                ])),
            ])),
        ]);
        assert_eq!(v.select("spec.template.name"), Some(&Value::string("web")));
        assert_eq!(v.select("spec.missing"), None);
        assert_eq!(v.select(""), Some(&v));
    }

    #[test]
    fn test_select_with_array_index() {
        let v = Value::from_pairs([
            ("items", Value::array(vec![
                Value::string("a"),
                Value::string("b"),
            ])),
        ]);
        assert_eq!(v.select("items.0"), Some(&Value::string("a")));
        assert_eq!(v.select("items.1"), Some(&Value::string("b")));
    }

    #[test]
    fn test_flatten() {
        let nested = Value::array(vec![
            Value::int(1),
            Value::array(vec![Value::int(2), Value::array(vec![Value::int(3)])]),
            Value::int(4),
        ]);
        let flat = nested.flatten();
        assert_eq!(flat, Value::array(vec![
            Value::int(1), Value::int(2), Value::int(3), Value::int(4),
        ]));
    }

    #[test]
    fn test_flatten_non_array() {
        assert_eq!(Value::int(42).flatten(), Value::int(42));
    }

    #[test]
    fn test_find_blocks() {
        let v = Value::from_pairs([
            ("dependency", Value::block("dependency", ["vpc"], {
                let mut m = IndexMap::new();
                m.insert(Arc::from("config_path"), Value::string("../vpc"));
                m
            })),
            ("other", Value::from_pairs([
                ("dependency", Value::block("dependency", ["rds"], {
                    let mut m = IndexMap::new();
                    m.insert(Arc::from("config_path"), Value::string("../rds"));
                    m
                })),
            ])),
        ]);
        let blocks = v.find_blocks("dependency");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].labels[0].as_ref(), "vpc");
        assert_eq!(blocks[1].labels[0].as_ref(), "rds");
    }

    #[test]
    fn test_walk() {
        let v = Value::from_pairs([("a", Value::array(vec![Value::int(1), Value::int(2)]))]);
        let mut count = 0;
        v.walk(&mut |_| count += 1);
        // root map + "a" array + int(1) + int(2) = 4
        assert_eq!(count, 4);
    }

    #[test]
    fn test_map_leaves() {
        let v = Value::from_pairs([
            ("x", Value::int(1)),
            ("y", Value::from_pairs([("z", Value::int(2))])),
        ]);
        let doubled = v.map_leaves(&|leaf| {
            match leaf {
                Value::Int(n) => Value::int(n * 2),
                other => other.clone(),
            }
        });
        assert_eq!(doubled.select("x"), Some(&Value::int(2)));
        assert_eq!(doubled.select("y.z"), Some(&Value::int(4)));
    }

    #[test]
    fn test_from_pairs() {
        let v = Value::from_pairs([("key", Value::string("val"))]);
        assert_eq!(v.get("key"), Some(&Value::string("val")));
    }

    #[test]
    fn test_block_access() {
        let b = Value::block("resource", ["aws_instance", "web"], {
            let mut m = IndexMap::new();
            m.insert(Arc::from("ami"), Value::string("ami-12345"));
            m
        });
        assert!(b.is_block());
        assert_eq!(b.get("ami"), Some(&Value::string("ami-12345")));
        assert_eq!(b.keys(), vec!["ami"]);
        let block = b.as_block().unwrap();
        assert_eq!(block.block_type.as_ref(), "resource");
        assert_eq!(block.labels.len(), 2);
    }

    #[test]
    fn test_json_roundtrip() {
        let original = Value::from_pairs([
            ("name", Value::string("test")),
            ("count", Value::int(42)),
            ("enabled", Value::bool(true)),
            ("tags", Value::array(vec![Value::string("a"), Value::string("b")])),
        ]);
        let json = serde_json::Value::from(&original);
        let back = Value::from(json);
        assert_eq!(original, back);
    }

    #[test]
    fn test_json_block_roundtrip() {
        let original = Value::block("dependency", ["vpc"], {
            let mut m = IndexMap::new();
            m.insert(Arc::from("config_path"), Value::string("../vpc"));
            m
        });
        let json = serde_json::Value::from(&original);
        // Block becomes object with __block_type metadata
        assert_eq!(json["__block_type"], "dependency");
        assert_eq!(json["__labels"][0], "vpc");
        assert_eq!(json["config_path"], "../vpc");
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Value::null()), "null");
        assert_eq!(format!("{}", Value::bool(true)), "true");
        assert_eq!(format!("{}", Value::int(42)), "42");
        assert_eq!(format!("{}", Value::string("hello")), "hello");
    }

    #[test]
    fn test_len_and_is_empty() {
        assert_eq!(Value::null().len(), 0);
        assert!(Value::null().is_empty());
        assert_eq!(Value::string("hello").len(), 5);
        assert!(!Value::string("hello").is_empty());
        assert_eq!(Value::array(vec![Value::int(1)]).len(), 1);
    }

    #[test]
    fn test_entries() {
        let m = Value::from_pairs([("x", Value::int(1)), ("y", Value::int(2))]);
        let entries = m.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], ("x", &Value::int(1)));
    }
}
