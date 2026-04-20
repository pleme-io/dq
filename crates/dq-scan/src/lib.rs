//! # dq-scan -- Directory Analysis and Topology Mapping for GitOps Infrastructure
//!
//! Scans a GitOps-structured repository (ArgoCD ApplicationSets, Helm charts,
//! environment hierarchies) and builds a topology graph that captures the
//! relationships between components.
//!
//! All repository-specific patterns (directory names, file types, markers) are
//! driven by [`ScanConfig`] — loaded from `.dq.yaml`/`.dq.json` at the repo
//! root, or using sensible defaults. No infrastructure-specific knowledge is
//! hardcoded in the library.
//!
//! ## Architecture
//!
//! - [`config`] -- Configuration that drives all discovery and classification.
//! - [`discovery`] -- Walk directory trees, discover files by extension and pattern.
//! - [`argocd`] -- Parse ArgoCD ApplicationSet YAMLs into structured `AppSetInfo`.
//! - [`helm`] -- Parse `Chart.yaml` metadata into `ChartInfo`.
//! - [`environments`] -- Walk environment directory hierarchies to extract taxonomy.
//! - [`topology`] -- Assemble the full topology graph with typed edges.
//! - [`report`] -- Serialize topology to JSON `Value`, DOT, or summary.

pub mod config;
pub mod discovery;
pub mod argocd;
pub mod helm;
pub mod environments;
pub mod modules;
pub mod topology;
pub mod report;

pub use config::ScanConfig;
pub use modules::{scan_modules, TerraformModule};
pub use topology::{Topology, ScanResult};

use std::path::Path;

/// Scan a directory root and produce a full topology result.
///
/// Loads configuration from `.dq.yaml`/`.dq.json` at root, or uses defaults.
pub fn scan_directory(root: &Path) -> Result<ScanResult, ScanError> {
    let config = ScanConfig::load_or_default(root);
    scan_directory_with_config(root, &config)
}

/// Scan a directory with an explicit configuration.
pub fn scan_directory_with_config(root: &Path, config: &ScanConfig) -> Result<ScanResult, ScanError> {
    topology::build_topology(root, config)
}

/// Errors that can occur during scanning.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parse error at {path}: {message}")]
    YamlParse { path: String, message: String },

    #[error("missing field {field} in {path}")]
    MissingField { field: String, path: String },

    #[error("unexpected structure in {path}: {message}")]
    UnexpectedStructure { path: String, message: String },
}
