//! Render Terragrunt configs with includes merged and dependencies resolved.
//! Equivalent to `terragrunt render --format json` but in pure Rust.

use crate::includes;
use dq_core::{Error, Value};
use indexmap::IndexMap;
use std::path::Path;
use std::sync::Arc;

/// Render a single module's effective configuration.
/// Resolves includes and merges them according to merge_strategy.
pub fn render_module(path: &Path) -> Result<Value, Error> {
    let hcl_path = if path.is_file() {
        path.to_path_buf()
    } else {
        path.join("terragrunt.hcl")
    };

    let chain = includes::resolve_include_chain(&hcl_path)?;
    let merged = includes::merge_include_chain(&chain);

    // Add metadata about the resolution
    let mut result = IndexMap::new();
    result.insert(Arc::from("_source"), Value::string(hcl_path.to_string_lossy().as_ref()));
    result.insert(
        Arc::from("_include_chain"),
        Value::array(
            chain.paths.iter()
                .map(|p| Value::string(p.to_string_lossy().as_ref()))
                .collect::<Vec<_>>()
        ),
    );

    // Merge the resolved config into the result
    if let Some(m) = merged.as_map() {
        for (k, v) in m.iter() {
            result.insert(k.clone(), v.clone());
        }
    }

    Ok(Value::map(result))
}
