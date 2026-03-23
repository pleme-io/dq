//! Dependency graph builder for Terragrunt configurations.
//!
//! Walks a directory tree, discovers all `terragrunt.hcl` files,
//! parses their dependency blocks, and builds a DAG.
//! Supports DOT export, topological ordering, and path tracing.

use crate::config::TerragruntConfig;
use dq_core::{Error, Value};
use indexmap::IndexMap;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo::toposort;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A node in the dependency graph.
#[derive(Clone, Debug)]
pub struct ModuleNode {
    /// Absolute path to the module directory
    pub path: PathBuf,
    /// Relative path from the project root (for display)
    pub relative_path: String,
    /// Parsed configuration (if available)
    pub config: Option<TerragruntConfig>,
    /// Terraform source (from terraform.source)
    pub source: Option<String>,
}

impl fmt::Display for ModuleNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.relative_path)
    }
}

/// The full dependency graph for a Terragrunt project.
pub struct DependencyGraph {
    pub graph: DiGraph<ModuleNode, &'static str>,
    pub node_map: HashMap<PathBuf, NodeIndex>,
    pub root: PathBuf,
}

impl DependencyGraph {
    /// Discover and build the dependency graph from a root directory.
    pub fn from_directory(root: &Path) -> Result<Self, Error> {
        let root = root.canonicalize()
            .map_err(|e| Error::Parse(format!("canonicalize {}: {e}", root.display())))?;

        let mut graph = DiGraph::new();
        let mut node_map = HashMap::new();

        // Phase 1: Discover all terragrunt.hcl files
        let hcl_files = discover_hcl_files(&root);

        // Phase 2: Parse configs and create nodes
        for hcl_path in &hcl_files {
            let module_dir = hcl_path.parent().unwrap_or(&root).to_path_buf();
            let relative = module_dir.strip_prefix(&root)
                .unwrap_or(&module_dir)
                .to_string_lossy()
                .to_string();

            let config = TerragruntConfig::from_path(hcl_path).ok();
            let source = config.as_ref()
                .and_then(|c| c.terraform.as_ref())
                .and_then(|t| t.source.clone());

            let node = ModuleNode {
                path: module_dir.clone(),
                relative_path: if relative.is_empty() { ".".into() } else { relative },
                config,
                source,
            };

            let idx = graph.add_node(node);
            node_map.insert(module_dir, idx);
        }

        // Phase 3: Add edges from dependency blocks
        let entries: Vec<(PathBuf, Vec<PathBuf>)> = node_map.keys()
            .filter_map(|module_path| {
                let idx = node_map[module_path];
                let node = &graph[idx];
                let deps = node.config.as_ref()?.all_dependency_paths();
                let resolved: Vec<PathBuf> = deps.iter()
                    .filter_map(|dep_path| {
                        let resolved = if dep_path.is_relative() {
                            module_path.join(dep_path)
                        } else {
                            dep_path.clone()
                        };
                        resolved.canonicalize().ok()
                    })
                    .collect();
                Some((module_path.clone(), resolved))
            })
            .collect();

        for (module_path, dep_paths) in entries {
            let from = node_map[&module_path];
            for dep_path in dep_paths {
                if let Some(&to) = node_map.get(&dep_path) {
                    graph.add_edge(from, to, "depends_on");
                }
            }
        }

        Ok(DependencyGraph { graph, node_map, root })
    }

    /// Export the graph in DOT format (for Graphviz visualization).
    pub fn to_dot(&self) -> String {
        // Manual DOT generation for full control over output
        let mut dot = String::from("digraph {\n");
        for idx in self.graph.node_indices() {
            let node = &self.graph[idx];
            dot.push_str(&format!("    \"{}\" [label=\"{}\"];\n", node.relative_path, node.relative_path));
        }
        for edge in self.graph.edge_indices() {
            if let Some((a, b)) = self.graph.edge_endpoints(edge) {
                dot.push_str(&format!(
                    "    \"{}\" -> \"{}\";\n",
                    self.graph[a].relative_path,
                    self.graph[b].relative_path,
                ));
            }
        }
        dot.push_str("}\n");
        dot
    }

