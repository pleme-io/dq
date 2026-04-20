//! # dq-formats — Format Detection, Parsing, and Serialization
//!
//! Bridges wire formats (JSON, YAML, TOML, HCL, CSV, MessagePack)
//! to/from [`dq_core::Value`]. Each format module implements [`Format`].

pub mod detect;
pub mod json;
pub mod yaml;
pub mod toml_fmt;
pub mod hcl;
pub mod csv_fmt;
pub mod msgpack;

use dq_core::{Error, Value};

/// Supported wire formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatKind {
    Json,
    Yaml,
    Toml,
    Hcl,
    Csv,
    MsgPack,
}

impl FormatKind {
    /// Canonical file extensions for this format.
    pub fn extensions(&self) -> &[&str] {
        match self {
            FormatKind::Json => &["json", "jsonl", "geojson"],
            FormatKind::Yaml => &["yaml", "yml"],
            FormatKind::Toml => &["toml"],
            FormatKind::Hcl => &["hcl", "tf", "tfvars"],
            FormatKind::Csv => &["csv", "tsv"],
            FormatKind::MsgPack => &["msgpack", "mp"],
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            FormatKind::Json => "json",
            FormatKind::Yaml => "yaml",
            FormatKind::Toml => "toml",
            FormatKind::Hcl => "hcl",
            FormatKind::Csv => "csv",
            FormatKind::MsgPack => "msgpack",
        }
    }

    /// Parse from format name string.
    pub fn from_name(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "json" | "jsonl" => Some(FormatKind::Json),
            "yaml" | "yml" => Some(FormatKind::Yaml),
            "toml" => Some(FormatKind::Toml),
            "hcl" | "tf" | "tfvars" | "terragrunt" => Some(FormatKind::Hcl),
            "csv" | "tsv" => Some(FormatKind::Csv),
            "msgpack" | "mp" | "messagepack" => Some(FormatKind::MsgPack),
            _ => None,
        }
    }
}

/// Trait for format implementations.
pub trait Format {
    /// Parse bytes into a Value.
    fn parse(&self, input: &[u8]) -> Result<Value, Error>;

    /// Serialize a Value to bytes.
    fn serialize(&self, value: &Value) -> Result<Vec<u8>, Error>;
}

/// Parse input using the specified format.
pub fn parse(format: FormatKind, input: &[u8]) -> Result<Value, Error> {
    match format {
        FormatKind::Json => json::JsonFormat.parse(input),
        FormatKind::Yaml => yaml::YamlFormat.parse(input),
        FormatKind::Toml => toml_fmt::TomlFormat.parse(input),
        FormatKind::Hcl => hcl::HclFormat.parse(input),
        FormatKind::Csv => csv_fmt::CsvFormat.parse(input),
        FormatKind::MsgPack => msgpack::MsgPackFormat.parse(input),
    }
}

/// Serialize a Value to the specified format.
pub fn serialize(format: FormatKind, value: &Value) -> Result<Vec<u8>, Error> {
    match format {
        FormatKind::Json => json::JsonFormat.serialize(value),
        FormatKind::Yaml => yaml::YamlFormat.serialize(value),
        FormatKind::Toml => toml_fmt::TomlFormat.serialize(value),
        FormatKind::Hcl => hcl::HclFormat.serialize(value),
        FormatKind::Csv => csv_fmt::CsvFormat.serialize(value),
        FormatKind::MsgPack => msgpack::MsgPackFormat.serialize(value),
    }
}

