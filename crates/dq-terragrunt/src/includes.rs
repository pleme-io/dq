//! Include chain resolution and merge.
//!
//! Traces the `include` block chain from a leaf module up to the root,
//! then merges configs according to each include's merge_strategy.

use crate::config::{MergeStrategy, TerragruntConfig};
use dq_core::{Error, Value};
use dq_merge::Strategy;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Resolved include chain from leaf → root.
#[derive(Clone, Debug)]
pub struct IncludeChain {
    pub configs: Vec<TerragruntConfig>,
    pub paths: Vec<PathBuf>,
}

/// Resolve the full include chain for a module, detecting cycles.
///
/// Follows **all** include blocks (not just the first), performing a
/// depth-first traversal of the include tree.  Each included config is
/// visited at most once; revisiting a path triggers a cycle error.
pub fn resolve_include_chain(start: &Path) -> Result<IncludeChain, Error> {
    let mut configs = Vec::new();
    let mut paths = Vec::new();
    let mut visited = HashSet::new();

    resolve_include_recursive(start, &mut configs, &mut paths, &mut visited)?;

    Ok(IncludeChain { configs, paths })
}

/// Recursive helper: parse the config at `current`, record it, then recurse
/// into every include it references.
fn resolve_include_recursive(
    current: &Path,
    configs: &mut Vec<TerragruntConfig>,
    paths: &mut Vec<PathBuf>,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), Error> {
    let canonical = current.canonicalize()
        .map_err(|e| Error::Parse(format!("canonicalize {}: {e}", current.display())))?;

    if !visited.insert(canonical.clone()) {
        return Err(Error::Other(format!(
            "include cycle detected: {} already visited",
            canonical.display()
        )));
    }

    let config = TerragruntConfig::from_path(current)?;
    let include_paths = config.all_include_paths();
    paths.push(current.to_path_buf());
    configs.push(config);

    // Follow every include, not just the first
    let base_dir = current.parent().unwrap_or(Path::new("."));
    for inc_path in &include_paths {
        let next = base_dir.join(inc_path);
        resolve_include_recursive(&next, configs, paths, visited)?;
    }

    Ok(())
}

/// Merge an include chain into a single resolved config Value.
/// Applies each include's merge_strategy in order (root first, leaf last).
pub fn merge_include_chain(chain: &IncludeChain) -> Value {
    // Reverse so root is first (lowest priority), leaf is last (highest)
    let reversed: Vec<_> = chain.configs.iter().rev().collect();

    if reversed.is_empty() {
        return Value::Null;
    }

    let mut result = reversed[0].raw.clone();
    for config in &reversed[1..] {
        // Determine merge strategy from this config's includes
        let strategy = config.includes.values()
            .next()
            .map(|inc| match inc.merge_strategy {
                MergeStrategy::Shallow => Strategy::Shallow,
                MergeStrategy::Deep => Strategy::Deep,
                MergeStrategy::NoMerge => Strategy::Replace,
            })
            .unwrap_or(Strategy::Shallow);

        result = dq_merge::merge(&result, &config.raw, strategy);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_hcl(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    // ── includes.rs tests (3) ─────────────────────────────────────

    #[test]
    fn single_include_chain() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let child_dir = root.join("envs").join("prod");
        std::fs::create_dir_all(&child_dir).unwrap();

        // Root config with no includes
        write_hcl(root, "terragrunt.hcl", r#"
locals {
  project = "myproject"
}
"#);

        // Child config that includes the root
        write_hcl(&child_dir, "terragrunt.hcl", &format!(r#"
include "root" {{
  path = "{}"
}}

locals {{
  env = "prod"
}}
"#, root.join("terragrunt.hcl").to_string_lossy().replace('\\', "/")));

        let chain = resolve_include_chain(&child_dir.join("terragrunt.hcl")).unwrap();
        assert_eq!(chain.configs.len(), 2);
        assert_eq!(chain.paths.len(), 2);
        // First config is the child (starting point)
        assert!(chain.paths[0].to_string_lossy().contains("prod"));
        // Second is the root (resolved include)
        assert!(!chain.paths[1].to_string_lossy().contains("prod"));
    }

    #[test]
    fn cycle_detection() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let a_dir = root.join("a");
        let b_dir = root.join("b");
        std::fs::create_dir_all(&a_dir).unwrap();
        std::fs::create_dir_all(&b_dir).unwrap();

        // A includes B
        write_hcl(&a_dir, "terragrunt.hcl", &format!(r#"
include "b" {{
  path = "{}"
}}
"#, b_dir.join("terragrunt.hcl").to_string_lossy().replace('\\', "/")));

        // B includes A (creates a cycle)
        write_hcl(&b_dir, "terragrunt.hcl", &format!(r#"
include "a" {{
  path = "{}"
}}
"#, a_dir.join("terragrunt.hcl").to_string_lossy().replace('\\', "/")));

        let result = resolve_include_chain(&a_dir.join("terragrunt.hcl"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("cycle"), "Expected cycle error, got: {err}");
    }

    #[test]
    fn merge_strategy_chain() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let child_dir = root.join("child");
        std::fs::create_dir_all(&child_dir).unwrap();

        // Root config
        write_hcl(root, "terragrunt.hcl", r#"
inputs = {
  base_key = "from_root"
  shared   = "root_value"
}
"#);

        // Child config with deep merge strategy
        write_hcl(&child_dir, "terragrunt.hcl", &format!(r#"
include "root" {{
  path           = "{}"
  merge_strategy = "deep"
}}

inputs = {{
  child_key = "from_child"
  shared    = "child_value"
}}
"#, root.join("terragrunt.hcl").to_string_lossy().replace('\\', "/")));

        let chain = resolve_include_chain(&child_dir.join("terragrunt.hcl")).unwrap();
        let merged = merge_include_chain(&chain);

        // The merged result should have inputs from both configs
        // Child's include has merge_strategy = deep, so inputs are deep-merged
        let inputs = merged.get("inputs");
        assert!(inputs.is_some(), "merged result should contain inputs");
    }
}
