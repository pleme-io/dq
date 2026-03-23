//! Error types for dq-core.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("type error: expected {expected}, got {actual}")]
    TypeError {
        expected: &'static str,
        actual: &'static str,
    },

    #[error("key not found: {0}")]
    KeyNotFound(String),

    #[error("index out of bounds: {index} (length {length})")]
    IndexOutOfBounds { index: usize, length: usize },

    #[error("invalid path: {0}")]
    InvalidPath(String),

    #[error("format error: {0}")]
    Format(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("merge conflict at {path}: {message}")]
    MergeConflict { path: String, message: String },

    #[error("{0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_type_error() {
        let e = Error::TypeError { expected: "string", actual: "number" };
        assert_eq!(e.to_string(), "type error: expected string, got number");
    }

    #[test]
    fn display_key_not_found() {
        let e = Error::KeyNotFound("foo".into());
        assert_eq!(e.to_string(), "key not found: foo");
    }

    #[test]
    fn display_index_out_of_bounds() {
        let e = Error::IndexOutOfBounds { index: 5, length: 3 };
        assert_eq!(e.to_string(), "index out of bounds: 5 (length 3)");
    }

    #[test]
    fn display_merge_conflict() {
        let e = Error::MergeConflict { path: "a.b".into(), message: "both modified".into() };
        assert_eq!(e.to_string(), "merge conflict at a.b: both modified");
    }

    #[test]
    fn error_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
    }
}
