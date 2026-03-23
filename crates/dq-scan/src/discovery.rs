//! Directory walking and file discovery by extension/pattern.
//!
//! Walks a directory tree and classifies discovered files into categories
//! (ApplicationSet, Chart, values, config) based on file names and paths.
//! All pattern matching is driven by [`ScanConfig`].

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config::ScanConfig;

/// A file discovered during directory scanning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredFile {
    /// Absolute path to the file.
    pub path: PathBuf,
    /// Detected format (yaml, json, etc.).
    pub format: FileFormat,
    /// Semantic category based on naming/location.
    pub category: FileCategory,
}

/// Wire format of a discovered file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileFormat {
    Yaml,
    Json,
    Other,
}

/// Semantic category of a discovered file within the GitOps repo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileCategory {
    /// An ArgoCD ApplicationSet YAML.
    ApplicationSet,
    /// A Helm Chart.yaml.
    Chart,
    /// A Helm values file (YAML).
    Values,
    /// A config file (JSON or YAML) in environments/*/argocd/.
    Config,
    /// Something else (not specifically categorized).
    Unknown,
}

/// Trait abstracting filesystem access for testability.
pub trait FileSystem {
    /// Recursively discover all files under `root`.
    fn walk_files(&self, root: &Path) -> std::io::Result<Vec<PathBuf>>;
    /// Read the contents of a file.
    fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>>;
}

/// Real filesystem implementation.
pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn walk_files(&self, root: &Path) -> std::io::Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        walk_recursive(root, &mut files)?;
        Ok(files)
    }

    fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        std::fs::read(path)
    }
}

fn walk_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_recursive(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// Discover files under `root` matching the config patterns, using the provided filesystem.
pub fn discover_files<F: FileSystem>(
    fs: &F,
    root: &Path,
    config: &ScanConfig,
) -> std::io::Result<Vec<DiscoveredFile>> {
    let extensions = config.extension_set();
    let appset_markers = config.appset_marker_set();
    let all_files = fs.walk_files(root)?;
    let mut discovered = Vec::new();

    for path in all_files {
        let format = match path.extension().and_then(|e| e.to_str()) {
            Some("yaml") | Some("yml") => FileFormat::Yaml,
            Some("json") => FileFormat::Json,
            _ => FileFormat::Other,
        };

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !extensions.contains(&ext) {
            continue;
        }

        let category = classify_file(&path, config, &appset_markers);

        discovered.push(DiscoveredFile {
            path,
            format,
            category,
        });
    }

    discovered.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(discovered)
}

/// Classify a file path into a semantic category.
fn classify_file(path: &Path, config: &ScanConfig, appset_markers: &HashSet<String>) -> FileCategory {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    // Chart.yaml detection
    if file_name == config.chart_filename {
        return FileCategory::Chart;
    }

    // Check if the file lives under an ApplicationSet directory marker
    let path_str = path.to_string_lossy();
    for marker in appset_markers {
        if path_str.contains(marker.as_str()) {
            if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("yaml") | Some("yml")
            ) {
                return FileCategory::ApplicationSet;
            }
        }
    }

    // ArgoCD config file detection (uses config's argocd_config_dir)
    let argocd_dir_pattern = format!("/{}/", config.argocd_config_dir);
    if path_str.contains(&argocd_dir_pattern) && file_name == config.argocd_config_filename {
        return FileCategory::Config;
    }

    // Helm values files — check against all known value directory names from config
    for dir_name in config.env_dir_type_map.keys() {
        if config.env_dir_type_map.get(dir_name).map(|t| t.as_str()) == Some("helm_values") {
            if path_str.contains(dir_name.as_str()) {
                return FileCategory::Values;
            }
        }
    }

    FileCategory::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// In-memory filesystem for testing.
    pub struct MemoryFs {
        pub files: HashMap<PathBuf, Vec<u8>>,
    }

    impl MemoryFs {
        pub fn new() -> Self {
            Self {
                files: HashMap::new(),
            }
        }

        pub fn add_file(&mut self, path: impl Into<PathBuf>, content: impl Into<Vec<u8>>) {
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

    #[test]
    fn discover_yaml_files() {
        let mut fs = MemoryFs::new();
        let root = PathBuf::from("/repo");
        fs.add_file("/repo/argocd-app/cluster-generator/web-generator.yaml", b"apiVersion: argoproj.io/v1alpha1".to_vec());
        fs.add_file("/repo/charts/web/Chart.yaml", b"apiVersion: v2".to_vec());
        fs.add_file("/repo/environments/tenant-a/production/aws/helm_values_files/us-east-2/values.yaml", b"key: value".to_vec());
        fs.add_file("/repo/README.md", b"# readme".to_vec());

        let config = ScanConfig::default();
        let files = discover_files(&fs, &root, &config).unwrap();

        assert_eq!(files.len(), 3);

        let appset = files.iter().find(|f| f.category == FileCategory::ApplicationSet);
        assert!(appset.is_some());

        let chart = files.iter().find(|f| f.category == FileCategory::Chart);
        assert!(chart.is_some());

        let values = files.iter().find(|f| f.category == FileCategory::Values);
        assert!(values.is_some());
    }

    #[test]
    fn classify_config_json() {
        let config = ScanConfig::default();
        let markers = config.appset_marker_set();
        let path = Path::new("/repo/environments/tenant-a/production/aws/argocd/us-east-2/config.json");
        assert_eq!(classify_file(path, &config, &markers), FileCategory::Config);
    }

    #[test]
    fn discover_empty_directory() {
        let fs = MemoryFs::new();
        let root = PathBuf::from("/empty");
        let config = ScanConfig::default();
        let files = discover_files(&fs, &root, &config).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn discover_filters_non_matching_extensions() {
        let mut fs = MemoryFs::new();
        let root = PathBuf::from("/repo");
        fs.add_file("/repo/readme.md", b"# readme".to_vec());
        fs.add_file("/repo/script.sh", b"#!/bin/bash".to_vec());
        fs.add_file("/repo/data.txt", b"data".to_vec());
        let config = ScanConfig::default();
        let files = discover_files(&fs, &root, &config).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn discover_with_custom_appset_markers() {
        let mut fs = MemoryFs::new();
        let root = PathBuf::from("/repo");
        fs.add_file("/repo/custom-appsets/web.yaml", b"apiVersion: v1".to_vec());
        let config = ScanConfig {
            appset_dir_markers: vec!["custom-appsets".into()],
            ..Default::default()
        };
        let files = discover_files(&fs, &root, &config).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].category, FileCategory::ApplicationSet);
    }
}
