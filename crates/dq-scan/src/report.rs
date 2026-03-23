//! Report generation from topology.
//!
//! Serializes the topology into different output formats:
//! - JSON [`Value`] for programmatic consumption
//! - Graphviz DOT for visualization
//! - Summary statistics

use dq_core::Value;
use indexmap::IndexMap;
use std::sync::Arc;

use crate::argocd::GeneratorType;
use crate::helm::ChartType;
use crate::topology::{Edge, Topology};

/// Convert the full topology to a dq-core Value (JSON-serializable).
pub fn to_value(topology: &Topology) -> Value {
    let mut map = IndexMap::new();

    // ApplicationSets
    let appsets: Vec<Value> = topology
        .appsets
        .iter()
        .map(|a| {
            Value::from_pairs([
                ("name", Value::string(&a.name)),
                ("generator_type", Value::string(a.generator_type.to_string())),
                ("chart_path", match &a.chart_path {
                    Some(p) => Value::string(p),
                    None => Value::Null,
                }),
                (
                    "value_files",
                    Value::array(
                        a.value_files
                            .iter()
                            .map(|v| Value::string(v))
                            .collect::<Vec<_>>(),
                    ),
                ),
                (
                    "helm_parameters",
                    Value::array(
                        a.helm_parameters
                            .iter()
                            .map(|p| {
                                Value::from_pairs([
                                    ("name", Value::string(&p.name)),
                                    ("value", Value::string(&p.value)),
                                ])
                            })
                            .collect::<Vec<_>>(),
                    ),
                ),
                (
                    "excluded_tenants",
                    Value::array(
                        a.excluded_tenants
                            .iter()
                            .map(|t| Value::string(t))
                            .collect::<Vec<_>>(),
                    ),
                ),
                (
                    "cluster_selectors",
                    Value::array(
                        a.cluster_selectors
                            .iter()
                            .map(|s| {
                                Value::from_pairs([
                                    ("key", Value::string(&s.key)),
                                    ("operator", Value::string(&s.operator)),
                                    (
                                        "values",
                                        Value::array(
                                            s.values
                                                .iter()
                                                .map(|v| Value::string(v))
                                                .collect::<Vec<_>>(),
                                        ),
                                    ),
                                ])
                            })
                            .collect::<Vec<_>>(),
                    ),
                ),
                (
                    "git_file_paths",
                    Value::array(
                        a.git_file_paths
                            .iter()
                            .map(|p| Value::string(p))
                            .collect::<Vec<_>>(),
                    ),
                ),
                ("source_file", Value::string(&a.source_file)),
            ])
        })
        .collect();

    // Charts
    let charts: Vec<Value> = topology
        .charts
        .iter()
        .map(|c| {
            Value::from_pairs([
                ("name", Value::string(&c.name)),
                ("version", Value::string(&c.version)),
                ("description", Value::string(&c.description)),
                ("chart_type", Value::string(c.chart_type.to_string())),
                ("chart_dir", Value::string(&c.chart_dir)),
                (
                    "dependencies",
                    Value::array(
                        c.dependencies
                            .iter()
                            .map(|d| {
                                Value::from_pairs([
                                    ("name", Value::string(&d.name)),
                                    ("version", Value::string(&d.version)),
                                    ("repository", Value::string(&d.repository)),
                                ])
                            })
                            .collect::<Vec<_>>(),
                    ),
                ),
            ])
        })
        .collect();

    // Edges
    let edges: Vec<Value> = topology
        .edges
        .iter()
        .map(|e| match e {
            Edge::AppSetToChart { appset, chart } => Value::from_pairs([
                ("type", Value::string("appset_to_chart")),
                ("appset", Value::string(appset)),
                ("chart", Value::string(chart)),
            ]),
            Edge::AppSetToValueFile { appset, path } => Value::from_pairs([
                ("type", Value::string("appset_to_value_file")),
                ("appset", Value::string(appset)),
                ("path", Value::string(path)),
            ]),
            Edge::ChartDependency { parent, child } => Value::from_pairs([
                ("type", Value::string("chart_dependency")),
                ("parent", Value::string(parent)),
                ("child", Value::string(child)),
            ]),
        })
        .collect();

    // Taxonomy
    let taxonomy = Value::from_pairs([
        (
            "tenants",
            Value::array(
                topology
                    .taxonomy
                    .tenants
                    .iter()
                    .map(|t| Value::string(t))
                    .collect::<Vec<_>>(),
            ),
        ),
        (
            "environments",
            Value::array(
                topology
                    .taxonomy
                    .environments
                    .iter()
                    .map(|e| Value::string(e))
                    .collect::<Vec<_>>(),
            ),
        ),
        (
            "cloud_providers",
            Value::array(
                topology
                    .taxonomy
                    .cloud_providers
                    .iter()
                    .map(|c| Value::string(c))
                    .collect::<Vec<_>>(),
            ),
        ),
        (
            "regions",
            Value::from_pairs(topology.taxonomy.regions.iter().map(|(cloud, regions)| {
                (
                    cloud.as_str(),
                    Value::array(
                        regions
                            .iter()
                            .map(|r| Value::string(r))
                            .collect::<Vec<_>>(),
                    ),
                )
            })),
        ),
    ]);

    map.insert(Arc::from("appsets"), Value::array(appsets));
    map.insert(Arc::from("charts"), Value::array(charts));
    map.insert(Arc::from("edges"), Value::array(edges));
    map.insert(Arc::from("taxonomy"), taxonomy);
    map.insert(
        Arc::from("config_paths_count"),
        Value::int(topology.config_paths.len() as i64),
    );

    Value::Map(Arc::new(map))
}

