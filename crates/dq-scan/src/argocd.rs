//! ArgoCD ApplicationSet YAML parsing.
//!
//! Extracts structured metadata from ArgoCD ApplicationSet definitions,
//! including generator type, cluster selectors, chart paths, value files,
//! Helm parameters, and excluded tenants.

use dq_core::Value;

use crate::ScanError;

/// Parsed metadata from a single ArgoCD ApplicationSet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSetInfo {
    /// The ApplicationSet name from metadata.name.
    pub name: String,
    /// The type of generator(s) used.
    pub generator_type: GeneratorType,
    /// Cluster selector matchExpressions (only for Cluster and Matrix generators).
    pub cluster_selectors: Vec<MatchExpression>,
    /// The Helm chart path from spec.template.spec.source.path.
    pub chart_path: Option<String>,
    /// Value file template strings from spec.template.spec.source.helm.valueFiles.
    pub value_files: Vec<String>,
    /// Helm parameters from spec.template.spec.source.helm.parameters.
    pub helm_parameters: Vec<HelmParameter>,
    /// Tenants excluded via NotIn expressions on the "tenant" key.
    pub excluded_tenants: Vec<String>,
    /// Git file paths (only for Git generators).
    pub git_file_paths: Vec<String>,
    /// The source file this was parsed from.
    pub source_file: String,
}

/// The type of generator used in the ApplicationSet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GeneratorType {
    Cluster,
    Git,
    Matrix,
    List,
    Unknown,
}

impl std::fmt::Display for GeneratorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeneratorType::Cluster => write!(f, "cluster"),
            GeneratorType::Git => write!(f, "git"),
            GeneratorType::Matrix => write!(f, "matrix"),
            GeneratorType::List => write!(f, "list"),
            GeneratorType::Unknown => write!(f, "unknown"),
        }
    }
}

/// A Kubernetes label selector matchExpression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchExpression {
    pub key: String,
    pub operator: String,
    pub values: Vec<String>,
}

/// A Helm parameter (name/value pair).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelmParameter {
    pub name: String,
    pub value: String,
}

