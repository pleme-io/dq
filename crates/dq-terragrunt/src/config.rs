//! Terragrunt configuration parser.
//!
//! Parses `terragrunt.hcl` into typed structures backed by [`dq_core::Value`].
//! Extracts all block types: terraform, remote_state, include, dependency,
//! dependencies, generate, locals, inputs, feature, exclude, errors.

use dq_core::Value;
use indexmap::IndexMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A parsed Terragrunt configuration file.
#[derive(Clone, Debug)]
pub struct TerragruntConfig {
    /// Absolute path to the terragrunt.hcl file
    pub path: PathBuf,

    /// Raw parsed Value (full HCL body)
    pub raw: Value,

    /// terraform block: source, extra_arguments, hooks
    pub terraform: Option<TerraformBlock>,

    /// remote_state block: backend, config
    pub remote_state: Option<RemoteStateBlock>,

    /// include blocks (keyed by label)
    pub includes: IndexMap<Arc<str>, IncludeBlock>,

    /// dependency blocks (keyed by label)
    pub dependencies: IndexMap<Arc<str>, DependencyBlock>,

    /// dependencies block (just paths, no outputs)
    pub dependency_paths: Vec<PathBuf>,

    /// generate blocks (keyed by label)
    pub generates: IndexMap<Arc<str>, GenerateBlock>,

    /// locals block
    pub locals: IndexMap<Arc<str>, Value>,

    /// inputs block
    pub inputs: IndexMap<Arc<str>, Value>,
}

#[derive(Clone, Debug)]
pub struct TerraformBlock {
    pub source: Option<String>,
    pub extra_arguments: Vec<Value>,
    pub before_hooks: Vec<Value>,
    pub after_hooks: Vec<Value>,
    pub raw: Value,
}

#[derive(Clone, Debug)]
pub struct RemoteStateBlock {
    pub backend: String,
    pub config: Value,
    pub generate: Option<Value>,
    pub raw: Value,
}

#[derive(Clone, Debug)]
pub struct IncludeBlock {
    pub label: Arc<str>,
    pub path: String,
    pub expose: bool,
    pub merge_strategy: MergeStrategy,
    pub raw: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeStrategy {
    Shallow,
    Deep,
    NoMerge,
}

#[derive(Clone, Debug)]
pub struct DependencyBlock {
    pub label: Arc<str>,
    pub config_path: PathBuf,
    pub enabled: bool,
    pub skip_outputs: bool,
    pub mock_outputs: Option<Value>,
    pub raw: Value,
}

#[derive(Clone, Debug)]
pub struct GenerateBlock {
    pub label: Arc<str>,
    pub path: String,
    pub contents: String,
    pub if_exists: String,
    pub raw: Value,
}

impl TerragruntConfig {
    /// Parse a terragrunt.hcl file from disk.
    pub fn from_path(path: &Path) -> Result<Self, dq_core::Error> {
        let content = std::fs::read(path)
            .map_err(|e| dq_core::Error::Parse(format!("read {}: {e}", path.display())))?;
        let raw = dq_formats::parse(dq_formats::FormatKind::Hcl, &content)?;
        Self::from_value(path.to_path_buf(), raw)
    }

