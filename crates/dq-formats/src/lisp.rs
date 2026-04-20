//! Canonical tatara-lisp s-expression form for dq ``Value``.
//!
//! Bridges via serde_json as the common intermediate — tatara-lisp
//! already provides a symmetric ``sexp_to_json`` / ``json_to_sexp``
//! pair, and ``dq_core::Value`` has a bidirectional conversion to
//! ``serde_json::Value``. Composing the two gives us a full IR
//! round-trip without writing a fourth Value↔Sexp converter.
//!
//! Wire shape:
//!
//! * Objects → `(key1 val1 key2 val2 …)` with keyword-prefixed keys
//!   (tatara-lisp's convention; matches the shikumi provider).
//! * Arrays → `(item1 item2 …)` (no quoting; tatara-lisp lists are
//!   already heterogeneous).
//! * Strings → `"escaped"` via Sexp::Str.
//! * Numbers / Bools / Null → native atoms.
//!
//! The tatara-lisp `Display` impl is deterministic: same AST →
//! byte-identical output.
//!
//! This format is opt-in via the ``lisp`` Cargo feature, which pulls
//! the tatara-lisp crate from the pleme-io monorepo.

use dq_core::{Error, Value};
use tatara_lisp::domain::{json_to_sexp, sexp_to_json};
use tatara_lisp::reader::read;

use crate::Format;

pub struct LispFormat;

impl Format for LispFormat {
    fn parse(&self, input: &[u8]) -> Result<Value, Error> {
        let src = std::str::from_utf8(input)
            .map_err(|e| Error::Format(format!("lisp input must be utf-8: {e}")))?;
        let sexps = read(src).map_err(|e| Error::Format(format!("lisp parse error: {e}")))?;
        // A typical dq Lisp file has a single top-level form
        // wrapping the whole document. If a caller emits multiple
        // forms we wrap them in an implicit list so nothing is lost.
        let sexp = match sexps.len() {
            0 => return Ok(Value::Null),
            1 => sexps.into_iter().next().unwrap(),
            _ => tatara_lisp::ast::Sexp::List(sexps),
        };
        let json = sexp_to_json(&sexp);
        Ok(Value::from(json))
    }

    fn serialize(&self, value: &Value) -> Result<Vec<u8>, Error> {
        let json: serde_json::Value = value.into();
        let sexp = json_to_sexp(&json);
        // ``Sexp: Display`` produces the canonical form.
        let mut text = format!("{}", sexp);
        if !text.ends_with('\n') {
            text.push('\n');
        }
        Ok(text.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq_core::Value;

    #[test]
    fn lisp_roundtrip_simple_map() {
        let val = Value::from_pairs([
            ("name", Value::string("topology")),
            ("count", Value::int(42)),
        ]);
        let bytes = LispFormat.serialize(&val).unwrap();
        let back = LispFormat.parse(&bytes).unwrap();
        assert_eq!(val, back);
    }

    #[test]
    fn lisp_roundtrip_nested_arrays() {
        let val = Value::from_pairs([(
            "items",
            Value::array(vec![
                Value::int(1),
                Value::string("two"),
                Value::bool(true),
            ]),
        )]);
        let bytes = LispFormat.serialize(&val).unwrap();
        let back = LispFormat.parse(&bytes).unwrap();
        assert_eq!(val, back);
    }

    #[test]
    fn lisp_canonical_is_deterministic() {
        let val = Value::from_pairs([
            ("a", Value::int(1)),
            ("b", Value::int(2)),
        ]);
        let bytes1 = LispFormat.serialize(&val).unwrap();
        let bytes2 = LispFormat.serialize(&val).unwrap();
        assert_eq!(bytes1, bytes2);
    }
}