/// Generate a Graphviz DOT representation of the topology.
pub fn to_dot(topology: &Topology) -> String {
    let mut lines = Vec::new();
    lines.push("digraph topology {".to_string());
    lines.push("  rankdir=LR;".to_string());
    lines.push("  node [shape=box];".to_string());
    lines.push(String::new());

    // AppSet nodes
    lines.push("  // ApplicationSets".to_string());
    for appset in &topology.appsets {
        let label = format!("{}\\n[{}]", appset.name, appset.generator_type);
        lines.push(format!(
            "  \"appset:{}\" [label=\"{}\", style=filled, fillcolor=\"#4FC3F7\"];",
            dot_escape(&appset.name),
            dot_escape(&label),
        ));
    }
    lines.push(String::new());

    // Chart nodes
    lines.push("  // Helm Charts".to_string());
    for chart in &topology.charts {
        let label = format!("{}\\n{}", chart.name, chart.version);
        lines.push(format!(
            "  \"chart:{}\" [label=\"{}\", style=filled, fillcolor=\"#81C784\"];",
            dot_escape(&chart.name),
            dot_escape(&label),
        ));
    }
    lines.push(String::new());

    // Edges
    lines.push("  // Edges".to_string());
    for edge in &topology.edges {
        match edge {
            Edge::AppSetToChart { appset, chart } => {
                lines.push(format!(
                    "  \"appset:{}\" -> \"chart:{}\" [label=\"deploys\"];",
                    dot_escape(appset),
                    dot_escape(chart),
                ));
            }
            Edge::AppSetToValueFile { appset, path } => {
                // Use a shortened path for readability
                let short = shorten_value_path(path);
                let vf_id = format!("vf:{appset}:{short}");
                lines.push(format!(
                    "  \"{}\" [label=\"{}\", shape=note, style=filled, fillcolor=\"#FFF176\"];",
                    dot_escape(&vf_id),
                    dot_escape(&short),
                ));
                lines.push(format!(
                    "  \"appset:{}\" -> \"{}\" [label=\"values\", style=dashed];",
                    dot_escape(appset),
                    dot_escape(&vf_id),
                ));
            }
            Edge::ChartDependency { parent, child } => {
                lines.push(format!(
                    "  \"chart:{}\" -> \"chart:{}\" [label=\"depends\", style=dotted];",
                    dot_escape(parent),
                    dot_escape(child),
                ));
            }
        }
    }

    lines.push("}".to_string());
    lines.join("\n")
}

