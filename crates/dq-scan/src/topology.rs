//! Topology graph construction.
//!
//! Assembles all discovered ApplicationSets, Charts, ConfigPaths, and
//! environment taxonomy into a single topology graph with typed edges
//! representing the relationships between them.

use std::path::Path;

use crate::argocd::{self, AppSetInfo};
use crate::config::ScanConfig;
use crate::discovery::{
    DiscoveredFile, FileCategory, FileSystem, RealFileSystem,
};
use crate::environments::{self, ConfigPath, Taxonomy};
use crate::helm::{self, ChartInfo};
use crate::ScanError;

/// A typed edge in the topology graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edge {
    /// An ApplicationSet references a Helm chart by path.
    AppSetToChart {
        /// ApplicationSet name.
        appset: String,
        /// Chart name.
        chart: String,
    },
    /// An ApplicationSet references a value file template.
    AppSetToValueFile {
        /// ApplicationSet name.
        appset: String,
        /// Value file template path (may contain Go template expressions).
        path: String,
    },
    /// A Helm chart depends on another chart (from Chart.yaml dependencies).
    ChartDependency {
        /// Parent chart name.
        parent: String,
        /// Child (dependency) chart name.
        child: String,
    },
}

/// The complete topology of a scanned GitOps repository.
#[derive(Debug, Clone)]
pub struct Topology {
    /// All parsed ApplicationSets.
    pub appsets: Vec<AppSetInfo>,
    /// All parsed Helm charts.
    pub charts: Vec<ChartInfo>,
    /// All discovered environment config paths.
    pub config_paths: Vec<ConfigPath>,
    /// Typed edges connecting the above entities.
    pub edges: Vec<Edge>,
    /// Aggregated taxonomy of environments.
    pub taxonomy: Taxonomy,
}

/// Wrapper around Topology for the public API.
#[derive(Debug, Clone)]
pub struct ScanResult {
    pub topology: Topology,
}

/// Build the full topology from a repository root directory.
///
/// Orchestrates: discovery -> parsing -> edge building -> taxonomy.
pub fn build_topology(root: &Path, config: &ScanConfig) -> Result<ScanResult, ScanError> {
    let fs = RealFileSystem;
    build_topology_with_fs(&fs, root, config)
}

/// Build topology using a provided filesystem (for testability).
pub fn build_topology_with_fs<F: FileSystem>(
    fs: &F,
    root: &Path,
    config: &ScanConfig,
) -> Result<ScanResult, ScanError> {
    // Phase 1: Discover files
    let discovered = crate::discovery::discover_files(fs, root, config)?;

    // Phase 2: Parse ApplicationSets
    let appsets = parse_appsets(fs, &discovered)?;

    // Phase 3: Parse Charts
    let charts = parse_charts(fs, root, &discovered)?;

    // Phase 4: Walk environments (directory name from config)
    let env_root = root.join(&config.environments_dir);
    let config_paths = if env_root.is_dir() || cfg!(test) {
        environments::walk_environments(fs, &env_root, config).unwrap_or_default()
    } else {
        Vec::new()
    };

    // Phase 5: Build taxonomy
    let taxonomy = environments::build_taxonomy(&config_paths);

    // Phase 6: Build edges
    let edges = build_edges(&appsets, &charts);

    Ok(ScanResult {
        topology: Topology {
            appsets,
            charts,
            config_paths,
            edges,
            taxonomy,
        },
    })
}

/// Parse all ApplicationSet files from the discovered file list.
fn parse_appsets<F: FileSystem>(
    fs: &F,
    discovered: &[DiscoveredFile],
) -> Result<Vec<AppSetInfo>, ScanError> {
    let mut appsets = Vec::new();

    for file in discovered {
        if file.category != FileCategory::ApplicationSet {
            continue;
        }

        let content = fs.read_file(&file.path)?;
        let value = dq_formats::parse(dq_formats::FormatKind::Yaml, &content).map_err(|e| {
            ScanError::YamlParse {
                path: file.path.to_string_lossy().to_string(),
                message: e.to_string(),
            }
        })?;

        let source = file
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        match argocd::parse_appset(&value, &source) {
            Ok(info) => appsets.push(info),
            Err(e) => {
                // Log but don't fail on individual parse errors -- some files
                // in ApplicationSet directories might not be valid ApplicationSets
                // (e.g., README.md already filtered, but partial YAMLs can exist).
                eprintln!(
                    "warning: skipping {}: {e}",
                    file.path.to_string_lossy()
                );
            }
        }
    }

    Ok(appsets)
}

