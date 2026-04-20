//! HCL format via hcl-rs. Preserves block structure as [`Value::Block`].

use crate::Format;
use dq_core::{Error, Value};
use indexmap::IndexMap;
use std::sync::Arc;

pub struct HclFormat;

impl Format for HclFormat {
    fn parse(&self, input: &[u8]) -> Result<Value, Error> {
        let s = std::str::from_utf8(input)
            .map_err(|e| Error::Parse(format!("HCL: invalid UTF-8: {e}")))?;
        let body: hcl::Body = hcl::from_str(s)
            .map_err(|e| Error::Parse(format!("HCL: {e}")))?;
        Ok(from_hcl_body(&body))
    }

    fn serialize(&self, value: &Value) -> Result<Vec<u8>, Error> {
        let body = to_hcl_body(value);
        hcl::format::to_string(&body)
            .map(|s| s.into_bytes())
            .map_err(|e| Error::Format(format!("HCL serialize: {e}")))
    }
}

fn from_hcl_body(body: &hcl::Body) -> Value {
    let mut map = IndexMap::new();

    for structure in body.iter() {
        match structure {
            hcl::Structure::Attribute(attr) => {
                let key: Arc<str> = Arc::from(attr.key.as_str());
                let value = from_hcl_expression(&attr.expr);
                map.insert(key, value);
            }
            hcl::Structure::Block(block) => {
                let block_type: Arc<str> = Arc::from(block.identifier.as_str());
                let labels: Vec<Arc<str>> = block.labels.iter()
                    .map(|l| Arc::from(l.as_str()))
                    .collect();
                let body = from_hcl_body(&block.body);
                let body_map = match body {
                    Value::Map(m) => m,
                    _ => Arc::new(IndexMap::new()),
                };

                let block_val = Value::Block(dq_core::value::Block {
                    block_type: block_type.clone(),
                    labels,
                    body: body_map,
                });

                // Always array. Every HCL block type lands as a list
                // of blocks, even when the source file declares only
                // one — callers iterate with `.[]` unconditionally.
                // This is a breaking change relative to the old
                // "smart" behaviour but eliminates the single-vs-many
                // ambiguity every consumer had to paper over.
                let key = block_type;
                if let Some(existing) = map.get(&key) {
                    if let Value::Array(arr) = existing {
                        let mut new_arr = arr.as_ref().clone();
                        new_arr.push(block_val);
                        map.insert(key, Value::Array(Arc::new(new_arr)));
                    } else {
                        // Defensive — this branch shouldn't fire now
                        // that we always emit arrays, but keeps the
                        // type system honest if a caller hand-inserts
                        // a non-array value under a block-type key.
                        map.insert(key, Value::array(vec![existing.clone(), block_val]));
                    }
                } else {
                    map.insert(key, Value::array(vec![block_val]));
                }
            }
        }
    }

    Value::Map(Arc::new(map))
}

/// Convert a dq Value back to an hcl::Body for serialization.
fn to_hcl_body(value: &Value) -> hcl::Body {
    let mut structures = Vec::new();
    match value {
        Value::Map(m) => push_map_to_structures(&mut structures, m),
        Value::Block(b) => push_map_to_structures(&mut structures, &b.body),
        _ => {
            structures.push(hcl::Structure::Attribute(
                hcl::Attribute::new("value", to_hcl_expression(value)),
            ));
        }
    }
    hcl::Body(structures)
}

fn make_hcl_block(key: &str, b: &dq_core::value::Block) -> hcl::Block {
    let labels: Vec<hcl::BlockLabel> = b.labels.iter()
        .map(|l| hcl::BlockLabel::String(l.to_string()))
        .collect();
    let mut body_structures = Vec::new();
    push_map_to_structures(&mut body_structures, &b.body);
    hcl::Block {
        identifier: hcl::Identifier::unchecked(key),
        labels,
        body: hcl::Body(body_structures),
    }
}

fn to_hcl_structure(key: &str, value: &Value) -> hcl::Structure {
    match value {
        Value::Block(b) => hcl::Structure::Block(make_hcl_block(key, b)),
        _ => hcl::Structure::Attribute(
            hcl::Attribute::new(hcl::Identifier::unchecked(key), to_hcl_expression(value)),
        ),
    }
}