/// Parse an ArgoCD ApplicationSet from a dq-core Value (parsed YAML).
///
/// The Value should represent the top-level ApplicationSet document with
/// `apiVersion`, `kind`, `metadata`, and `spec` fields.
pub fn parse_appset(value: &Value, source_file: &str) -> Result<AppSetInfo, ScanError> {
    let name = value
        .select("metadata.name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let generators = value
        .select("spec.generators")
        .and_then(|v| v.as_array())
        .unwrap_or(&[]);

    let (generator_type, cluster_selectors, git_file_paths) =
        parse_generators(generators);

    // Extract excluded tenants from cluster selectors
    let excluded_tenants = extract_excluded_tenants(&cluster_selectors);

    // Extract template source information
    let chart_path = value
        .select("spec.template.spec.source.path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let value_files = extract_value_files(value);
    let helm_parameters = extract_helm_parameters(value);

    Ok(AppSetInfo {
        name,
        generator_type,
        cluster_selectors,
        chart_path,
        value_files,
        helm_parameters,
        excluded_tenants,
        git_file_paths,
        source_file: source_file.to_string(),
    })
}

/// Parse generator entries to determine type, selectors, and git paths.
fn parse_generators(generators: &[Value]) -> (GeneratorType, Vec<MatchExpression>, Vec<String>) {
    if generators.is_empty() {
        return (GeneratorType::Unknown, vec![], vec![]);
    }

    let first = &generators[0];

    // Check for matrix generator
    if let Some(matrix) = first.get("matrix") {
        let nested_generators = matrix
            .select("generators")
            .and_then(|v| v.as_array())
            .unwrap_or(&[]);

        let mut all_selectors = Vec::new();
        let mut all_git_paths = Vec::new();

        for gen in nested_generators {
            if let Some(clusters) = gen.get("clusters") {
                all_selectors.extend(extract_match_expressions(clusters));
            }
            if let Some(git) = gen.get("git") {
                all_git_paths.extend(extract_git_file_paths(git));
            }
        }

        return (GeneratorType::Matrix, all_selectors, all_git_paths);
    }

    // Check for cluster generator
    if let Some(clusters) = first.get("clusters") {
        let selectors = extract_match_expressions(clusters);
        return (GeneratorType::Cluster, selectors, vec![]);
    }

    // Check for git generator
    if let Some(git) = first.get("git") {
        let git_paths = extract_git_file_paths(git);
        return (GeneratorType::Git, vec![], git_paths);
    }

    // Check for list generator
    if first.get("list").is_some() {
        return (GeneratorType::List, vec![], vec![]);
    }

    (GeneratorType::Unknown, vec![], vec![])
}

/// Extract matchExpressions from a clusters generator value.
fn extract_match_expressions(clusters: &Value) -> Vec<MatchExpression> {
    let expressions = clusters
        .select("selector.matchExpressions")
        .and_then(|v| v.as_array())
        .unwrap_or(&[]);

    expressions
        .iter()
        .filter_map(|expr| {
            let key = expr.get("key")?.as_str()?.to_string();
            let operator = expr.get("operator")?.as_str()?.to_string();
            let values = expr
                .get("values")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            Some(MatchExpression {
                key,
                operator,
                values,
            })
        })
        .collect()
}

/// Extract file paths from a git generator value.
fn extract_git_file_paths(git: &Value) -> Vec<String> {
    git.get("files")
        .and_then(|v| v.as_array())
        .unwrap_or(&[])
        .iter()
        .filter_map(|entry| {
            entry
                .get("path")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect()
}

/// Extract tenants excluded via NotIn operator on the "tenant" key.
fn extract_excluded_tenants(selectors: &[MatchExpression]) -> Vec<String> {
    selectors
        .iter()
        .filter(|expr| expr.key == "tenant" && expr.operator == "NotIn")
        .flat_map(|expr| expr.values.clone())
        .collect()
}

/// Extract Helm valueFiles from the template spec.
fn extract_value_files(appset: &Value) -> Vec<String> {
    appset
        .select("spec.template.spec.source.helm.valueFiles")
        .and_then(|v| v.as_array())
        .unwrap_or(&[])
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect()
}

/// Extract Helm parameters from the template spec.
fn extract_helm_parameters(appset: &Value) -> Vec<HelmParameter> {
    appset
        .select("spec.template.spec.source.helm.parameters")
        .and_then(|v| v.as_array())
        .unwrap_or(&[])
        .iter()
        .filter_map(|param| {
            let name = param.get("name")?.as_str()?.to_string();
            let value = param.get("value")?.as_str()?.to_string();
            Some(HelmParameter { name, value })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq_formats::FormatKind;

    fn parse_yaml(input: &str) -> Value {
        dq_formats::parse(FormatKind::Yaml, input.as_bytes()).expect("YAML parse failed")
    }

    #[test]
    fn parse_cluster_generator_appset() {
        let yaml = r#"
apiVersion: argoproj.io/v1alpha1
kind: ApplicationSet
metadata:
  name: web-app-v2
  namespace: argocd
spec:
  goTemplate: true
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
      name: 'test'
    spec:
      source:
        repoURL: git@github.com:example-org/example-environments.git
        targetRevision: master
        path: charts/web-app
        helm:
          releaseName: test
          parameters:
            - name: tenant
              value: "my-tenant"
          valueFiles:
            - '../../../../environments/tenant/production/AWS/helm_values_files/us-east-2/web-app-values.yaml'
            - '../../../../environments/tenant/production/AWS/helm_values_files/image-values.yaml'
"#;
        let value = parse_yaml(yaml);
        let info = parse_appset(&value, "web-generator.yaml").unwrap();

        assert_eq!(info.name, "web-app-v2");
        assert_eq!(info.generator_type, GeneratorType::Cluster);
        assert_eq!(info.cluster_selectors.len(), 2);

        assert_eq!(info.cluster_selectors[0].key, "web-app");
        assert_eq!(info.cluster_selectors[0].operator, "Exists");
        assert!(info.cluster_selectors[0].values.is_empty());

        assert_eq!(info.cluster_selectors[1].key, "tenant");
        assert_eq!(info.cluster_selectors[1].operator, "NotIn");
        assert_eq!(info.cluster_selectors[1].values, vec!["excluded-tenant"]);

        assert_eq!(info.excluded_tenants, vec!["excluded-tenant"]);

        assert_eq!(info.chart_path.as_deref(), Some("charts/web-app"));

        assert_eq!(info.value_files.len(), 2);
        assert!(info.value_files[0].contains("web-app-values.yaml"));

        assert_eq!(info.helm_parameters.len(), 1);
        assert_eq!(info.helm_parameters[0].name, "tenant");
        assert_eq!(info.helm_parameters[0].value, "my-tenant");

        assert_eq!(info.source_file, "web-generator.yaml");
    }

    #[test]
    fn parse_git_generator_appset() {
        let yaml = r#"
apiVersion: argoproj.io/v1alpha1
kind: ApplicationSet
metadata:
  name: ingress-nginx
  namespace: argocd
spec:
  generators:
    - git:
        repoURL: git@github.com:example-org/example-environments.git
        revision: master
        files:
          - path: "environments/tenant-a/**/AZR/argocd/**/config.json"
          - path: "environments/tenant-b/staging/AZR/argocd/**/config.json"
          - path: "environments/tenant-c/**/AZR/argocd/**/config.json"
  template:
    metadata:
      name: 'ingress-nginx-test'
    spec:
      source:
        repoURL: git@github.com:example-org/example-environments.git
        targetRevision: master
        path: charts/ingress-nginx
        helm:
          valueFiles:
            - '../../../../environments/tenant/env/AZR/helm_values_files/region/ingress-nginx.yaml'
"#;
        let value = parse_yaml(yaml);
        let info = parse_appset(&value, "ingress-nginx-generator.yaml").unwrap();

        assert_eq!(info.name, "ingress-nginx");
        assert_eq!(info.generator_type, GeneratorType::Git);
        assert_eq!(info.git_file_paths.len(), 3);
        assert!(info.git_file_paths[0].contains("tenant-a"));
        assert!(info.git_file_paths[1].contains("tenant-b"));
        assert!(info.git_file_paths[2].contains("tenant-c"));
        assert!(info.cluster_selectors.is_empty());
        assert_eq!(info.chart_path.as_deref(), Some("charts/ingress-nginx"));
        assert_eq!(info.value_files.len(), 1);
    }

    #[test]
    fn parse_matrix_generator_appset() {
        let yaml = r#"
apiVersion: argoproj.io/v1alpha1
kind: ApplicationSet
metadata:
  name: runner-scale-set-v2
  namespace: argocd
spec:
  goTemplate: true
  generators:
    - matrix:
        generators:
          - clusters:
              selector:
                matchExpressions:
                  - key: runner-scale-set
                    operator: Exists
                  - key: tenant
                    operator: NotIn
                    values: ["excluded-tenant"]
          - list:
              elements:
                - runnerScaleSet: ci-runner
                  namespace: ci
  template:
    metadata:
      name: 'test-name'
    spec:
      source:
        repoURL: git@github.com:example-org/example-environments.git
        targetRevision: master
        path: charts/runner-scale-set
        helm:
          valueFiles:
            - '../../../../environments/cicd/cicd/AWS/helm_values_files/region/runners/values.yaml'
"#;
        let value = parse_yaml(yaml);
        let info = parse_appset(&value, "runner-generator.yaml").unwrap();

        assert_eq!(info.name, "runner-scale-set-v2");
        assert_eq!(info.generator_type, GeneratorType::Matrix);
        assert_eq!(info.cluster_selectors.len(), 2);
        assert_eq!(info.excluded_tenants, vec!["excluded-tenant"]);
        assert_eq!(info.chart_path.as_deref(), Some("charts/runner-scale-set"));
    }

    #[test]
    fn parse_appset_no_generators() {
        let yaml = r#"
apiVersion: argoproj.io/v1alpha1
kind: ApplicationSet
metadata:
  name: empty
spec:
  generators: []
  template:
    metadata:
      name: 'empty'
    spec:
      source:
        path: some/path
"#;
        let value = parse_yaml(yaml);
        let info = parse_appset(&value, "empty.yaml").unwrap();
        assert_eq!(info.generator_type, GeneratorType::Unknown);
        assert!(info.cluster_selectors.is_empty());
    }
}