    /// Parse from an already-parsed Value.
    pub fn from_value(path: PathBuf, raw: Value) -> Result<Self, dq_core::Error> {
        let mut config = TerragruntConfig {
            path,
            raw: raw.clone(),
            terraform: None,
            remote_state: None,
            includes: IndexMap::new(),
            dependencies: IndexMap::new(),
            dependency_paths: Vec::new(),
            generates: IndexMap::new(),
            locals: IndexMap::new(),
            inputs: IndexMap::new(),
        };

        // Extract terraform block
        if let Some(tf) = raw.get("terraform") {
            let mut before_hooks = Vec::new();
            let mut after_hooks = Vec::new();

            // Collect before_hook sub-blocks
            if let Some(bh) = tf.get("before_hook") {
                collect_hook_blocks(bh, &mut before_hooks);
            }
            // Collect after_hook sub-blocks
            if let Some(ah) = tf.get("after_hook") {
                collect_hook_blocks(ah, &mut after_hooks);
            }

            config.terraform = Some(TerraformBlock {
                source: tf.get("source").and_then(|v| v.as_str()).map(String::from),
                extra_arguments: tf.get("extra_arguments")
                    .and_then(|v| v.as_array())
                    .map(|a| a.to_vec())
                    .unwrap_or_default(),
                before_hooks,
                after_hooks,
                raw: tf.clone(),
            });
        }

        // Extract remote_state block
        if let Some(rs) = raw.get("remote_state") {
            config.remote_state = Some(RemoteStateBlock {
                backend: rs.get("backend").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                config: rs.get("config").cloned().unwrap_or(Value::Null),
                generate: rs.get("generate").cloned(),
                raw: rs.clone(),
            });
        }

        // Extract include blocks
        if let Some(inc) = raw.get("include") {
            extract_labeled_blocks(inc, |label, val| {
                config.includes.insert(label.clone(), IncludeBlock {
                    label: label.clone(),
                    path: val.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    expose: val.get("expose").and_then(|v| v.as_bool()).unwrap_or(false),
                    merge_strategy: match val.get("merge_strategy").and_then(|v| v.as_str()) {
                        Some("deep") => MergeStrategy::Deep,
                        Some("no_merge") => MergeStrategy::NoMerge,
                        _ => MergeStrategy::Shallow,
                    },
                    raw: val.clone(),
                });
            });
        }

        // Extract dependency blocks
        if let Some(dep) = raw.get("dependency") {
            extract_labeled_blocks(dep, |label, val| {
                let config_path = val.get("config_path")
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from)
                    .unwrap_or_default();
                config.dependencies.insert(label.clone(), DependencyBlock {
                    label: label.clone(),
                    config_path,
                    enabled: val.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true),
                    skip_outputs: val.get("skip_outputs").and_then(|v| v.as_bool()).unwrap_or(false),
                    mock_outputs: val.get("mock_outputs").cloned(),
                    raw: val.clone(),
                });
            });
        }

        // Extract dependencies block (plural)
        if let Some(deps) = raw.get("dependencies") {
            if let Some(paths) = deps.get("paths").and_then(|v| v.as_array()) {
                for p in paths {
                    if let Some(s) = p.as_str() {
                        config.dependency_paths.push(PathBuf::from(s));
                    }
                }
            }
        }

        // Extract locals (may be a Block or a Map depending on HCL syntax)
        if let Some(locals) = raw.get("locals") {
            if let Some(m) = extract_inner_map(locals) {
                config.locals = m;
            }
        }

        // Extract inputs (may be a Block or a Map)
        if let Some(inputs) = raw.get("inputs") {
            if let Some(m) = extract_inner_map(inputs) {
                config.inputs = m;
            }
        }

        // Extract generate blocks
        if let Some(gen) = raw.get("generate") {
            extract_labeled_blocks(gen, |label, val| {
                config.generates.insert(label.clone(), GenerateBlock {
                    label: label.clone(),
                    path: val.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    contents: val.get("contents").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    if_exists: val.get("if_exists").and_then(|v| v.as_str()).unwrap_or("overwrite_terragrunt").to_string(),
                    raw: val.clone(),
                });
            });
        }

        Ok(config)
    }

    /// All config paths this module depends on (from dependency + dependencies blocks).
    pub fn all_dependency_paths(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self.dependencies.values()
            .filter(|d| d.enabled)
            .map(|d| d.config_path.clone())
            .collect();
        paths.extend(self.dependency_paths.iter().cloned());
        paths
    }

    /// All include paths.
    pub fn all_include_paths(&self) -> Vec<String> {
        self.includes.values().map(|i| i.path.clone()).collect()
    }
}