fn to_hcl_expression(value: &Value) -> hcl::Expression {
    match value {
        Value::Null => hcl::Expression::Null,
        Value::Bool(b) => hcl::Expression::Bool(*b),
        Value::Int(n) => hcl::Expression::Number((*n).into()),
        Value::Float(f) => hcl::Expression::Number(
            hcl::Number::from_f64(*f).unwrap_or_else(|| hcl::Number::from(0)),
        ),
        Value::String(s) => hcl::Expression::String(s.to_string()),
        Value::Datetime(s) => hcl::Expression::String(s.to_string()),
        Value::Bytes(b) => {
            use base64::Engine;
            hcl::Expression::String(base64::engine::general_purpose::STANDARD.encode(b.as_ref()))
        }
        Value::Array(a) => {
            hcl::Expression::Array(a.iter().map(to_hcl_expression).collect())
        }
        Value::Map(m) => {
            let obj: Vec<(hcl::ObjectKey, hcl::Expression)> = m
                .iter()
                .map(|(k, v)| {
                    (
                        hcl::ObjectKey::Identifier(hcl::Identifier::unchecked(k.as_ref())),
                        to_hcl_expression(v),
                    )
                })
                .collect();
            hcl::Expression::Object(obj.into_iter().collect())
        }
        Value::Block(b) => {
            let obj: Vec<(hcl::ObjectKey, hcl::Expression)> = b
                .body
                .iter()
                .map(|(k, v)| {
                    (
                        hcl::ObjectKey::Identifier(hcl::Identifier::unchecked(k.as_ref())),
                        to_hcl_expression(v),
                    )
                })
                .collect();
            hcl::Expression::Object(obj.into_iter().collect())
        }
    }
}

/// Push map entries into a structures vec, handling array-of-blocks specially.
fn push_map_to_structures(out: &mut Vec<hcl::Structure>, m: &IndexMap<Arc<str>, Value>) {
    for (k, v) in m.iter() {
        match v {
            Value::Array(arr) if arr.iter().all(|v| v.is_block()) => {
                for item in arr.iter() {
                    if let Value::Block(b) = item {
                        out.push(hcl::Structure::Block(make_hcl_block(k, b)));
                    }
                }
            }
            _ => out.push(to_hcl_structure(k, v)),
        }
    }
}

