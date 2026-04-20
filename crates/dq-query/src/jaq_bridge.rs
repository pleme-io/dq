//! Bridge between dq-core Values and the jaq query engine.
//!
//! Converts dq_core::Value to/from jaq_json::Val, parses and compiles
//! jq expressions via jaq-core, and executes them.

use dq_core::{Error, Value};
use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Compiler, Ctx, RcIter};
use jaq_json::Val;

/// dq-provided jq prelude. Every user expression is compiled with
/// these definitions in scope, so consumers don't re-define them in
/// every query. Scoped to helpers that are genuinely universal across
/// HCL / YAML / JSON — no format-specific or schema-specific content
/// lives here (that belongs in `.dq.yaml` config or downstream tools).
const DQ_PRELUDE: &str = r#"
# as_blocks: canonicalise single-or-array HCL block output to array.
# dq emits a single HCL block of a type as an object (for ergonomic
# field access) and multiple blocks as an array — ``as_blocks``
# normalises both so callers can always iterate with `.[]`.
def as_blocks: if . == null then [] elif type == "array" then . else [.] end;

# try_path: multi-step optional chaining. `try_path(["a", "b", "c"])`
# returns `.a.b.c` if every step exists, else null. Safer than
# `.a?.b?.c?` which behaves inconsistently across jq dialects.
def try_path(path):
  reduce path[] as $k (.; if type == "object" and has($k) then .[$k] else null end);

# hcl_string: coerce an HCL-valued field (literal, template, or
# function-call expression) to its printable string form. Returns the
# input unchanged if already a string.
def hcl_string:
  if type == "string" then .
  elif type == "object" and .__expr? == "template" then .template
  elif type == "object" and .__expr? == "func_call" then
    "\(.name)(\( (.args // []) | map(tostring) | join(", ") ))"
  elif type == "object" and has("template") then .template
  elif type == "object" and has("name") then .name
  else tostring end;

# blocks_of_type(kind): recursively find every HCL block with the
# given ``__block_type``. Useful for "extract all resources from a
# module" without caring which file they're in.
def blocks_of_type($kind):
  [.. | objects | select(.__block_type? == $kind)];

# expressions_of_type(kind): recursively find every HCL expression
# with the given ``__expr``. Useful for auditing function-call usage
# (``func_call``), refs to variables (``variable``), etc.
def expressions_of_type($kind):
  [.. | objects | select(.__expr? == $kind)];
"#;

/// Execute a jq expression against a serde_json::Value via jaq.
/// Returns all output values.
pub(crate) fn run_jaq(input: &Value, expression: &str) -> Result<Vec<Value>, Error> {
    // 1. Convert dq_core::Value -> serde_json::Value -> jaq_json::Val
    let json_val: serde_json::Value = input.into();
    let jaq_val = Val::from(json_val);

    // 2. Parse the expression — prepend the dq prelude so every
    // query has access to `as_blocks`, `try_path`, `hcl_string`, etc.
    // without the caller having to define them.
    let arena = Arena::default();
    let loader = Loader::new(jaq_std::defs().chain(jaq_json::defs()));
    let combined_code = format!("{}\n{}", DQ_PRELUDE, expression);
    let program = File {
        code: combined_code.as_str(),
        path: (),
    };

    let modules = loader.load(&arena, program).map_err(|errs| {
        let messages: Vec<String> = errs
            .into_iter()
            .flat_map(|(file, err)| {
                let code = file.code;
                match err {
                    jaq_core::load::Error::Lex(lex_errs) => lex_errs
                        .into_iter()
                        .map(|e| format!("lex error in '{}': {:?}", code, e))
                        .collect::<Vec<_>>(),
                    jaq_core::load::Error::Parse(parse_errs) => parse_errs
                        .into_iter()
                        .map(|(expected, found)| {
                            format!(
                                "parse error in '{}': expected {:?}, found {:?}",
                                code, expected, found
                            )
                        })
                        .collect::<Vec<_>>(),
                    jaq_core::load::Error::Io(io_errs) => io_errs
                        .into_iter()
                        .map(|(path, msg)| format!("io error at '{}': {}", path, msg))
                        .collect::<Vec<_>>(),
                }
            })
            .collect();
        Error::Parse(messages.join("; "))
    })?;

    // 3. Compile
    let filter = Compiler::default()
        .with_funs(jaq_std::funs().chain(jaq_json::funs()))
        .compile(modules)
        .map_err(|errs| {
            let messages: Vec<String> = errs
                .into_iter()
                .flat_map(|(file, compile_errs)| {
                    let code = file.code;
                    compile_errs
                        .into_iter()
                        .map(move |(name, undef)| {
                            format!(
                                "compile error in '{}': undefined {} '{}'",
                                code,
                                undef.as_str(),
                                name
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            Error::Parse(messages.join("; "))
        })?;

    // 4. Execute
    let inputs = RcIter::new(core::iter::empty());
    let out = filter.run((Ctx::new([], &inputs), jaq_val));

    // 5. Collect results, convert back: jaq_json::Val -> serde_json::Value -> dq_core::Value
    let mut results = Vec::new();
    for item in out {
        match item {
            Ok(val) => {
                let json: serde_json::Value = val.into();
                results.push(Value::from(json));
            }
            Err(err) => {
                return Err(Error::Other(format!("jaq runtime error: {err}")));
            }
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bridge_identity() {
        let input = Value::from(json!({"a": 1, "b": 2}));
        let result = run_jaq(&input, ".").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], input);
    }

    #[test]
    fn bridge_field_access() {
        let input = Value::from(json!({"name": "alice"}));
        let result = run_jaq(&input, ".name").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], Value::from(json!("alice")));
    }

    #[test]
    fn bridge_parse_error() {
        let input = Value::from(json!(null));
        let err = run_jaq(&input, ".[invalid???").unwrap_err();
        match err {
            Error::Parse(_) => {} // expected
            other => panic!("expected Parse error, got: {other}"),
        }
    }
}