    /// Topological sort — returns module paths in dependency order.
    pub fn topo_sort(&self) -> Result<Vec<PathBuf>, Error> {
        toposort(&self.graph, None)
            .map(|indices| {
                indices.iter()
                    .map(|idx| self.graph[*idx].path.clone())
                    .collect()
            })
            .map_err(|cycle| {
                let node = &self.graph[cycle.node_id()];
                Error::Other(format!("dependency cycle detected at: {}", node.relative_path))
            })
    }

    /// Get all direct dependencies of a module.
    pub fn dependencies_of(&self, module_path: &Path) -> Vec<&ModuleNode> {
        let canonical = module_path.canonicalize().unwrap_or_else(|_| module_path.to_path_buf());
        if let Some(&idx) = self.node_map.get(&canonical) {
            self.graph.neighbors(idx)
                .map(|n| &self.graph[n])
                .collect()
        } else {
            vec![]
        }
    }

    /// Get all transitive dependencies (full dependency tree).
    pub fn transitive_dependencies_of(&self, module_path: &Path) -> Vec<&ModuleNode> {
        let canonical = module_path.canonicalize().unwrap_or_else(|_| module_path.to_path_buf());
        let Some(&start) = self.node_map.get(&canonical) else { return vec![] };

        let mut visited = std::collections::HashSet::new();
        let mut result = Vec::new();
        let mut stack = vec![start];

        while let Some(idx) = stack.pop() {
            for neighbor in self.graph.neighbors(idx) {
                if visited.insert(neighbor) {
                    result.push(&self.graph[neighbor]);
                    stack.push(neighbor);
                }
            }
        }

        result
    }

    /// Get all modules that depend on this module (reverse dependencies).
    pub fn dependents_of(&self, module_path: &Path) -> Vec<&ModuleNode> {
        let canonical = module_path.canonicalize().unwrap_or_else(|_| module_path.to_path_buf());
        if let Some(&idx) = self.node_map.get(&canonical) {
            self.graph.neighbors_directed(idx, petgraph::Direction::Incoming)
                .map(|n| &self.graph[n])
                .collect()
        } else {
            vec![]
        }
    }

    /// Total number of modules.
    pub fn module_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Total number of dependency edges.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Convert the graph to a dq Value for serialization/querying.
    pub fn to_value(&self) -> Value {
        let nodes: Vec<Value> = self.graph.node_indices().map(|idx| {
            let node = &self.graph[idx];
            let deps: Vec<Value> = self.graph.neighbors(idx)
                .map(|n| Value::string(self.graph[n].relative_path.as_str()))
                .collect();
            let dependents: Vec<Value> = self.graph.neighbors_directed(idx, petgraph::Direction::Incoming)
                .map(|n| Value::string(self.graph[n].relative_path.as_str()))
                .collect();

            let mut map = IndexMap::new();
            map.insert(Arc::from("path"), Value::string(node.relative_path.as_str()));
            if let Some(ref src) = node.source {
                map.insert(Arc::from("source"), Value::string(src.as_str()));
            }
            map.insert(Arc::from("dependencies"), Value::array(deps));
            map.insert(Arc::from("dependents"), Value::array(dependents));
            Value::map(map)
        }).collect();

        let mut root_map = IndexMap::new();
        root_map.insert(Arc::from("root"), Value::string(self.root.to_string_lossy().as_ref()));
        root_map.insert(Arc::from("module_count"), Value::int(self.module_count() as i64));
        root_map.insert(Arc::from("edge_count"), Value::int(self.edge_count() as i64));
        root_map.insert(Arc::from("modules"), Value::array(nodes));
        Value::map(root_map)
    }
}

/// Recursively discover all terragrunt.hcl files under a directory.
fn discover_hcl_files(root: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    discover_inner(root, &mut result);
    result
}

