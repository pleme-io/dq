//! Environment directory hierarchy walking.
//!
//! Parses a configurable environment directory structure to extract the taxonomy
//! of tenants, environments, cloud providers, and regions.
//!
//! The expected layout is:
//! ```text
//! {environments_dir}/
//!   {tenant}/
//!     {environment}/
//!       {cloudProvider}/
//!         {config_subdir}/
//!           {region}/
//!             *.yaml
//! ```
//!
//! Directory names for config subdirs and their semantic types are driven
//! by [`ScanConfig::env_dir_type_map`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::config::ScanConfig;
use crate::discovery::FileSystem;

/// A single configuration file with its parsed location in the hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPath {
    /// Tenant name.
    pub tenant: String,
    /// Environment name (e.g., "production", "staging").
    pub environment: String,
    /// Cloud provider code (e.g., "AWS", "AZR", "GCP").
    pub cloud_provider: String,
    /// Region within the cloud provider.
    pub region: Option<String>,
    /// Semantic file type label (from config's `env_dir_type_map`).
    pub file_type: String,
    /// Absolute path to the file.
    pub path: PathBuf,
}

/// Aggregated taxonomy of the environments directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taxonomy {
    /// All tenant names found.
    pub tenants: BTreeSet<String>,
    /// All environment names found (across all tenants).
    pub environments: BTreeSet<String>,
    /// All cloud provider codes found.
    pub cloud_providers: BTreeSet<String>,
    /// Regions grouped by cloud provider.
    pub regions: BTreeMap<String, BTreeSet<String>>,
}

/// Walk the environments directory and extract ConfigPaths.
pub fn walk_environments<F: FileSystem>(
    fs: &F,
    environments_root: &Path,
    config: &ScanConfig,
) -> std::io::Result<Vec<ConfigPath>> {
    let all_files = fs.walk_files(environments_root)?;

    let mut configs = Vec::new();

    for file_path in &all_files {
        let rel = match file_path.strip_prefix(environments_root) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let components: Vec<&str> = rel
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect();

        // Need at least tenant/environment/cloudProvider/filename
        if components.len() < 4 {
            continue;
        }

        let tenant = components[0].to_string();
        let environment = components[1].to_string();
        let cloud_provider = components[2].to_string();

        // Skip non-directory tenant entries (scripts, etc.)
        if tenant.contains('.') {
            continue;
        }

        let (file_type, region) = classify_env_file(&components[3..], config);

        configs.push(ConfigPath {
            tenant,
            environment,
            cloud_provider,
            region,
            file_type,
            path: file_path.clone(),
        });
    }

    configs.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(configs)
}

/// Classify a file within the cloud provider subtree and extract region.
fn classify_env_file(remaining: &[&str], config: &ScanConfig) -> (String, Option<String>) {
    if remaining.is_empty() {
        return ("other".to_string(), None);
    }

    let dir_name = remaining[0];
    let file_type = config.classify_env_dir(dir_name);

    let region = if remaining.len() >= 3 {
        Some(remaining[1].to_string())
    } else {
        None
    };

    (file_type, region)
}

