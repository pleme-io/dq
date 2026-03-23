//! # dq-core — Universal Infrastructure Data Structures
//!
//! The core value type that all dq operations work on. Every format
//! (JSON, YAML, TOML, HCL, Terragrunt, CSV, MessagePack) deserializes
//! into [`Value`] and serializes back out from it.
//!
//! Unlike `serde_json::Value` (6 variants, lossy), this type preserves:
//! - Integer vs float distinction (TOML, MessagePack)
//! - Datetime (TOML native type)
//! - Ordered maps (YAML, TOML, HCL)
//! - HCL blocks with labels (Terraform/Terragrunt structure)
//! - Binary data (MessagePack, CBOR)
//! - Source location tracking (for error reporting)
//!
//! All operations in dq-merge, dq-query, dq-terragrunt operate on this type.

pub mod value;
pub mod path;
pub mod error;

pub use value::Value;
pub use path::Path;
pub use error::Error;