fn from_hcl_expression(expr: &hcl::Expression) -> Value {
    match expr {
        hcl::Expression::Null => Value::Null,
        hcl::Expression::Bool(b) => Value::Bool(*b),
        hcl::Expression::Number(n) => {
            if let Some(i) = n.as_i64() { Value::Int(i) }
            else if let Some(f) = n.as_f64() { Value::Float(f) }
            else { Value::string(n.to_string()) }
        }
        hcl::Expression::String(s) => Value::string(s.as_str()),
        hcl::Expression::Array(arr) => {
            Value::array(arr.iter().map(from_hcl_expression).collect::<Vec<_>>())
        }
        hcl::Expression::Object(obj) => {
            let map: IndexMap<Arc<str>, Value> = obj.iter()
                .map(|(k, v)| {
                    let key = match k {
                        hcl::ObjectKey::Identifier(id) => Arc::from(id.as_str()),
                        hcl::ObjectKey::Expression(e) => Arc::from(format!("{e}").as_str()),
                        _ => Arc::from(format!("{k:?}").as_str()),
                    };
                    (key, from_hcl_expression(v))
                })
                .collect();
            Value::map(map)
        }
        // Preserve complex expressions as structured data instead of naive
        // stringification. Uses `__expr` metadata key to mark the expression type.
        // Structured operands are extracted where possible.
        hcl::Expression::Variable(var) => {
            Value::from_pairs([
                ("__expr", Value::string("variable")),
                ("name", Value::string(var.as_str())),
            ])
        }
        hcl::Expression::Traversal(traversal) => {
            let root = from_hcl_expression(&traversal.expr);
            let ops: Vec<Value> = traversal.operators.iter()
                .map(|op| match op {
                    hcl::TraversalOperator::GetAttr(id) => Value::from_pairs([
                        ("type", Value::string("get_attr")),
                        ("name", Value::string(id.as_str())),
                    ]),
                    hcl::TraversalOperator::Index(expr) => Value::from_pairs([
                        ("type", Value::string("index")),
                        ("expr", from_hcl_expression(expr)),
                    ]),
                    hcl::TraversalOperator::AttrSplat => Value::from_pairs([
                        ("type", Value::string("attr_splat")),
                    ]),
                    hcl::TraversalOperator::FullSplat => Value::from_pairs([
                        ("type", Value::string("full_splat")),
                    ]),
                    _ => Value::from_pairs([
                        ("type", Value::string("unknown_op")),
                    ]),
                })
                .collect();
            Value::from_pairs([
                ("__expr", Value::string("traversal")),
                ("root", root),
                ("operators", Value::array(ops)),
            ])
        }
        hcl::Expression::FuncCall(func_call) => {
            let full_name = if func_call.name.namespace.is_empty() {
                func_call.name.name.as_str().to_string()
            } else {
                let ns: Vec<&str> = func_call.name.namespace.iter()
                    .map(|id| id.as_str())
                    .collect();
                format!("{}::{}", ns.join("::"), func_call.name.name.as_str())
            };
            let args: Vec<Value> = func_call.args.iter()
                .map(from_hcl_expression)
                .collect();
            Value::from_pairs([
                ("__expr", Value::string("func_call")),
                ("name", Value::string(full_name)),
                ("args", Value::array(args)),
            ])
        }
        hcl::Expression::Conditional(cond) => {
            Value::from_pairs([
                ("__expr", Value::string("conditional")),
                ("condition", from_hcl_expression(&cond.cond_expr)),
                ("true_val", from_hcl_expression(&cond.true_expr)),
                ("false_val", from_hcl_expression(&cond.false_expr)),
            ])
        }
        hcl::Expression::Operation(op) => {
            match op.as_ref() {
                hcl::Operation::Binary(bin) => Value::from_pairs([
                    ("__expr", Value::string("binary_op")),
                    ("lhs", from_hcl_expression(&bin.lhs_expr)),
                    ("rhs", from_hcl_expression(&bin.rhs_expr)),
                ]),
                hcl::Operation::Unary(un) => Value::from_pairs([
                    ("__expr", Value::string("unary_op")),
                    ("expr", from_hcl_expression(&un.expr)),
                ]),
            }
        }
        hcl::Expression::ForExpr(for_expr) => {
            let mut parts = vec![
                ("__expr", Value::string("for_expr")),
                ("value_var", Value::string(for_expr.value_var.as_str())),
                ("collection", from_hcl_expression(&for_expr.collection_expr)),
            ];
            if let Some(key_var) = &for_expr.key_var {
                parts.push(("key_var", Value::string(key_var.as_str())));
            }
            Value::from_pairs(parts)
        }
        hcl::Expression::TemplateExpr(tmpl) => {
            // TemplateExpr wraps a string template like "${var.name}-suffix"
            // Serialize the template to string to preserve it
            let tmpl_str = hcl::to_string(&hcl::Attribute::new("_t", hcl::Expression::TemplateExpr(tmpl.clone())))
                .unwrap_or_default();
            // Extract just the value part after "= "
            let val = tmpl_str.split(" = ").nth(1)
                .map(|s| s.trim().trim_matches('"'))
                .unwrap_or("");
            Value::from_pairs([
                ("__expr", Value::string("template")),
                ("template", Value::string(val)),
            ])
        }
        other => {
            // Remaining: Parenthesis, Heredoc, etc.
            // Serialize via hcl::to_string for best-effort raw representation
            let raw = hcl::to_string(&hcl::Attribute::new("_v", other.clone()))
                .ok()
                .and_then(|s| s.split(" = ").nth(1).map(|v| v.trim().to_string()))
                .unwrap_or_else(|| format!("{other:?}"));
            Value::from_pairs([
                ("__expr", Value::string("other")),
                ("raw", Value::string(raw)),
            ])
        }
    }
}

/// Check if a Value is an unevaluated HCL expression (has `__expr` marker).
pub fn is_hcl_expression(value: &Value) -> bool {
    value.get("__expr").is_some()
}

/// Extract the function name from an HCL expression Value, if it's a func_call.
pub fn expression_func_name(value: &Value) -> Option<&str> {
    if value.get("__expr").and_then(|v| v.as_str()) == Some("func_call") {
        value.get("name").and_then(|v| v.as_str())
    } else {
        None
    }
}

/// Convert hcl::Value to dq_core::Value.
pub fn from_hcl_value(v: &hcl::value::Value) -> Value {
    match v {
        hcl::value::Value::Null => Value::Null,
        hcl::value::Value::Bool(b) => Value::Bool(*b),
        hcl::value::Value::Number(n) => {
            if let Some(i) = n.as_i64() { Value::Int(i) }
            else if let Some(f) = n.as_f64() { Value::Float(f) }
            else { Value::string(n.to_string()) }
        }
        hcl::value::Value::String(s) => Value::string(s.as_str()),
        hcl::value::Value::Array(a) => {
            Value::array(a.iter().map(from_hcl_value).collect::<Vec<_>>())
        }
        hcl::value::Value::Object(m) => {
            let map: IndexMap<Arc<str>, Value> = m.iter()
                .map(|(k, v)| (Arc::from(k.as_str()), from_hcl_value(v)))
                .collect();
            Value::map(map)
        }
    }
}
