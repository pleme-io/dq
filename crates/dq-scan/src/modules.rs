//! Scan reusable Terraform modules under a configurable root.
//!
//! The root defaults to ``saas/terraform/modules/`` and is overridable
//! via ``ScanConfig::terraform_modules_root``. The expected layout is
//! ``<root>/<cloud>/<module>/`` with any number of ``.tf`` files inside
//! (and optionally nested subdirectories, which are walked recursively).
//!
//! Each leaf module becomes one record with the structure the
//! topology layer consumes:
//!
//! ```text
//!   cloud:      "AWS"
//!   name:       "eks_cluster"
//!   path:       "saas/terraform/modules/AWS/eks_cluster"
//!   resources:  [{type, name}, …]    # from `resource "T" "N" {}` blocks
//!   variables:  [String, …]           # from `variable "N" {}` blocks
//!   outputs:    [String, …]           # from `output   "N" {}` blocks
//! ```
//!
//! All HCL parsing goes through ``dq_formats::parse`` with the HCL
//! format kind, so this scanner inherits the ecosystem's typed
//! representation (and the always-array block semantics). No regex
//! or ad-hoc tokenisation.

use dq_core::Value;
use indexmap::IndexMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::ScanConfig;

#[derive(Debug, Clone)]
pub struct TerraformModule {
    pub cloud: String,
    pub name: String,
    pub path: PathBuf,
    pub resources: Vec<(String, String)>,
    pub variables: Vec<String>,
    pub outputs: Vec<String>,
}

/// Walk the configured modules root and return one record per leaf module.
///
/// Silently skips directories whose `.tf` files don't parse — a hard
/// error in one module shouldn't abort the whole scan.
pub fn scan_modules(root: &Path, config: &ScanConfig) -> Vec<TerraformModule> {
    if config.terraform_modules_root.is_empty() {
        return vec![];
    }
    let modules_root = root.join(&config.terraform_modules_root);
    if !modules_root.is_dir() {
        return vec![];
    }

    let mut out: Vec<TerraformModule> = Vec::new();
    let mut cloud_dirs = match std::fs::read_dir(&modules_root) {
        Ok(rd) => rd.flatten().collect::<Vec<_>>(),
        Err(_) => return vec![],
    };
    cloud_dirs.sort_by_key(|e| e.file_name());
    for cloud_dir in cloud_dirs {
        let cloud_path = cloud_dir.path();
        if !cloud_path.is_dir() {
            continue;
        }
        let Some(cloud_name) = cloud_path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if cloud_name.starts_with('.') {
            continue;
        }
        let mut mod_dirs = match std::fs::read_dir(&cloud_path) {
            Ok(rd) => rd.flatten().collect::<Vec<_>>(),
            Err(_) => continue,
        };
        mod_dirs.sort_by_key(|e| e.file_name());
        for mod_dir in mod_dirs {
            let mod_path = mod_dir.path();
            if !mod_path.is_dir() {
                continue;
            }
            let Some(mod_name) = mod_path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if mod_name.starts_with('.') {
                continue;
            }
            let module = scan_one_module(root, &cloud_path, &mod_path, cloud_name, mod_name);
            out.push(module);
        }
    }
    out
}

fn scan_one_module(
    repo_root: &Path,
    _cloud_path: &Path,
    mod_path: &Path,
    cloud_name: &str,
    mod_name: &str,
) -> TerraformModule {
    let mut resources: Vec<(String, String)> = Vec::new();
    let mut variables: Vec<String> = Vec::new();
    let mut outputs: Vec<String> = Vec::new();
    for tf in collect_tf_files(mod_path) {
        let Ok(bytes) = std::fs::read(&tf) else {
            continue;
        };
        let Ok(parsed) = dq_formats::parse(dq_formats::FormatKind::Hcl, &bytes) else {
            continue;
        };
        harvest_blocks(&parsed, &mut resources, &mut variables, &mut outputs);
    }
    resources.sort();
    resources.dedup();
    variables.sort();
    variables.dedup();
    outputs.sort();
    outputs.dedup();
    let rel = mod_path.strip_prefix(repo_root).unwrap_or(mod_path).to_path_buf();
    TerraformModule {
        cloud: cloud_name.to_string(),
        name: mod_name.to_string(),
        path: rel,
        resources,
        variables,
        outputs,
    }
}

fn collect_tf_files(dir: &Path) -> Vec<PathBuf> {
    let mut acc: Vec<PathBuf> = Vec::new();
    fn walk(dir: &Path, acc: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if p.file_name().and_then(|s| s.to_str()).map(|s| s.starts_with('.')).unwrap_or(false) {
                    continue;
                }
                walk(&p, acc);
            } else if p.extension().and_then(|s| s.to_str()) == Some("tf") {
                acc.push(p);
            }
        }
    }
    walk(dir, &mut acc);
    acc.sort();
    acc
}

/// Traverse a parsed HCL Value and pull every top-level
/// resource/variable/output block label pair into the accumulators.
fn harvest_blocks(
    value: &Value,
    resources: &mut Vec<(String, String)>,
    variables: &mut Vec<String>,
    outputs: &mut Vec<String>,
) {
    let Value::Map(map) = value else {
        return;
    };
    for (key, inner) in map.iter() {
        // dq-formats now emits every block type as an array of blocks.
        let blocks = match inner {
            Value::Array(items) => items.as_ref().clone(),
            Value::Block(_) => vec![inner.clone()],
            _ => continue,
        };
        for b in blocks {
            let Value::Block(blk) = b else { continue };
            match key.as_ref() {
                "resource" => {
                    if let (Some(ty), Some(name)) = (blk.labels.first(), blk.labels.get(1)) {
                        resources.push((ty.to_string(), name.to_string()));
                    }
                }
                "variable" => {
                    if let Some(name) = blk.labels.first() {
                        variables.push(name.to_string());
                    }
                }
                "output" => {
                    if let Some(name) = blk.labels.first() {
                        outputs.push(name.to_string());
                    }
                }
                _ => {}
            }
        }
    }
}

/// Convert a ``TerraformModule`` to a dq ``Value`` for serialisation.
pub fn module_to_value(m: &TerraformModule) -> Value {
    let mut obj: IndexMap<Arc<str>, Value> = IndexMap::new();
    obj.insert(Arc::from("cloud"), Value::string(&m.cloud));
    obj.insert(Arc::from("name"), Value::string(&m.name));
    obj.insert(
        Arc::from("path"),
        Value::string(m.path.to_string_lossy().as_ref()),
    );
    let resources: Vec<Value> = m
        .resources
        .iter()
        .map(|(t, n)| {
            let mut r: IndexMap<Arc<str>, Value> = IndexMap::new();
            r.insert(Arc::from("type"), Value::string(t));
            r.insert(Arc::from("name"), Value::string(n));
            Value::map(r)
        })
        .collect();
    obj.insert(Arc::from("resources"), Value::array(resources));
    obj.insert(
        Arc::from("variables"),
        Value::array(m.variables.iter().map(Value::string).collect::<Vec<_>>()),
    );
    obj.insert(
        Arc::from("outputs"),
        Value::array(m.outputs.iter().map(Value::string).collect::<Vec<_>>()),
    );
    Value::map(obj)
}

pub fn modules_to_value(modules: &[TerraformModule]) -> Value {
    Value::array(modules.iter().map(module_to_value).collect::<Vec<_>>())
}