fn discover_inner(dir: &Path, result: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.starts_with('.') && name != "node_modules" {
                discover_inner(&path, result);
            }
        } else if path.file_name().map(|n| n == "terragrunt.hcl").unwrap_or(false) {
            result.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Create a terragrunt.hcl file in the given directory.
    fn write_hcl(dir: &Path, content: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join("terragrunt.hcl");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    // ── dag.rs tests (7) ──────────────────────────────────────────

    #[test]
    fn empty_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let graph = DependencyGraph::from_directory(tmp.path()).unwrap();
        assert_eq!(graph.module_count(), 0);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn single_module() {
        let tmp = tempfile::tempdir().unwrap();
        write_hcl(tmp.path(), r#"
terraform {
  source = "tfr:///modules/vpc?version=1.0"
}
"#);
        let graph = DependencyGraph::from_directory(tmp.path()).unwrap();
        assert_eq!(graph.module_count(), 1);
        assert_eq!(graph.edge_count(), 0);
    }

    #[test]
    fn linear_dependency_chain() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // A depends on B, B depends on C
        let a_dir = root.join("a");
        let b_dir = root.join("b");
        let c_dir = root.join("c");

        write_hcl(&c_dir, "terraform { source = \".\" }");
        write_hcl(&b_dir, &format!(r#"
dependency "c" {{
  config_path = "../c"
}}
terraform {{ source = "." }}
"#));
        write_hcl(&a_dir, &format!(r#"
dependency "b" {{
  config_path = "../b"
}}
terraform {{ source = "." }}
"#));

        let graph = DependencyGraph::from_directory(root).unwrap();
        assert_eq!(graph.module_count(), 3);
        assert_eq!(graph.edge_count(), 2);
    }

    #[test]
    fn diamond_dependencies() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        //    top
        //   /   \
        //  left  right
        //   \   /
        //   bottom
        let top = root.join("top");
        let left = root.join("left");
        let right = root.join("right");
        let bottom = root.join("bottom");

        write_hcl(&bottom, "terraform { source = \".\" }");
        write_hcl(&left, r#"
dependency "bottom" {
  config_path = "../bottom"
}
terraform { source = "." }
"#);
        write_hcl(&right, r#"
dependency "bottom" {
  config_path = "../bottom"
}
terraform { source = "." }
"#);
        write_hcl(&top, r#"
dependency "left" {
  config_path = "../left"
}
dependency "right" {
  config_path = "../right"
}
terraform { source = "." }
"#);

        let graph = DependencyGraph::from_directory(root).unwrap();
        assert_eq!(graph.module_count(), 4);
        assert_eq!(graph.edge_count(), 4);
    }

    #[test]
    fn topo_sort_ordering() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let a_dir = root.join("a");
        let b_dir = root.join("b");

        write_hcl(&b_dir, "terraform { source = \".\" }");
        write_hcl(&a_dir, r#"
dependency "b" {
  config_path = "../b"
}
terraform { source = "." }
"#);

        let graph = DependencyGraph::from_directory(root).unwrap();
        let sorted = graph.topo_sort().unwrap();

        // In topological order, A must come before B (A depends on B,
        // so A is processed first in petgraph's toposort since edges
        // point from dependent to dependency).
        let a_canon = a_dir.canonicalize().unwrap();
        let b_canon = b_dir.canonicalize().unwrap();

        let a_pos = sorted.iter().position(|p| *p == a_canon).unwrap();
        let b_pos = sorted.iter().position(|p| *p == b_canon).unwrap();

        // petgraph toposort: if A->B edge (A depends on B), then A comes before B in topo order
        assert!(a_pos < b_pos, "A (depends on B) should come before B in topo sort");
    }

    #[test]
    fn dot_output_format() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let a_dir = root.join("a");
        let b_dir = root.join("b");

        write_hcl(&b_dir, "terraform { source = \".\" }");
        write_hcl(&a_dir, r#"
dependency "b" {
  config_path = "../b"
}
terraform { source = "." }
"#);

        let graph = DependencyGraph::from_directory(root).unwrap();
        let dot = graph.to_dot();

        assert!(dot.starts_with("digraph {"));
        assert!(dot.ends_with("}\n"));
        assert!(dot.contains("->"));
        assert!(dot.contains("[label="));
    }

    #[test]
    fn value_export_structure() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        write_hcl(root, "terraform { source = \".\" }");

        let graph = DependencyGraph::from_directory(root).unwrap();
        let value = graph.to_value();

        assert!(value.get("root").is_some());
        assert_eq!(value.get("module_count").and_then(|v| v.as_i64()), Some(1));
        assert_eq!(value.get("edge_count").and_then(|v| v.as_i64()), Some(0));
        let modules = value.get("modules").and_then(|v| v.as_array()).unwrap();
        assert_eq!(modules.len(), 1);
        let module = &modules[0];
        assert!(module.get("path").is_some());
        assert!(module.get("dependencies").is_some());
        assert!(module.get("dependents").is_some());
    }
}
