//! Path expressions for navigating [`Value`] trees.
//!
//! Supports dot-notation (`a.b.c`), array indexing (`a.0.b`),
//! wildcards (`a.*.name`), and recursive descent (`a..name`).

use crate::Value;
use std::sync::Arc;

/// A parsed path expression.
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub segments: Vec<Segment>,
}

/// A single path segment.
#[derive(Clone, Debug, PartialEq)]
pub enum Segment {
    /// Named key (map/block field)
    Key(Arc<str>),
    /// Array index
    Index(usize),
    /// Wildcard — match all children
    Wildcard,
    /// Recursive descent — match at any depth
    RecursiveDescent,
}

impl Path {
    /// Parse a dot-notation path string.
    ///
    /// ```text
    /// "a.b.c"     → [Key("a"), Key("b"), Key("c")]
    /// "a.0.b"     → [Key("a"), Index(0), Key("b")]
    /// "a.*.name"  → [Key("a"), Wildcard, Key("name")]
    /// "a..name"   → [Key("a"), RecursiveDescent, Key("name")]
    /// ```
    pub fn parse(s: &str) -> Self {
        let mut segments = Vec::new();
        let parts: Vec<&str> = s.split('.').collect();
        let mut i = 0;

        while i < parts.len() {
            if parts[i].is_empty() {
                if i == 0 {
                    // Leading dot (e.g., ".foo") — skip
                    i += 1;
                    continue;
                }
                // Empty part in middle/end means consecutive dots (..)
                segments.push(Segment::RecursiveDescent);
                i += 1;
                continue;
            }

            let part = parts[i];
            if part == "*" {
                segments.push(Segment::Wildcard);
            } else if let Ok(idx) = part.parse::<usize>() {
                segments.push(Segment::Index(idx));
            } else {
                segments.push(Segment::Key(Arc::from(part)));
            }
            i += 1;
        }

        Path { segments }
    }

    /// Resolve this path against a value, returning all matches.
    pub fn resolve<'a>(&self, root: &'a Value) -> Vec<&'a Value> {
        let mut current = vec![root];

        for seg in &self.segments {
            let mut next = Vec::new();
            for val in &current {
                match seg {
                    Segment::Key(k) => {
                        if let Some(v) = val.get(k) {
                            next.push(v);
                        }
                    }
                    Segment::Index(i) => {
                        if let Some(v) = val.get_index(*i) {
                            next.push(v);
                        }
                    }
                    Segment::Wildcard => {
                        next.extend(val.values());
                    }
                    Segment::RecursiveDescent => {
                        collect_all_descendants(*val, &mut next);
                    }
                }
            }
            current = next;
        }

        current
    }
}

fn collect_all_descendants<'a>(val: &'a Value, out: &mut Vec<&'a Value>) {
    out.push(val);
    for child in val.values() {
        collect_all_descendants(child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // ── Parse tests ──────────────────────────────────────────────────

    #[test]
    fn parse_simple_keys() {
        let p = Path::parse("a.b.c");
        assert_eq!(p.segments, vec![
            Segment::Key(Arc::from("a")),
            Segment::Key(Arc::from("b")),
            Segment::Key(Arc::from("c")),
        ]);
    }

    #[test]
    fn parse_single_key() {
        let p = Path::parse("name");
        assert_eq!(p.segments, vec![Segment::Key(Arc::from("name"))]);
    }

    #[test]
    fn parse_with_index() {
        let p = Path::parse("items.0.name");
        assert_eq!(p.segments, vec![
            Segment::Key(Arc::from("items")),
            Segment::Index(0),
            Segment::Key(Arc::from("name")),
        ]);
    }

    #[test]
    fn parse_wildcard() {
        let p = Path::parse("a.*.name");
        assert_eq!(p.segments, vec![
            Segment::Key(Arc::from("a")),
            Segment::Wildcard,
            Segment::Key(Arc::from("name")),
        ]);
    }

    #[test]
    fn parse_recursive_descent() {
        let p = Path::parse("a..name");
        assert_eq!(p.segments, vec![
            Segment::Key(Arc::from("a")),
            Segment::RecursiveDescent,
            Segment::Key(Arc::from("name")),
        ]);
    }

    #[test]
    fn parse_empty_string() {
        let p = Path::parse("");
        assert!(p.segments.is_empty());
    }

    #[test]
    fn parse_large_index() {
        let p = Path::parse("arr.999");
        assert_eq!(p.segments, vec![
            Segment::Key(Arc::from("arr")),
            Segment::Index(999),
        ]);
    }

    // ── Resolve tests ────────────────────────────────────────────────

    #[test]
    fn resolve_simple_key() {
        let v = Value::from_pairs([("a", Value::int(1))]);
        let results = Path::parse("a").resolve(&v);
        assert_eq!(results, vec![&Value::int(1)]);
    }

    #[test]
    fn resolve_nested_keys() {
        let v = Value::from_pairs([
            ("a", Value::from_pairs([("b", Value::int(42))])),
        ]);
        let results = Path::parse("a.b").resolve(&v);
        assert_eq!(results, vec![&Value::int(42)]);
    }

    #[test]
    fn resolve_missing_key_returns_empty() {
        let v = Value::from_pairs([("a", Value::int(1))]);
        let results = Path::parse("b").resolve(&v);
        assert!(results.is_empty());
    }

    #[test]
    fn resolve_array_index() {
        let v = Value::from_pairs([
            ("items", Value::array(vec![Value::string("x"), Value::string("y")])),
        ]);
        let results = Path::parse("items.1").resolve(&v);
        assert_eq!(results, vec![&Value::string("y")]);
    }

    #[test]
    fn resolve_wildcard_on_map() {
        let v = Value::from_pairs([
            ("servers", Value::from_pairs([
                ("a", Value::from_pairs([("port", Value::int(80))])),
                ("b", Value::from_pairs([("port", Value::int(443))])),
            ])),
        ]);
        let results = Path::parse("servers.*.port").resolve(&v);
        assert_eq!(results, vec![&Value::int(80), &Value::int(443)]);
    }

    #[test]
    fn resolve_wildcard_on_array() {
        let v = Value::array(vec![Value::int(1), Value::int(2), Value::int(3)]);
        let results = Path::parse("*").resolve(&v);
        assert_eq!(results, vec![&Value::int(1), &Value::int(2), &Value::int(3)]);
    }

    #[test]
    fn resolve_recursive_descent() {
        let v = Value::from_pairs([
            ("a", Value::from_pairs([
                ("name", Value::string("inner")),
                ("b", Value::from_pairs([
                    ("name", Value::string("deep")),
                ])),
            ])),
        ]);
        let results = Path::parse("a..name").resolve(&v);
        // Recursive descent collects all descendants, then we filter by "name"
        // The "a" subtree descendants include: the map itself, "inner", the inner map, "deep"
        // Then filtering for "name" key on maps yields "inner" and "deep"
        let string_results: Vec<&str> = results.iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(string_results.contains(&"inner"));
        assert!(string_results.contains(&"deep"));
    }

    #[test]
    fn resolve_out_of_bounds_index_returns_empty() {
        let v = Value::array(vec![Value::int(1)]);
        let results = Path::parse("99").resolve(&v);
        assert!(results.is_empty());
    }
}
