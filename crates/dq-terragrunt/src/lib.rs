//! # dq-terragrunt — Terragrunt Config Introspection
//!
//! Parse `terragrunt.hcl` files into [`dq_core::Value`], resolve dependency
//! graphs, trace include chains, and generate documentation artifacts.
//!
//! All operations work on the core dq data structures — no Terragrunt CLI
//! or Go runtime needed.

pub mod config;
pub mod dag;
pub mod eval;
pub mod includes;
pub mod render;

pub use config::TerragruntConfig;
pub use dag::DependencyGraph;
pub use eval::TerragruntEvalContext;
