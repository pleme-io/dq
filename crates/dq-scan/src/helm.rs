//! Helm Chart.yaml parsing.
//!
//! Extracts structured metadata from Helm chart definitions: name, version,
//! description, chart type, and dependency information.

use dq_core::Value;

use crate::ScanError;

/// Parsed metadata from a Helm Chart.yaml.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartInfo {
    /// Chart name.
    pub name: String,
    /// Chart version (SemVer string).
    pub version: String,
    /// Human-readable description.
    pub description: String,
    /// Chart type: "application" or "library".
    pub chart_type: ChartType,
    /// Chart dependencies (subchart requirements).
    pub dependencies: Vec<ChartDependency>,
    /// Relative path of the chart directory within the repo.
    pub chart_dir: String,
}

/// Helm chart type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChartType {
    Application,
    Library,
}

impl std::fmt::Display for ChartType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChartType::Application => write!(f, "application"),
            ChartType::Library => write!(f, "library"),
        }
    }
}

/// A dependency entry from Chart.yaml.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartDependency {
    /// Dependency chart name.
    pub name: String,
    /// Required version (SemVer constraint).
    pub version: String,
    /// Helm repository URL.
    pub repository: String,
}

/// Parse a Chart.yaml from a dq-core Value.
///
/// The Value should represent the top-level Chart.yaml document.
pub fn parse_chart(value: &Value, chart_dir: &str) -> Result<ChartInfo, ScanError> {
    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let version = extract_version(value);

    let description = value
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let chart_type = match value.get("type").and_then(|v| v.as_str()) {
        Some("library") => ChartType::Library,
        _ => ChartType::Application,
    };

    let dependencies = value
        .get("dependencies")
        .and_then(|v| v.as_array())
        .unwrap_or(&[])
        .iter()
        .filter_map(|dep| {
            let dep_name = dep.get("name")?.as_str()?.to_string();
            let dep_version = extract_version(dep);
            let repository = dep
                .get("repository")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(ChartDependency {
                name: dep_name,
                version: dep_version,
                repository,
            })
        })
        .collect();

    Ok(ChartInfo {
        name,
        version,
        description,
        chart_type,
        dependencies,
        chart_dir: chart_dir.to_string(),
    })
}

/// Extract a version field, handling both string and numeric types.
///
/// Chart.yaml versions can be parsed as numbers by YAML parsers (e.g., `3.73.0`
/// might have the minor/patch parsed as a float). We handle Int, Float, and
/// String cases.
fn extract_version(value: &Value) -> String {
    match value.get("version") {
        Some(Value::String(s)) => s.to_string(),
        Some(Value::Float(f)) => format!("{f}"),
        Some(Value::Int(i)) => format!("{i}"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq_formats::FormatKind;

    fn parse_yaml(input: &str) -> Value {
        dq_formats::parse(FormatKind::Yaml, input.as_bytes()).expect("YAML parse failed")
    }

    #[test]
    fn parse_chart_yaml_basic() {
        let yaml = r#"
apiVersion: v2
name: web-app
description: Web application chart.
type: application
version: 0.1.0
"#;
        let value = parse_yaml(yaml);
        let info = parse_chart(&value, "charts/web-app").unwrap();

        assert_eq!(info.name, "web-app");
        assert_eq!(info.version, "0.1.0");
        assert_eq!(info.description, "Web application chart.");
        assert_eq!(info.chart_type, ChartType::Application);
        assert!(info.dependencies.is_empty());
        assert_eq!(info.chart_dir, "charts/web-app");
    }

    #[test]
    fn parse_chart_yaml_with_dependencies() {
        let yaml = r#"
apiVersion: v2
name: datadog
version: "3.73.0"
appVersion: "7"
description: Datadog Agent
dependencies:
  - name: datadog
    version: "3.73.0"
    repository: https://helm.datadoghq.com
"#;
        let value = parse_yaml(yaml);
        let info = parse_chart(&value, "saas/kubernetes/helm/datadog").unwrap();

        assert_eq!(info.name, "datadog");
        assert_eq!(info.description, "Datadog Agent");
        assert_eq!(info.dependencies.len(), 1);
        assert_eq!(info.dependencies[0].name, "datadog");
        assert_eq!(info.dependencies[0].repository, "https://helm.datadoghq.com");
    }

    #[test]
    fn parse_chart_yaml_library_type() {
        let yaml = r#"
apiVersion: v2
name: common
version: "1.0.0"
description: Common library chart
type: library
"#;
        let value = parse_yaml(yaml);
        let info = parse_chart(&value, "charts/common").unwrap();

        assert_eq!(info.chart_type, ChartType::Library);
    }

    #[test]
    fn parse_chart_yaml_no_type_defaults_to_application() {
        let yaml = r#"
apiVersion: v2
name: minimal
version: "0.1.0"
description: Minimal chart
"#;
        let value = parse_yaml(yaml);
        let info = parse_chart(&value, "charts/minimal").unwrap();

        assert_eq!(info.chart_type, ChartType::Application);
    }
}