/// Generate summary statistics as a Value.
pub fn to_summary(topology: &Topology) -> Value {
    let generator_counts = {
        let mut cluster = 0i64;
        let mut git = 0i64;
        let mut matrix = 0i64;
        let mut list = 0i64;
        let mut unknown = 0i64;
        for a in &topology.appsets {
            match a.generator_type {
                GeneratorType::Cluster => cluster += 1,
                GeneratorType::Git => git += 1,
                GeneratorType::Matrix => matrix += 1,
                GeneratorType::List => list += 1,
                GeneratorType::Unknown => unknown += 1,
            }
        }
        Value::from_pairs([
            ("cluster", Value::int(cluster)),
            ("git", Value::int(git)),
            ("matrix", Value::int(matrix)),
            ("list", Value::int(list)),
            ("unknown", Value::int(unknown)),
        ])
    };

    let chart_type_counts = {
        let mut app = 0i64;
        let mut lib = 0i64;
        for c in &topology.charts {
            match c.chart_type {
                ChartType::Application => app += 1,
                ChartType::Library => lib += 1,
            }
        }
        Value::from_pairs([
            ("application", Value::int(app)),
            ("library", Value::int(lib)),
        ])
    };

    let config_type_counts = {
        let mut type_counts: IndexMap<Arc<str>, i64> = IndexMap::new();
        for c in &topology.config_paths {
            *type_counts.entry(Arc::from(c.file_type.as_str())).or_insert(0) += 1;
        }
        let pairs: Vec<(&str, Value)> = type_counts.iter()
            .map(|(k, v)| (k.as_ref(), Value::int(*v)))
            .collect();
        Value::from_pairs(pairs)
    };

    let edge_counts = {
        let mut appset_to_chart = 0i64;
        let mut appset_to_vf = 0i64;
        let mut chart_dep = 0i64;
        for e in &topology.edges {
            match e {
                Edge::AppSetToChart { .. } => appset_to_chart += 1,
                Edge::AppSetToValueFile { .. } => appset_to_vf += 1,
                Edge::ChartDependency { .. } => chart_dep += 1,
            }
        }
        Value::from_pairs([
            ("appset_to_chart", Value::int(appset_to_chart)),
            ("appset_to_value_file", Value::int(appset_to_vf)),
            ("chart_dependency", Value::int(chart_dep)),
        ])
    };

    Value::from_pairs([
        ("appsets_count", Value::int(topology.appsets.len() as i64)),
        ("charts_count", Value::int(topology.charts.len() as i64)),
        (
            "config_paths_count",
            Value::int(topology.config_paths.len() as i64),
        ),
        ("edges_count", Value::int(topology.edges.len() as i64)),
        (
            "tenants_count",
            Value::int(topology.taxonomy.tenants.len() as i64),
        ),
        (
            "environments_count",
            Value::int(topology.taxonomy.environments.len() as i64),
        ),
        (
            "cloud_providers_count",
            Value::int(topology.taxonomy.cloud_providers.len() as i64),
        ),
        ("generators_by_type", generator_counts),
        ("charts_by_type", chart_type_counts),
        ("config_files_by_type", config_type_counts),
        ("edges_by_type", edge_counts),
    ])
}

/// Convert just the ApplicationSets to a Value array.
pub fn appsets_to_value(appsets: &[crate::argocd::AppSetInfo]) -> Value {
    Value::array(
        appsets
            .iter()
            .map(|a| {
                Value::from_pairs([
                    ("name", Value::string(&a.name)),
                    ("generator_type", Value::string(a.generator_type.to_string())),
                    ("chart_path", match &a.chart_path {
                        Some(p) => Value::string(p),
                        None => Value::Null,
                    }),
                    ("value_files", Value::array(
                        a.value_files.iter().map(|v| Value::string(v)).collect::<Vec<_>>(),
                    )),
                    ("excluded_tenants", Value::array(
                        a.excluded_tenants.iter().map(|t| Value::string(t)).collect::<Vec<_>>(),
                    )),
                ])
            })
            .collect::<Vec<_>>(),
    )
}