/// Auto-detect format from file extension and parse.
pub fn parse_auto(path: &str, input: &[u8]) -> Result<Value, Error> {
    let format = detect::detect_from_path(path)
        .or_else(|| detect::detect_from_content(input))
        .ok_or_else(|| Error::Format(format!("cannot detect format for: {path}")))?;
    parse(format, input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq_core::Value;

    // ── Detection tests ──────────────────────────────────────────────

    #[test]
    fn detect_json_by_extension() {
        assert_eq!(detect::detect_from_path("data.json"), Some(FormatKind::Json));
        assert_eq!(detect::detect_from_path("events.jsonl"), Some(FormatKind::Json));
    }

    #[test]
    fn detect_yaml_by_extension() {
        assert_eq!(detect::detect_from_path("config.yaml"), Some(FormatKind::Yaml));
        assert_eq!(detect::detect_from_path("data.yml"), Some(FormatKind::Yaml));
    }

    #[test]
    fn detect_toml_by_extension() {
        assert_eq!(detect::detect_from_path("Cargo.toml"), Some(FormatKind::Toml));
    }

    #[test]
    fn detect_hcl_by_extension() {
        assert_eq!(detect::detect_from_path("main.tf"), Some(FormatKind::Hcl));
        assert_eq!(detect::detect_from_path("terragrunt.hcl"), Some(FormatKind::Hcl));
    }

    #[test]
    fn detect_json_by_content() {
        let content = br#"{"key": "value"}"#;
        assert_eq!(detect::detect_from_content(content), Some(FormatKind::Json));
    }

    #[test]
    fn detect_json_array_by_content() {
        let content = br#"[1, 2, 3]"#;
        assert_eq!(detect::detect_from_content(content), Some(FormatKind::Json));
    }

    #[test]
    fn detect_toml_by_content() {
        let content = b"[package]\nname = \"dq\"\nversion = \"0.1.0\"";
        assert_eq!(detect::detect_from_content(content), Some(FormatKind::Toml));
    }

    #[test]
    fn detect_hcl_by_content() {
        let content = b"resource \"aws_instance\" \"web\" {\n  ami = \"ami-12345\"\n}";
        assert_eq!(detect::detect_from_content(content), Some(FormatKind::Hcl));
    }

    #[test]
    fn detect_yaml_by_content() {
        let content = b"---\nname: test\nitems:\n  - one\n  - two";
        assert_eq!(detect::detect_from_content(content), Some(FormatKind::Yaml));
    }

    #[test]
    fn detect_unknown_extension() {
        assert_eq!(detect::detect_from_path("data.xyz"), None);
    }

    // ── JSON roundtrip ───────────────────────────────────────────────

    #[test]
    fn json_roundtrip_object() {
        let input = br#"{"name":"test","count":42,"enabled":true}"#;
        let val = parse(FormatKind::Json, input).unwrap();
        assert_eq!(val.get("name"), Some(&Value::string("test")));
        assert_eq!(val.get("count"), Some(&Value::int(42)));
        let output = serialize(FormatKind::Json, &val).unwrap();
        let back = parse(FormatKind::Json, &output).unwrap();
        assert_eq!(val, back);
    }

    #[test]
    fn json_roundtrip_array() {
        let input = br#"[1, "hello", null, true]"#;
        let val = parse(FormatKind::Json, input).unwrap();
        assert!(val.is_array());
        assert_eq!(val.len(), 4);
    }

    #[test]
    fn json_roundtrip_nested() {
        let input = br#"{"a":{"b":{"c":1}}}"#;
        let val = parse(FormatKind::Json, input).unwrap();
        assert_eq!(val.select("a.b.c"), Some(&Value::int(1)));
    }

    // ── YAML tests ───────────────────────────────────────────────────

    #[test]
    fn yaml_parse_basic() {
        let input = b"name: test\ncount: 42\nenabled: true";
        let val = parse(FormatKind::Yaml, input).unwrap();
        assert_eq!(val.get("name"), Some(&Value::string("test")));
        assert_eq!(val.get("count"), Some(&Value::int(42)));
    }

    #[test]
    fn yaml_parse_nested() {
        let input = b"spec:\n  template:\n    name: web";
        let val = parse(FormatKind::Yaml, input).unwrap();
        assert_eq!(val.select("spec.template.name"), Some(&Value::string("web")));
    }

    #[test]
    fn yaml_roundtrip() {
        let val = Value::from_pairs([
            ("items", Value::array(vec![Value::int(1), Value::int(2)])),
        ]);
        let bytes = serialize(FormatKind::Yaml, &val).unwrap();
        let back = parse(FormatKind::Yaml, &bytes).unwrap();
        assert_eq!(val, back);
    }

    // ── TOML tests ───────────────────────────────────────────────────

    #[test]
    fn toml_datetime_preservation() {
        let input = b"created = 2024-01-15T10:30:00Z";
        let val = parse(FormatKind::Toml, input).unwrap();
        let dt = val.get("created").unwrap();
        assert!(dt.is_datetime());
        assert_eq!(dt.as_str(), Some("2024-01-15T10:30:00Z"));
    }

    #[test]
    fn toml_roundtrip() {
        let input = b"[package]\nname = \"dq\"\nversion = \"0.1.0\"";
        let val = parse(FormatKind::Toml, input).unwrap();
        let bytes = serialize(FormatKind::Toml, &val).unwrap();
        let back = parse(FormatKind::Toml, &bytes).unwrap();
        assert_eq!(val, back);
    }

    #[test]
    fn toml_null_omission() {
        let val = Value::from_pairs([
            ("keep", Value::string("yes")),
            ("drop", Value::Null),
        ]);
        let bytes = serialize(FormatKind::Toml, &val).unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("keep"));
        assert!(!text.contains("drop"));
    }

    #[test]
    fn toml_nested_table() {
        let input = b"[server]\nhost = \"localhost\"\nport = 8080";
        let val = parse(FormatKind::Toml, input).unwrap();
        assert_eq!(val.select("server.host"), Some(&Value::string("localhost")));
        assert_eq!(val.select("server.port"), Some(&Value::int(8080)));
    }

    // ── HCL tests ────────────────────────────────────────────────────

    #[test]
    fn hcl_parse_attribute() {
        let input = b"name = \"test\"\ncount = 42";
        let val = parse(FormatKind::Hcl, input).unwrap();
        assert_eq!(val.get("name"), Some(&Value::string("test")));
        assert_eq!(val.get("count"), Some(&Value::int(42)));
    }

    #[test]
    fn hcl_parse_block() {
        let input = b"resource \"aws_instance\" \"web\" {\n  ami = \"ami-12345\"\n}";
        let val = parse(FormatKind::Hcl, input).unwrap();
        let blocks = val.find_blocks("resource");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].labels[0].as_ref(), "aws_instance");
        assert_eq!(blocks[0].labels[1].as_ref(), "web");
        assert_eq!(blocks[0].body.get("ami"), Some(&Value::string("ami-12345")));
    }

    #[test]
    fn hcl_parse_multiple_blocks() {
        let input = b"dependency \"vpc\" {\n  config_path = \"../vpc\"\n}\ndependency \"rds\" {\n  config_path = \"../rds\"\n}";
        let val = parse(FormatKind::Hcl, input).unwrap();
        let blocks = val.find_blocks("dependency");
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn hcl_serialize_attributes() {
        let val = Value::from_pairs([
            ("name", Value::string("test")),
            ("count", Value::int(42)),
        ]);
        let bytes = serialize(FormatKind::Hcl, &val).unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("name"));
        assert!(text.contains("test"));
    }

    #[test]
    fn hcl_serialize_block() {
        let val = Value::from_pairs([
            ("resource", Value::block("resource", ["aws_instance", "web"], {
                let mut m = indexmap::IndexMap::new();
                m.insert(std::sync::Arc::from("ami"), Value::string("ami-12345"));
                m
            })),
        ]);
        let bytes = serialize(FormatKind::Hcl, &val).unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("resource"));
        assert!(text.contains("aws_instance"));
        assert!(text.contains("ami-12345"));
    }

    // ── CSV tests ────────────────────────────────────────────────────

    #[test]
    fn csv_type_inference() {
        let input = b"name,age,score,active\nAlice,30,95.5,true\nBob,25,,false";
        let val = parse(FormatKind::Csv, input).unwrap();
        let rows = val.as_array().unwrap();
        // First row
        assert_eq!(rows[0].get("name"), Some(&Value::string("Alice")));
        assert_eq!(rows[0].get("age"), Some(&Value::int(30)));
        assert_eq!(rows[0].get("score"), Some(&Value::float(95.5)));
        assert_eq!(rows[0].get("active"), Some(&Value::bool(true)));
        // Second row — empty cell becomes null
        assert_eq!(rows[1].get("score"), Some(&Value::Null));
    }

    #[test]
    fn csv_roundtrip() {
        let val = Value::array(vec![
            Value::from_pairs([("a", Value::int(1)), ("b", Value::string("x"))]),
            Value::from_pairs([("a", Value::int(2)), ("b", Value::string("y"))]),
        ]);
        let bytes = serialize(FormatKind::Csv, &val).unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("a,b"));
    }

    #[test]
    fn csv_all_strings() {
        let input = b"col\nhello\nworld";
        let val = parse(FormatKind::Csv, input).unwrap();
        let rows = val.as_array().unwrap();
        assert_eq!(rows[0].get("col"), Some(&Value::string("hello")));
    }

    #[test]
    fn csv_boolean_case_insensitive() {
        let input = b"flag\nTRUE\nFALSE\nTrue";
        let val = parse(FormatKind::Csv, input).unwrap();
        let rows = val.as_array().unwrap();
        assert_eq!(rows[0].get("flag"), Some(&Value::bool(true)));
        assert_eq!(rows[1].get("flag"), Some(&Value::bool(false)));
        assert_eq!(rows[2].get("flag"), Some(&Value::bool(true)));
    }

    // ── MsgPack tests ────────────────────────────────────────────────

    #[test]
    fn msgpack_roundtrip() {
        let val = Value::from_pairs([
            ("key", Value::string("value")),
            ("num", Value::int(42)),
        ]);
        let bytes = serialize(FormatKind::MsgPack, &val).unwrap();
        let back = parse(FormatKind::MsgPack, &bytes).unwrap();
        assert_eq!(val, back);
    }

    #[test]
    fn msgpack_detect_from_content() {
        let val = Value::from_pairs([("x", Value::int(1))]);
        let bytes = serialize(FormatKind::MsgPack, &val).unwrap();
        // MsgPack is binary, first byte > 0x7F for map with string keys
        if bytes[0] > 0x7F {
            assert_eq!(detect::detect_from_content(&bytes), Some(FormatKind::MsgPack));
        }
    }

    // ── Cross-format conversion ──────────────────────────────────────

    #[test]
    fn json_to_yaml_conversion() {
        let json = br#"{"name":"test","items":[1,2,3]}"#;
        let val = parse(FormatKind::Json, json).unwrap();
        let yaml_bytes = serialize(FormatKind::Yaml, &val).unwrap();
        let back = parse(FormatKind::Yaml, &yaml_bytes).unwrap();
        assert_eq!(val, back);
    }

    #[test]
    fn yaml_to_json_conversion() {
        let yaml = b"name: test\ncount: 42";
        let val = parse(FormatKind::Yaml, yaml).unwrap();
        let json_bytes = serialize(FormatKind::Json, &val).unwrap();
        let back = parse(FormatKind::Json, &json_bytes).unwrap();
        assert_eq!(val, back);
    }

    // ── HCL expression preservation ──────────────────────────────────

    #[test]
    fn hcl_preserve_variable_expression() {
        // `var.region` is a traversal (variable `var` + `.region` attr access),
        // so __expr is "traversal" and the root is a variable named "var".
        let input = b"value = var.region";
        let val = parse(FormatKind::Hcl, input).unwrap();
        let expr = val.get("value").unwrap();
        assert_eq!(expr.get("__expr").unwrap().as_str(), Some("traversal"));
        // The root of the traversal is the variable "var"
        let root = expr.get("root").unwrap();
        assert_eq!(root.get("__expr").unwrap().as_str(), Some("variable"));
        assert_eq!(root.get("name").unwrap().as_str(), Some("var"));
        // And there's at least one operator (GetAttr "region")
        let ops = expr.get("operators").unwrap().as_array().unwrap();
        assert!(ops.len() >= 1);
    }

    #[test]
    fn hcl_preserve_traversal_expression() {
        let input = b"value = local.config.vpc_id";
        let val = parse(FormatKind::Hcl, input).unwrap();
        let expr = val.get("value").unwrap();
        assert_eq!(expr.get("__expr").unwrap().as_str(), Some("traversal"));
        let ops = expr.get("operators").unwrap().as_array().unwrap();
        assert!(ops.len() >= 1);
    }

    #[test]
    fn hcl_preserve_function_call() {
        let input = b"value = upper(\"hello\")";
        let val = parse(FormatKind::Hcl, input).unwrap();
        let expr = val.get("value").unwrap();
        assert_eq!(expr.get("__expr").unwrap().as_str(), Some("func_call"));
        assert_eq!(expr.get("name").unwrap().as_str(), Some("upper"));
        let args = expr.get("args").unwrap().as_array().unwrap();
        assert_eq!(args.len(), 1);
    }

    #[test]
    fn hcl_preserve_conditional_expression() {
        let input = b"value = true ? \"yes\" : \"no\"";
        let val = parse(FormatKind::Hcl, input).unwrap();
        let expr = val.get("value").unwrap();
        assert_eq!(expr.get("__expr").unwrap().as_str(), Some("conditional"));
        assert!(expr.get("condition").is_some());
        assert!(expr.get("true_val").is_some());
        assert!(expr.get("false_val").is_some());
    }

    #[test]
    fn hcl_is_expression_check() {
        let input = b"name = \"literal\"\nexpr = var.region";
        let val = parse(FormatKind::Hcl, input).unwrap();
        // Literal string is NOT an expression
        assert!(!crate::hcl::is_hcl_expression(val.get("name").unwrap()));
        // Variable reference IS an expression
        assert!(crate::hcl::is_hcl_expression(val.get("expr").unwrap()));
    }

    #[test]
    fn hcl_expression_func_name_extraction() {
        let input = b"value = jsonencode({})";
        let val = parse(FormatKind::Hcl, input).unwrap();
        let expr = val.get("value").unwrap();
        assert_eq!(crate::hcl::expression_func_name(expr), Some("jsonencode"));
    }
}