/// Parse all Chart.yaml files from the discovered file list.
fn parse_charts<F: FileSystem>(
    fs: &F,
    root: &Path,
    discovered: &[DiscoveredFile],
) -> Result<Vec<ChartInfo>, ScanError> {
    let mut charts = Vec::new();

    for file in discovered {
        if file.category != FileCategory::Chart {
            continue;
        }

        let content = fs.read_file(&file.path)?;
        let value = dq_formats::parse(dq_formats::FormatKind::Yaml, &content).map_err(|e| {
            ScanError::YamlParse {
                path: file.path.to_string_lossy().to_string(),
                message: e.to_string(),
            }
        })?;

        // Chart directory is the parent of Chart.yaml, relative to root
        let chart_dir = file
            .path
            .parent()
            .and_then(|p| p.strip_prefix(root).ok())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        match helm::parse_chart(&value, &chart_dir) {
            Ok(info) => charts.push(info),
            Err(e) => {
                eprintln!(
                    "warning: skipping {}: {e}",
                    file.path.to_string_lossy()
                );
            }
        }
    }

    Ok(charts)
}

/// Build edges between AppSets, Charts, and value files.
fn build_edges(appsets: &[AppSetInfo], charts: &[ChartInfo]) -> Vec<Edge> {
    let mut edges = Vec::new();

    for appset in appsets {
        // AppSet -> Chart edge: match the appset's chart_path to a chart's chart_dir
        if let Some(ref chart_path) = appset.chart_path {
            for chart in charts {
                if chart_path_matches(chart_path, &chart.chart_dir) {
                    edges.push(Edge::AppSetToChart {
                        appset: appset.name.clone(),
                        chart: chart.name.clone(),
                    });
                }
            }
        }

        // AppSet -> ValueFile edges
        for vf in &appset.value_files {
            edges.push(Edge::AppSetToValueFile {
                appset: appset.name.clone(),
                path: vf.clone(),
            });
        }
    }

    // Chart -> Chart dependency edges
    for chart in charts {
        for dep in &chart.dependencies {
            edges.push(Edge::ChartDependency {
                parent: chart.name.clone(),
                child: dep.name.clone(),
            });
        }
    }

    edges
}

/// Check if an ApplicationSet's chart path matches a chart's directory.
///
/// The chart_path in an ApplicationSet is relative to the repo root
/// (e.g., "saas/kubernetes/helm/saas"), and the chart_dir from Chart.yaml
/// is also relative to root. We normalize both and compare.
fn chart_path_matches(appset_chart_path: &str, chart_dir: &str) -> bool {
    let normalize = |s: &str| {
        s.trim_start_matches('/')
            .trim_end_matches('/')
            .to_string()
    };
    normalize(appset_chart_path) == normalize(chart_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::FileSystem;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// In-memory filesystem for testing.
    struct MemoryFs {
        files: HashMap<PathBuf, Vec<u8>>,
    }

    impl MemoryFs {
        fn new() -> Self {
            Self {
                files: HashMap::new(),
            }
        }

        fn add_file(&mut self, path: impl Into<PathBuf>, content: impl Into<Vec<u8>>) {
            self.files.insert(path.into(), content.into());
        }
    }

    impl FileSystem for MemoryFs {
        fn walk_files(&self, root: &Path) -> std::io::Result<Vec<PathBuf>> {
            let root_str = root.to_string_lossy().to_string();
            let mut files: Vec<PathBuf> = self
                .files
                .keys()
                .filter(|p| p.to_string_lossy().starts_with(&root_str))
                .cloned()
                .collect();
            files.sort();
            Ok(files)
        }

        fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "not found"))
        }
    }

    fn sample_appset_yaml() -> &'static str {
        r#"apiVersion: argoproj.io/v1alpha1
kind: ApplicationSet
metadata:
  name: web-app-v2
  namespace: argocd
spec:
  generators:
    - clusters:
        selector:
          matchExpressions:
            - key: web-app
              operator: Exists
            - key: tenant
              operator: NotIn
              values: ["excluded-tenant"]
  template:
    metadata:
      name: test
    spec:
      source:
        repoURL: git@github.com:example-org/example-environments.git
        path: charts/web-app
        helm:
          parameters:
            - name: tenant
              value: "my-tenant"
          valueFiles:
            - '../../../../environments/{{tenant}}/{{environment}}/{{cloudProvider}}/helm_values_files/{{region}}/web-app-values.yaml'
"#
    }

    fn sample_chart_yaml() -> &'static str {
        r#"apiVersion: v2
name: web-app
description: Web application chart.
type: application
version: 0.1.0
"#
    }

    fn sample_chart_with_deps_yaml() -> &'static str {
        r#"apiVersion: v2
name: monitoring
version: "1.0.0"
description: Monitoring chart
dependencies:
  - name: prometheus
    version: "25.0.0"
    repository: https://prometheus-community.github.io/helm-charts