/// Convert just the Helm charts to a Value array.
pub fn charts_to_value(charts: &[crate::helm::ChartInfo]) -> Value {
    Value::array(
        charts
            .iter()
            .map(|c| {
                Value::from_pairs([
                    ("name", Value::string(&c.name)),
                    ("version", Value::string(&c.version)),
                    ("chart_type", Value::string(c.chart_type.to_string())),
                    ("description", if c.description.is_empty() {
                        Value::Null
                    } else {
                        Value::string(&c.description)
                    }),
                ])
            })
            .collect::<Vec<_>>(),
    )
}

/// Convert environments + taxonomy to a Value.
pub fn environments_to_value(
    config_paths: &[crate::environments::ConfigPath],
    taxonomy: &crate::environments::Taxonomy,
) -> Value {
    Value::from_pairs([
        ("tenants", Value::array(
            taxonomy.tenants.iter().map(|t| Value::string(t)).collect::<Vec<_>>(),
        )),
        ("environments", Value::array(
            taxonomy.environments.iter().map(|e| Value::string(e)).collect::<Vec<_>>(),
        )),
        ("cloud_providers", Value::array(
            taxonomy.cloud_providers.iter().map(|c| Value::string(c)).collect::<Vec<_>>(),
        )),
        ("regions", {
            let map: IndexMap<Arc<str>, Value> = taxonomy.regions.iter()
                .map(|(cloud, regions)| {
                    (Arc::from(cloud.as_str()), Value::array(
                        regions.iter().map(|r| Value::string(r)).collect::<Vec<_>>(),
                    ))
                })
                .collect();
            Value::map(map)
        }),
        ("config_paths_count", Value::int(config_paths.len() as i64)),
    ])
}