/// Extract the inner map from a Value that may be Map or Block.
/// HCL blocks (like `locals { ... }`) parse as `Value::Block`, while
/// attribute assignments (like `inputs = { ... }`) parse as `Value::Map`.
fn extract_inner_map(value: &Value) -> Option<IndexMap<Arc<str>, Value>> {
    match value {
        Value::Map(m) => Some(m.as_ref().clone()),
        Value::Block(b) => Some(b.body.as_ref().clone()),
        _ => None,
    }
}

/// Extract labeled blocks — handles both single Block and Array of Blocks.
fn extract_labeled_blocks<F>(value: &Value, mut handler: F)
where
    F: FnMut(&Arc<str>, &Value),
{
    match value {
        Value::Block(b) => {
            let label = b.labels.first()
                .cloned()
                .unwrap_or_else(|| Arc::from("default"));
            handler(&label, value);
        }
        Value::Array(arr) => {
            for item in arr.iter() {
                if let Value::Block(b) = item {
                    let label = b.labels.first()
                        .cloned()
                        .unwrap_or_else(|| Arc::from("default"));
                    handler(&label, item);
                }
            }
        }
        Value::Map(m) => {
            for (k, v) in m.iter() {
                handler(k, v);
            }
        }
        _ => {}
    }
}

/// Collect hook blocks (before_hook / after_hook) from a terraform sub-block.
/// Handles a single Block, an Array of Blocks, or a Map of named hooks.
fn collect_hook_blocks(value: &Value, out: &mut Vec<Value>) {
    match value {
        Value::Block(_) => {
            out.push(value.clone());
        }
        Value::Array(arr) => {
            for item in arr.iter() {
                out.push(item.clone());
            }
        }
        Value::Map(_) => {
            out.push(value.clone());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_hcl(dir: &std::path::Path, content: &str) -> std::path::PathBuf {
        let path = dir.join("terragrunt.hcl");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    // ── config.rs tests (9) ────────────────────────────────────────

    #[test]
    fn parse_terraform_block() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_hcl(tmp.path(), r#"
terraform {
  source = "tfr:///modules/vpc?version=1.0.0"
}
"#);
        let config = TerragruntConfig::from_path(&path).unwrap();
        let tf = config.terraform.unwrap();
        assert_eq!(tf.source.as_deref(), Some("tfr:///modules/vpc?version=1.0.0"));
    }

    #[test]
    fn parse_remote_state_block() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_hcl(tmp.path(), r#"
remote_state {
  backend = "s3"
  config = {
    bucket = "my-bucket"
    region = "us-east-1"
  }
}
"#);
        let config = TerragruntConfig::from_path(&path).unwrap();
        let rs = config.remote_state.unwrap();
        assert_eq!(rs.backend, "s3");
        assert_eq!(
            rs.config.get("bucket").and_then(|v| v.as_str()),
            Some("my-bucket")
        );
        assert_eq!(
            rs.config.get("region").and_then(|v| v.as_str()),
            Some("us-east-1")
        );
    }

    #[test]
    fn parse_include_block() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_hcl(tmp.path(), r#"
include "root" {
  path   = find_in_parent_folders()
  expose = true
  merge_strategy = "deep"
}
"#);
        let config = TerragruntConfig::from_path(&path).unwrap();
        assert_eq!(config.includes.len(), 1);
        let inc = config.includes.get("root").unwrap();
        assert_eq!(inc.label.as_ref(), "root");
        assert!(inc.expose);
        assert_eq!(inc.merge_strategy, MergeStrategy::Deep);
    }

    #[test]
    fn parse_dependency_block() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_hcl(tmp.path(), r#"
dependency "vpc" {
  config_path = "../vpc"
  skip_outputs = true
}
"#);
        let config = TerragruntConfig::from_path(&path).unwrap();
        assert_eq!(config.dependencies.len(), 1);
        let dep = config.dependencies.get("vpc").unwrap();
        assert_eq!(dep.label.as_ref(), "vpc");
        assert_eq!(dep.config_path, std::path::PathBuf::from("../vpc"));
        assert!(dep.skip_outputs);
        assert!(dep.enabled);
    }

    #[test]
    fn parse_dependencies_block() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_hcl(tmp.path(), r#"
dependencies {
  paths = ["../vpc", "../rds", "../iam"]
}
"#);
        let config = TerragruntConfig::from_path(&path).unwrap();
        assert_eq!(config.dependency_paths.len(), 3);
        assert_eq!(config.dependency_paths[0], std::path::PathBuf::from("../vpc"));
        assert_eq!(config.dependency_paths[1], std::path::PathBuf::from("../rds"));
        assert_eq!(config.dependency_paths[2], std::path::PathBuf::from("../iam"));
    }

    #[test]
    fn parse_generate_block() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_hcl(tmp.path(), r#"
generate "backend" {
  path      = "backend.tf"
  if_exists = "overwrite"
  contents  = "terraform { backend \"s3\" {} }"
}
"#);
        let config = TerragruntConfig::from_path(&path).unwrap();
        assert_eq!(config.generates.len(), 1);
        let gen = config.generates.get("backend").unwrap();
        assert_eq!(gen.label.as_ref(), "backend");
        assert_eq!(gen.path, "backend.tf");
        assert_eq!(gen.if_exists, "overwrite");
    }

    #[test]
    fn parse_locals_and_inputs() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_hcl(tmp.path(), r#"
locals {
  region = "us-east-1"
  env    = "prod"
}

inputs = {
  vpc_cidr = "10.0.0.0/16"
  enable_nat = true
}
"#);
        let config = TerragruntConfig::from_path(&path).unwrap();
        assert_eq!(config.locals.len(), 2);
        assert_eq!(
            config.locals.get("region").and_then(|v| v.as_str()),
            Some("us-east-1")
        );
        assert_eq!(
            config.locals.get("env").and_then(|v| v.as_str()),
            Some("prod")
        );
        assert_eq!(config.inputs.len(), 2);
        assert_eq!(
            config.inputs.get("vpc_cidr").and_then(|v| v.as_str()),
            Some("10.0.0.0/16")
        );
        assert_eq!(
            config.inputs.get("enable_nat").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn parse_hooks() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_hcl(tmp.path(), r#"
terraform {
  source = "."

  before_hook "tflint" {
    commands = ["plan"]
    execute  = ["tflint"]
  }

  after_hook "cleanup" {
    commands = ["apply"]
    execute  = ["rm", "-rf", ".terraform"]
  }
}
"#);
        let config = TerragruntConfig::from_path(&path).unwrap();
        let tf = config.terraform.unwrap();
        assert_eq!(tf.before_hooks.len(), 1);
        assert_eq!(tf.after_hooks.len(), 1);
        // Verify the before_hook has the right content
        let bh = &tf.before_hooks[0];
        assert!(bh.get("commands").is_some() || bh.get("execute").is_some()
            || bh.as_block().map(|b| b.labels.iter().any(|l| l.as_ref() == "tflint")).unwrap_or(false));
    }

    #[test]
    fn all_dependency_paths_composition() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_hcl(tmp.path(), r#"
dependency "vpc" {
  config_path = "../vpc"
}

dependency "rds" {
  config_path = "../rds"
  enabled = false
}

dependencies {
  paths = ["../iam"]
}
"#);
        let config = TerragruntConfig::from_path(&path).unwrap();
        let all_paths = config.all_dependency_paths();
        // vpc is enabled, rds is disabled, iam comes from dependencies block
        assert_eq!(all_paths.len(), 2);
        assert!(all_paths.contains(&std::path::PathBuf::from("../vpc")));
        assert!(all_paths.contains(&std::path::PathBuf::from("../iam")));
        // rds should NOT be included (enabled = false)
        assert!(!all_paths.contains(&std::path::PathBuf::from("../rds")));
    }
}