"#
    }

    fn test_config() -> ScanConfig {
        ScanConfig::default()
    }

    #[test]
    fn build_topology_edges() {
        let mut fs = MemoryFs::new();
        let root = PathBuf::from("/repo");
        let config = test_config();

        fs.add_file(
            "/repo/argocd-app/cluster-generator/web-generator.yaml",
            sample_appset_yaml().as_bytes().to_vec(),
        );
        fs.add_file(
            "/repo/charts/web-app/Chart.yaml",
            sample_chart_yaml().as_bytes().to_vec(),
        );
        fs.add_file(
            "/repo/charts/monitoring/Chart.yaml",
            sample_chart_with_deps_yaml().as_bytes().to_vec(),
        );
        fs.add_file(
            "/repo/environments/tenant-a/production/AWS/helm_values_files/us-east-2/web-app-values.yaml",
            b"key: value".to_vec(),
        );

        let result = build_topology_with_fs(&fs, &root, &config).unwrap();
        let topo = &result.topology;

        assert_eq!(topo.appsets.len(), 1);
        assert_eq!(topo.appsets[0].name, "web-app-v2");

        assert_eq!(topo.charts.len(), 2);

        let appset_to_chart = topo.edges.iter()
            .filter(|e| matches!(e, Edge::AppSetToChart { .. }))
            .count();
        assert_eq!(appset_to_chart, 1);

        let appset_to_vf = topo.edges.iter()
            .filter(|e| matches!(e, Edge::AppSetToValueFile { .. }))
            .count();
        assert_eq!(appset_to_vf, 1);

        let chart_dep = topo.edges.iter()
            .filter(|e| matches!(e, Edge::ChartDependency { .. }))
            .count();
        assert_eq!(chart_dep, 1);

        let edge = topo.edges.iter()
            .find(|e| matches!(e, Edge::AppSetToChart { .. }))
            .unwrap();
        match edge {
            Edge::AppSetToChart { appset, chart } => {
                assert_eq!(appset, "web-app-v2");
                assert_eq!(chart, "web-app");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn chart_path_matching() {
        assert!(chart_path_matches("charts/web-app", "charts/web-app"));
        assert!(chart_path_matches("charts/web-app/", "charts/web-app"));
        assert!(!chart_path_matches("charts/web-app", "charts/monitoring"));
    }

    #[test]
    fn value_file_resolution() {
        let mut fs = MemoryFs::new();
        let root = PathBuf::from("/repo");
        let config = test_config();

        let appset1_yaml = r#"apiVersion: argoproj.io/v1alpha1
kind: ApplicationSet
metadata:
  name: web-app-v2
spec:
  generators:
    - clusters:
        selector:
          matchExpressions:
            - key: web-app
              operator: Exists
  template:
    metadata:
      name: test
    spec:
      source:
        path: charts/web-app
        helm:
          valueFiles:
            - '../../../../environments/{{tenant}}/{{environment}}/{{cloudProvider}}/helm_values_files/{{region}}/web-app-values.yaml'
            - '../../../../environments/{{tenant}}/{{environment}}/{{cloudProvider}}/helm_values_files/{{region}}/configmap-sha-values.yaml'
"#;

        fs.add_file(
            "/repo/argocd-app/cluster-generator/web-generator.yaml",
            appset1_yaml.as_bytes().to_vec(),
        );
        fs.add_file(
            "/repo/charts/web-app/Chart.yaml",
            sample_chart_yaml().as_bytes().to_vec(),
        );
        fs.add_file(
            "/repo/environments/tenant-a/production/AWS/helm_values_files/us-east-2/web-app-values.yaml",
            b"key: value".to_vec(),
        );
        fs.add_file(
            "/repo/environments/tenant-a/production/AWS/helm_values_files/us-east-2/configmap-sha-values.yaml",
            b"sha: abc123".to_vec(),
        );

        let result = build_topology_with_fs(&fs, &root, &config).unwrap();
        let topo = &result.topology;

        let vf_edges: Vec<_> = topo.edges.iter()
            .filter(|e| matches!(e, Edge::AppSetToValueFile { .. }))
            .collect();
        assert_eq!(vf_edges.len(), 2);

        for edge in &vf_edges {
            match edge {
                Edge::AppSetToValueFile { appset, .. } => {
                    assert_eq!(appset, "web-app-v2");
                }
                _ => unreachable!(),
            }
        }

        let paths: Vec<_> = vf_edges.iter()
            .map(|e| match e {
                Edge::AppSetToValueFile { path, .. } => path.as_str(),
                _ => unreachable!(),
            })
            .collect();
        assert!(paths.iter().any(|p| p.contains("{{tenant}}")));
        assert!(paths.iter().any(|p| p.contains("web-app-values")));
        assert!(paths.iter().any(|p| p.contains("configmap-sha")));
    }
}