/// Escape special characters for DOT labels.
fn dot_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Shorten a value file path for DOT labels.
///
/// Strips leading `../` segments and long paths are reduced to just the filename.
fn shorten_value_path(path: &str) -> String {
    // Remove all leading "../" segments
    let mut shortened = path;
    while let Some(rest) = shortened.strip_prefix("../") {
        shortened = rest;
    }

    // If still too long, take the filename
    if shortened.len() > 60 {
        std::path::Path::new(shortened)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(shortened)
            .to_string()
    } else {
        shortened.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::argocd::{AppSetInfo, GeneratorType, HelmParameter, MatchExpression};
    use crate::environments::{ConfigPath, Taxonomy};
    use crate::helm::{ChartDependency, ChartInfo, ChartType};
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    fn sample_topology() -> Topology {
        let appsets = vec![AppSetInfo {
            name: "web-app-v2".to_string(),
            generator_type: GeneratorType::Cluster,
            cluster_selectors: vec![
                MatchExpression {
                    key: "web-app".to_string(),
                    operator: "Exists".to_string(),
                    values: vec![],
                },
                MatchExpression {
                    key: "tenant".to_string(),
                    operator: "NotIn".to_string(),
                    values: vec!["excluded-tenant".to_string()],
                },
            ],
            chart_path: Some("charts/web-app".to_string()),
            value_files: vec![
                "../../../../environments/{{tenant}}/{{env}}/{{cloud}}/helm_values_files/{{region}}/web-app-values.yaml".to_string(),
            ],
            helm_parameters: vec![HelmParameter {
                name: "tenant".to_string(),
                value: "my-tenant".to_string(),
            }],
            excluded_tenants: vec!["excluded-tenant".to_string()],
            git_file_paths: vec![],
            source_file: "web-generator.yaml".to_string(),
        }];

        let charts = vec![
            ChartInfo {
                name: "web-app".to_string(),
                version: "0.1.0".to_string(),
                description: "Web application".to_string(),
                chart_type: ChartType::Application,
                dependencies: vec![],
                chart_dir: "charts/web-app".to_string(),
            },
            ChartInfo {
                name: "monitoring".to_string(),
                version: "1.0.0".to_string(),
                description: "Monitoring stack".to_string(),
                chart_type: ChartType::Application,
                dependencies: vec![ChartDependency {
                    name: "prometheus".to_string(),
                    version: "25.0.0".to_string(),
                    repository: "https://prometheus-community.github.io/helm-charts".to_string(),
                }],
                chart_dir: "charts/monitoring".to_string(),
            },
        ];

        let config_paths = vec![ConfigPath {
            tenant: "tenant-a".to_string(),
            environment: "production".to_string(),
            cloud_provider: "AWS".to_string(),
            region: Some("us-east-2".to_string()),
            file_type: "helm_values".to_string(),
            path: PathBuf::from("/repo/environments/tenant-a/production/AWS/helm_values_files/us-east-2/values.yaml"),
        }];

        let mut regions = BTreeMap::new();
        regions.insert(
            "AWS".to_string(),
            BTreeSet::from(["us-east-2".to_string()]),
        );

        let taxonomy = Taxonomy {
            tenants: BTreeSet::from(["tenant-a".to_string()]),
            environments: BTreeSet::from(["production".to_string()]),
            cloud_providers: BTreeSet::from(["AWS".to_string()]),
            regions,
        };

        let edges = vec![
            Edge::AppSetToChart {
                appset: "web-app-v2".to_string(),
                chart: "web-app".to_string(),
            },
            Edge::AppSetToValueFile {
                appset: "web-app-v2".to_string(),
                path: "../../../../environments/{{tenant}}/{{env}}/{{cloud}}/helm_values_files/{{region}}/web-app-values.yaml".to_string(),
            },
            Edge::ChartDependency {
                parent: "monitoring".to_string(),
                child: "prometheus".to_string(),
            },
        ];

        Topology {
            appsets,
            charts,
            config_paths,
            edges,
            taxonomy,
        }
    }

    #[test]
    fn topology_to_value() {
        let topo = sample_topology();
        let val = to_value(&topo);

        // Check top-level structure
        assert!(val.get("appsets").is_some());
        assert!(val.get("charts").is_some());
        assert!(val.get("edges").is_some());
        assert!(val.get("taxonomy").is_some());

        // Check appsets array
        let appsets = val.get("appsets").unwrap().as_array().unwrap();
        assert_eq!(appsets.len(), 1);
        assert_eq!(
            appsets[0].get("name").unwrap().as_str().unwrap(),
            "web-app-v2"
        );
        assert_eq!(
            appsets[0].get("generator_type").unwrap().as_str().unwrap(),
            "cluster"
        );

        // Check charts array
        let charts = val.get("charts").unwrap().as_array().unwrap();
        assert_eq!(charts.len(), 2);

        // Check edges array
        let edges = val.get("edges").unwrap().as_array().unwrap();
        assert_eq!(edges.len(), 3);

        // Check taxonomy
        let taxonomy = val.get("taxonomy").unwrap();
        let tenants = taxonomy.get("tenants").unwrap().as_array().unwrap();
        assert_eq!(tenants.len(), 1);
        assert_eq!(tenants[0].as_str().unwrap(), "tenant-a");
    }

    #[test]
    fn topology_to_summary() {
        let topo = sample_topology();
        let summary = to_summary(&topo);

        assert_eq!(summary.get("appsets_count").unwrap().as_i64().unwrap(), 1);
        assert_eq!(summary.get("charts_count").unwrap().as_i64().unwrap(), 2);
        assert_eq!(
            summary
                .get("config_paths_count")
                .unwrap()
                .as_i64()
                .unwrap(),
            1
        );
        assert_eq!(summary.get("edges_count").unwrap().as_i64().unwrap(), 3);
        assert_eq!(
            summary.get("tenants_count").unwrap().as_i64().unwrap(),
            1
        );

        // Generator type breakdown
        let gen = summary.get("generators_by_type").unwrap();
        assert_eq!(gen.get("cluster").unwrap().as_i64().unwrap(), 1);
        assert_eq!(gen.get("git").unwrap().as_i64().unwrap(), 0);

        // Edge type breakdown
        let edge_types = summary.get("edges_by_type").unwrap();
        assert_eq!(
            edge_types.get("appset_to_chart").unwrap().as_i64().unwrap(),
            1
        );
        assert_eq!(
            edge_types
                .get("appset_to_value_file")
                .unwrap()
                .as_i64()
                .unwrap(),
            1
        );
        assert_eq!(
            edge_types
                .get("chart_dependency")
                .unwrap()
                .as_i64()
                .unwrap(),
            1
        );
    }

    #[test]
    fn topology_to_dot_produces_valid_output() {
        let topo = sample_topology();
        let dot = to_dot(&topo);

        // Structural checks
        assert!(dot.starts_with("digraph topology {"));
        assert!(dot.ends_with('}'));

        // Contains AppSet nodes
        assert!(dot.contains("appset:web-app-v2"));

        // Contains Chart nodes
        assert!(dot.contains("chart:web-app"));
        assert!(dot.contains("chart:monitoring"));

        // Contains edges
        assert!(dot.contains("-> \"chart:web-app\""));
        assert!(dot.contains("deploys"));
        assert!(dot.contains("depends"));
        assert!(dot.contains("values"));
    }

    #[test]
    fn shorten_value_path_works() {
        let long = "../../../../environments/{{tenant}}/{{env}}/AWS/helm_values_files/{{region}}/aws-saas-values.yaml";
        let short = shorten_value_path(long);
        assert!(!short.starts_with("../"));
        // Long paths (>60 chars) get shortened to just the filename
        assert!(short.contains("aws-saas-values.yaml"));

        // Shorter paths preserve structure
        let medium = "../../../../environments/global/dev/values.yaml";
        let short2 = shorten_value_path(medium);
        assert!(short2.contains("environments"));
        assert!(!short2.starts_with("../"));
    }

    #[test]
    fn topology_to_value_empty() {
        use std::collections::{BTreeMap, BTreeSet};
        let topo = Topology {
            appsets: vec![],
            charts: vec![],
            config_paths: vec![],
            edges: vec![],
            taxonomy: Taxonomy {
                tenants: BTreeSet::new(),
                environments: BTreeSet::new(),
                cloud_providers: BTreeSet::new(),
                regions: BTreeMap::new(),
            },
        };
        let val = to_value(&topo);
        assert_eq!(val.get("appsets").unwrap().as_array().unwrap().len(), 0);
        assert_eq!(val.get("charts").unwrap().as_array().unwrap().len(), 0);
    }

    #[test]
    fn topology_to_summary_empty() {
        use std::collections::{BTreeMap, BTreeSet};
        let topo = Topology {
            appsets: vec![],
            charts: vec![],
            config_paths: vec![],
            edges: vec![],
            taxonomy: Taxonomy {
                tenants: BTreeSet::new(),
                environments: BTreeSet::new(),
                cloud_providers: BTreeSet::new(),
                regions: BTreeMap::new(),
            },
        };
        let summary = to_summary(&topo);
        assert_eq!(summary.get("appsets_count").unwrap().as_i64().unwrap(), 0);
        assert_eq!(summary.get("charts_count").unwrap().as_i64().unwrap(), 0);
        assert_eq!(summary.get("edges_count").unwrap().as_i64().unwrap(), 0);
    }

    #[test]
    fn dot_escape_special_chars() {
        assert_eq!(dot_escape("hello\"world"), "hello\\\"world");
        assert_eq!(dot_escape("line\nnewline"), "line\\nnewline");
        assert_eq!(dot_escape("back\\slash"), "back\\\\slash");
    }
}