/// Build a Taxonomy from a set of ConfigPaths.
pub fn build_taxonomy(paths: &[ConfigPath]) -> Taxonomy {
    let mut tenants = BTreeSet::new();
    let mut environments = BTreeSet::new();
    let mut cloud_providers = BTreeSet::new();
    let mut regions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for p in paths {
        tenants.insert(p.tenant.clone());
        environments.insert(p.environment.clone());
        cloud_providers.insert(p.cloud_provider.clone());
        if let Some(ref region) = p.region {
            regions
                .entry(p.cloud_provider.clone())
                .or_default()
                .insert(region.clone());
        }
    }

    Taxonomy {
        tenants,
        environments,
        cloud_providers,
        regions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::FileSystem;
    use std::collections::HashMap;

    struct MemoryFs {
        files: HashMap<PathBuf, Vec<u8>>,
    }

    impl MemoryFs {
        fn new() -> Self {
            Self { files: HashMap::new() }
        }

        fn add_file(&mut self, path: impl Into<PathBuf>) {
            self.files.insert(path.into(), vec![]);
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
    fn walk_environments_hierarchy() {
        let mut fs = MemoryFs::new();
        let root = PathBuf::from("/repo/environments");
        let config = ScanConfig::default();

        fs.add_file("/repo/environments/tenant-a/production/AWS/helm_values_files/us-east-2/values.yaml");
        fs.add_file("/repo/environments/tenant-a/staging/GCP/helm_values_files/us-central1/values.yaml");
        fs.add_file("/repo/environments/tenant-b/production/AWS/argocd/us-east-2/config.json");
        fs.add_file("/repo/environments/tenant-c/staging/AZR/service_config/westeurope/conf.yaml");

        let configs = walk_environments(&fs, &root, &config).unwrap();
        assert_eq!(configs.len(), 4);

        let first = configs.iter().find(|c| c.tenant == "tenant-a" && c.environment == "production").unwrap();
        assert_eq!(first.cloud_provider, "AWS");
        assert_eq!(first.region.as_deref(), Some("us-east-2"));
        assert_eq!(first.file_type, "helm_values");

        let argocd = configs.iter().find(|c| c.file_type == "argocd_config").unwrap();
        assert_eq!(argocd.tenant, "tenant-b");
        assert_eq!(argocd.region.as_deref(), Some("us-east-2"));
    }

    #[test]
    fn build_taxonomy_counts() {
        let mut fs = MemoryFs::new();
        let root = PathBuf::from("/repo/environments");
        let config = ScanConfig::default();

        fs.add_file("/repo/environments/tenant-a/production/AWS/helm_values_files/us-east-2/values.yaml");
        fs.add_file("/repo/environments/tenant-a/staging/GCP/helm_values_files/us-central1/values.yaml");
        fs.add_file("/repo/environments/tenant-b/production/AWS/helm_values_files/us-east-2/values.yaml");
        fs.add_file("/repo/environments/tenant-c/staging/AZR/helm_values_files/westeurope/values.yaml");

        let configs = walk_environments(&fs, &root, &config).unwrap();
        let taxonomy = build_taxonomy(&configs);

        assert_eq!(taxonomy.tenants.len(), 3);
        assert_eq!(taxonomy.environments.len(), 2);
        assert_eq!(taxonomy.cloud_providers.len(), 3);
        assert_eq!(taxonomy.regions.get("AWS").unwrap().len(), 1);
    }

    #[test]
    fn skips_script_files_at_tenant_level() {
        let mut fs = MemoryFs::new();
        let root = PathBuf::from("/repo/environments");
        let config = ScanConfig::default();

        fs.add_file("/repo/environments/generate_configuration_randoms.sh");
        fs.add_file("/repo/environments/tenant-a/production/AWS/helm_values_files/us-east-2/values.yaml");

        let configs = walk_environments(&fs, &root, &config).unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].tenant, "tenant-a");
    }

    #[test]
    fn classify_env_file_no_remaining() {
        let config = ScanConfig::default();
        let (ft, region) = classify_env_file(&[], &config);
        assert_eq!(ft, "other");
        assert!(region.is_none());
    }

    #[test]
    fn classify_env_file_with_region() {
        let config = ScanConfig::default();
        let (ft, region) = classify_env_file(&["helm_values_files", "us-east-1", "values.yaml"], &config);
        assert_eq!(ft, "helm_values");
        assert_eq!(region.as_deref(), Some("us-east-1"));
    }

    #[test]
    fn build_taxonomy_empty_input() {
        let taxonomy = build_taxonomy(&[]);
        assert!(taxonomy.tenants.is_empty());
        assert!(taxonomy.environments.is_empty());
        assert!(taxonomy.cloud_providers.is_empty());
        assert!(taxonomy.regions.is_empty());
    }
}
