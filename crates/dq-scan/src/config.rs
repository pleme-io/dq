//! Scan configuration — drives all discovery and classification behavior.
//!
//! Loaded from a `.dq.yaml` or `.dq.json` file at the repository root,
//! or constructed programmatically. Replaces all hardcoded directory names,
//! file type mappings, and pattern markers so that dq-scan is fully generic.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Complete scan configuration for a GitOps repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanConfig {
    /// Name of the environments directory relative to repo root.
    pub environments_dir: String,

    /// Directory markers that indicate ApplicationSet YAML locations.
    /// Files under directories matching these names are classified as ApplicationSets.
    pub appset_dir_markers: Vec<String>,

    /// Base filename for Helm chart metadata (usually "Chart.yaml").
    pub chart_filename: String,

    /// File extensions to include during discovery.
    pub extensions: Vec<String>,

    /// Mapping from directory names inside the environment hierarchy
    /// to semantic file type labels. Keys are directory names (e.g., "helm_values_files"),
    /// values are the label strings (e.g., "helm_values").
    pub env_dir_type_map: HashMap<String, String>,

    /// Directory name within environment hierarchy that contains ArgoCD
    /// cluster config files (typically "argocd").
    pub argocd_config_dir: String,

    /// Filename pattern for ArgoCD cluster config (typically "config.json").
    pub argocd_config_filename: String,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            environments_dir: "environments".to_string(),
            appset_dir_markers: vec![
                "cluster-generator".to_string(),
                "git-generator".to_string(),
            ],
            chart_filename: "Chart.yaml".to_string(),
            extensions: vec![
                "yaml".to_string(),
                "yml".to_string(),
                "json".to_string(),
            ],
            env_dir_type_map: HashMap::from([
                ("helm_values_files".to_string(), "helm_values".to_string()),
                ("argocd".to_string(), "argocd_config".to_string()),
            ]),
            argocd_config_dir: "argocd".to_string(),
            argocd_config_filename: "config.json".to_string(),
        }
    }
}

impl ScanConfig {
    /// Try to load a ScanConfig from `.dq.yaml` or `.dq.json` at the given root.
    /// Falls back to default if no config file is found.
    pub fn load_or_default(root: &Path) -> Self {
        // Try .dq.yaml first
        let yaml_path = root.join(".dq.yaml");
        if yaml_path.is_file() {
            if let Ok(contents) = std::fs::read_to_string(&yaml_path) {
                if let Ok(config) = serde_saphyr::from_str::<ScanConfig>(&contents) {
                    return config;
                }
            }
        }

        // Try .dq.json
        let json_path = root.join(".dq.json");
        if json_path.is_file() {
            if let Ok(contents) = std::fs::read_to_string(&json_path) {
                if let Ok(config) = serde_json::from_str::<ScanConfig>(&contents) {
                    return config;
                }
            }
        }

        Self::default()
    }

    /// Build a set of extensions for pattern matching.
    pub fn extension_set(&self) -> HashSet<String> {
        self.extensions.iter().cloned().collect()
    }

    /// Build a set of appset directory markers.
    pub fn appset_marker_set(&self) -> HashSet<String> {
        self.appset_dir_markers.iter().cloned().collect()
    }

    /// Classify a directory name within the environment hierarchy.
    /// Returns a label from `env_dir_type_map`, or "other" if not mapped.
    pub fn classify_env_dir(&self, dir_name: &str) -> String {
        self.env_dir_type_map
            .get(dir_name)
            .cloned()
            .unwrap_or_else(|| "other".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sensible_values() {
        let config = ScanConfig::default();
        assert_eq!(config.environments_dir, "environments");
        assert_eq!(config.chart_filename, "Chart.yaml");
        assert!(config.extensions.contains(&"yaml".to_string()));
        assert!(config.appset_dir_markers.contains(&"cluster-generator".to_string()));
    }

    #[test]
    fn classify_env_dir_maps_known() {
        let config = ScanConfig::default();
        assert_eq!(config.classify_env_dir("helm_values_files"), "helm_values");
        assert_eq!(config.classify_env_dir("argocd"), "argocd_config");
        assert_eq!(config.classify_env_dir("unknown_dir"), "other");
    }

    #[test]
    fn config_deserializes_from_json() {
        let json = r#"{
            "environments_dir": "envs",
            "appset_dir_markers": ["appsets"],
            "env_dir_type_map": {"values": "helm_values", "secrets": "service_config"}
        }"#;
        let config: ScanConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.environments_dir, "envs");
        assert_eq!(config.appset_dir_markers, vec!["appsets"]);
        assert_eq!(config.classify_env_dir("values"), "helm_values");
        assert_eq!(config.classify_env_dir("secrets"), "service_config");
    }

    #[test]
    fn extension_set_generation() {
        let config = ScanConfig {
            extensions: vec!["yaml".into(), "json".into(), "hcl".into()],
            ..Default::default()
        };
        let set = config.extension_set();
        assert_eq!(set.len(), 3);
        assert!(set.contains("yaml"));
        assert!(set.contains("hcl"));
    }

    #[test]
    fn appset_marker_set_generation() {
        let config = ScanConfig {
            appset_dir_markers: vec!["custom-appsets".into()],
            ..Default::default()
        };
        let set = config.appset_marker_set();
        assert_eq!(set.len(), 1);
        assert!(set.contains("custom-appsets"));
    }

    #[test]
    fn custom_env_dir_type_map() {
        let mut map = std::collections::HashMap::new();
        map.insert("secrets".to_string(), "vault_secrets".to_string());
        map.insert("configs".to_string(), "service_config".to_string());
        let config = ScanConfig {
            env_dir_type_map: map,
            ..Default::default()
        };
        assert_eq!(config.classify_env_dir("secrets"), "vault_secrets");
        assert_eq!(config.classify_env_dir("configs"), "service_config");
        assert_eq!(config.classify_env_dir("other"), "other");
    }
}
