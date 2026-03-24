//! GitHub-friendly Mermaid diagram generation.
//!
//! Generates Markdown files containing Mermaid diagrams that render natively
//! on GitHub. Three visualizations:
//!
//! - **chart_deps** -- Chart dependency flowchart with external markers.
//! - **deploy_graph** -- AppSet-to-Chart flowchart with generator type colors.
//! - **matrix** -- Tenant x Environment config count table (plain markdown).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use dq_scan::topology::{Edge, Topology};

/// Maximum number of nodes before we truncate a Mermaid diagram.
/// GitHub's renderer chokes on very large diagrams.
const MAX_CHART_NODES: usize = 80;
const MAX_DEPLOY_CHARTS: usize = 30;

/// Sanitize a string into a valid Mermaid node ID.
///
/// Mermaid node IDs must be alphanumeric + underscores only.
fn sanitize_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

/// Generate a Markdown document with a Mermaid flowchart showing chart dependencies.
pub fn chart_deps(topology: &Topology) -> String {
    let mut out = String::new();
    out.push_str("# Chart Dependencies\n\n");

    if topology.charts.is_empty() {
        out.push_str("No chart data available.\n");
        return out;
    }

    // Build parent -> children map from edges
    let mut parent_to_children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut has_parent: BTreeSet<&str> = BTreeSet::new();

    for edge in &topology.edges {
        if let Edge::ChartDependency { parent, child } = edge {
            parent_to_children
                .entry(parent.as_str())
                .or_default()
                .push(child.as_str());
            has_parent.insert(child.as_str());
        }
    }

    // Index charts by name for metadata lookup
    let chart_map: BTreeMap<&str, &dq_scan::helm::ChartInfo> = topology
        .charts
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();

    let charts_with_deps: Vec<&str> = parent_to_children.keys().copied().collect();
    let standalone: Vec<&str> = topology
        .charts
        .iter()
        .filter(|c| {
            !parent_to_children.contains_key(c.name.as_str())
                && !has_parent.contains(c.name.as_str())
        })
        .map(|c| c.name.as_str())
        .collect();

    // Count total nodes to decide if we need truncation
    let mut all_nodes: BTreeSet<&str> = BTreeSet::new();
    for (parent, children) in &parent_to_children {
        all_nodes.insert(parent);
        for child in children {
            all_nodes.insert(child);
        }
    }
    let total_node_count = all_nodes.len();
    let truncated = total_node_count > MAX_CHART_NODES;

    if !charts_with_deps.is_empty() {
        out.push_str("```mermaid\ngraph TD\n");

        if truncated {
            // Only include the first MAX_CHART_NODES nodes
            let mut included_nodes: BTreeSet<&str> = BTreeSet::new();
            let mut node_count = 0;

            'outer: for parent in &charts_with_deps {
                if node_count >= MAX_CHART_NODES {
                    break;
                }
                included_nodes.insert(parent);
                node_count += 1;
                if let Some(children) = parent_to_children.get(parent) {
                    for child in children {
                        if node_count >= MAX_CHART_NODES {
                            break 'outer;
                        }
                        included_nodes.insert(child);
                        node_count += 1;
                    }
                }
            }

            render_chart_mermaid_nodes(
                &mut out,
                &charts_with_deps,
                &parent_to_children,
                &chart_map,
                topology,
                &included_nodes,
            );
        } else {
            render_chart_mermaid_nodes(
                &mut out,
                &charts_with_deps,
                &parent_to_children,
                &chart_map,
                topology,
                &all_nodes,
            );
        }

        out.push_str("```\n\n");

        if truncated {
            out.push_str(&format!(
                "> **Note:** Showing first {} nodes out of {} total to avoid rendering limits.\n\n",
                MAX_CHART_NODES,
                total_node_count + standalone.len(),
            ));
        }
    }

    // Standalone charts as a simple list
    if !standalone.is_empty() {
        out.push_str("## Standalone Charts (no dependencies)\n\n");
        for name in &standalone {
            if let Some(info) = chart_map.get(name) {
                let desc = if info.description.is_empty() {
                    String::new()
                } else {
                    format!(" -- {}", info.description)
                };
                out.push_str(&format!("- **{}** v{}{}\n", name, info.version, desc));
            } else {
                out.push_str(&format!("- **{}**\n", name));
            }
        }
        out.push('\n');
    }

    // Summary
    let total_deps: usize = parent_to_children.values().map(|v| v.len()).sum();
    out.push_str(&format!(
        "{} charts total, {} with dependencies ({} dependency edges), {} standalone\n",
        topology.charts.len(),
        charts_with_deps.len(),
        total_deps,
        standalone.len(),
    ));

    out
}

/// Render Mermaid nodes and edges for charts with dependencies.
fn render_chart_mermaid_nodes(
    out: &mut String,
    charts_with_deps: &[&str],
    parent_to_children: &BTreeMap<&str, Vec<&str>>,
    chart_map: &BTreeMap<&str, &dq_scan::helm::ChartInfo>,
    topology: &Topology,
    included: &BTreeSet<&str>,
) {
    out.push_str("    subgraph \"Charts with dependencies\"\n");

    let mut visited = BTreeSet::new();
    let mut emitted_nodes: BTreeSet<String> = BTreeSet::new();
    let mut edges_buf: Vec<String> = Vec::new();

    for parent in charts_with_deps {
        if !included.contains(parent) {
            continue;
        }
        emit_chart_node(
            out,
            &mut edges_buf,
            &mut emitted_nodes,
            &mut visited,
            parent,
            parent_to_children,
            chart_map,
            topology,
            included,
        );
    }

    out.push_str("    end\n");

    // Emit edges after the subgraph
    for edge_line in &edges_buf {
        out.push_str(edge_line);
    }
}

/// Recursively emit a chart node and its dependency edges.
fn emit_chart_node(
    out: &mut String,
    edges: &mut Vec<String>,
    emitted: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
    name: &str,
    parent_to_children: &BTreeMap<&str, Vec<&str>>,
    chart_map: &BTreeMap<&str, &dq_scan::helm::ChartInfo>,
    topology: &Topology,
    included: &BTreeSet<&str>,
) {
    let node_id = sanitize_id(name);
    if !visited.insert(name.to_string()) {
        return;
    }

    // Emit node definition if not yet emitted
    if emitted.insert(node_id.clone()) {
        if let Some(info) = chart_map.get(name) {
            out.push_str(&format!(
                "        {}[\"{}  v{}\"]\n",
                node_id, name, info.version
            ));
        } else {
            // External dependency
            let dep_version = find_dep_version(name, topology);
            out.push_str(&format!(
                "        {}[\"{} v{}<br/><i>external</i>\"]\n",
                node_id, name, dep_version
            ));
        }
    }

    // Recurse into children
    if let Some(children) = parent_to_children.get(name) {
        for child in children {
            let child_id = sanitize_id(child);

            // Emit child node if not yet done and included
            if included.contains(child) && emitted.insert(child_id.clone()) {
                if let Some(info) = chart_map.get(child) {
                    out.push_str(&format!(
                        "        {}[\"{}  v{}\"]\n",
                        child_id, child, info.version
                    ));
                } else {
                    let dep_version = find_dep_version(child, topology);
                    out.push_str(&format!(
                        "        {}[\"{} v{}<br/><i>external</i>\"]\n",
                        child_id, child, dep_version
                    ));
                }
            }

            // Emit edge
            if included.contains(child) {
                edges.push(format!("    {} --> {}\n", node_id, child_id));
            }

            // Recurse
            if included.contains(child) {
                emit_chart_node(
                    out, edges, emitted, visited, child,
                    parent_to_children, chart_map, topology, included,
                );
            }
        }
    }
}

/// Find the version of a dependency by searching all charts' dependency lists.
fn find_dep_version(dep_name: &str, topology: &Topology) -> String {
    for chart in &topology.charts {
        for dep in &chart.dependencies {
            if dep.name == dep_name {
                return dep.version.clone();
            }
        }
    }
    String::from("?")
}

/// Generate a Markdown document with a Mermaid flowchart showing AppSet-to-Chart relationships.
pub fn deploy_graph(topology: &Topology) -> String {
    let mut out = String::new();
    out.push_str("# Deployment Topology\n\n");

    // Build chart -> [(appset_name, generator_type)] mapping
    let mut chart_to_appsets: BTreeMap<&str, Vec<(&str, String)>> = BTreeMap::new();
    let mut unlinked_appsets: Vec<(&str, String)> = Vec::new();

    // Index appsets by name
    let appset_map: BTreeMap<&str, &dq_scan::argocd::AppSetInfo> = topology
        .appsets
        .iter()
        .map(|a| (a.name.as_str(), a))
        .collect();

    let mut linked_appsets: BTreeSet<&str> = BTreeSet::new();

    for edge in &topology.edges {
        if let Edge::AppSetToChart { appset, chart } = edge {
            let gen_type = appset_map
                .get(appset.as_str())
                .map(|a| a.generator_type.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            chart_to_appsets
                .entry(chart.as_str())
                .or_default()
                .push((appset.as_str(), gen_type));
            linked_appsets.insert(appset.as_str());
        }
    }

    for appset in &topology.appsets {
        if !linked_appsets.contains(appset.name.as_str()) {
            unlinked_appsets.push((appset.name.as_str(), appset.generator_type.to_string()));
        }
    }

    if chart_to_appsets.is_empty() && unlinked_appsets.is_empty() {
        out.push_str("No deployment data available.\n");
        return out;
    }

    // If too many charts, keep only the most connected
    let charts_to_show: Vec<(&str, &Vec<(&str, String)>)> = if chart_to_appsets.len() > MAX_DEPLOY_CHARTS {
        let mut by_count: Vec<(&str, &Vec<(&str, String)>)> =
            chart_to_appsets.iter().map(|(k, v)| (*k, v)).collect();
        by_count.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        by_count.truncate(MAX_DEPLOY_CHARTS);
        by_count
    } else {
        chart_to_appsets.iter().map(|(k, v)| (*k, v)).collect()
    };

    let truncated = chart_to_appsets.len() > MAX_DEPLOY_CHARTS;

    // Group appsets by generator type for subgraphs
    let mut gen_type_appsets: BTreeMap<String, Vec<(&str, &str)>> = BTreeMap::new();
    for (chart, appsets) in &charts_to_show {
        for (appset_name, gen_type) in *appsets {
            gen_type_appsets
                .entry(gen_type.clone())
                .or_default()
                .push((appset_name, chart));
        }
    }

    // Color map for generator types
    let color_map: HashMap<&str, (&str, &str)> = [
        ("cluster", ("#1f6feb", "#fff")),
        ("git", ("#238636", "#fff")),
        ("matrix", ("#8957e5", "#fff")),
        ("list", ("#d29922", "#000")),
    ]
    .into_iter()
    .collect();

    out.push_str("```mermaid\ngraph LR\n");

    // Subgraph per generator type with appset nodes
    let mut appset_node_ids: BTreeMap<&str, String> = BTreeMap::new();
    let mut style_directives: Vec<String> = Vec::new();

    for (gen_type, appset_chart_pairs) in &gen_type_appsets {
        let subgraph_title = gen_type.as_str();
        out.push_str(&format!("    subgraph \"{}\"\n", subgraph_title));

        let mut seen_appsets: BTreeSet<&str> = BTreeSet::new();
        for (appset_name, _chart) in appset_chart_pairs {
            if !seen_appsets.insert(appset_name) {
                continue;
            }
            let node_id = format!("appset_{}", sanitize_id(appset_name));
            out.push_str(&format!(
                "        {}[\"{}\"]\n",
                node_id, appset_name
            ));

            if let Some(&(fill, fg)) = color_map.get(gen_type.as_str()) {
                style_directives.push(format!(
                    "    style {} fill:{},color:{}\n",
                    node_id, fill, fg
                ));
            }

            appset_node_ids.insert(appset_name, node_id);
        }

        out.push_str("    end\n");
    }

    // Chart nodes subgraph
    let chart_names: BTreeSet<&str> = charts_to_show.iter().map(|(c, _)| *c).collect();
    out.push_str("    subgraph \"Charts\"\n");
    for chart in &chart_names {
        let node_id = format!("chart_{}", sanitize_id(chart));
        out.push_str(&format!("        {}[\"{}\"]\n", node_id, chart));
    }
    out.push_str("    end\n");

    // Edges: appset -> chart
    for (chart, appsets) in &charts_to_show {
        let chart_node = format!("chart_{}", sanitize_id(chart));
        for (appset_name, _gen_type) in *appsets {
            if let Some(appset_node) = appset_node_ids.get(appset_name) {
                out.push_str(&format!("    {} --> {}\n", appset_node, chart_node));
            }
        }
    }

    // Style directives
    for directive in &style_directives {
        out.push_str(directive);
    }

    out.push_str("```\n\n");

    if truncated {
        out.push_str(&format!(
            "> **Note:** Showing top {} most-connected charts out of {} total.\n\n",
            MAX_DEPLOY_CHARTS,
            chart_to_appsets.len(),
        ));
    }

    // Unlinked appsets
    if !unlinked_appsets.is_empty() {
        out.push_str("## Unlinked ApplicationSets (no chart match)\n\n");
        for (name, gen_type) in &unlinked_appsets {
            out.push_str(&format!("- **{}** ({})\n", name, gen_type));
        }
        out.push('\n');
    }

    // Stats
    out.push_str(&format!(
        "{} charts deployed by {} appsets ({} unlinked)\n",
        chart_to_appsets.len(),
        linked_appsets.len(),
        unlinked_appsets.len(),
    ));

    out
}

/// Generate a Markdown document with a tenant-environment config count table.
///
/// Uses plain markdown tables rather than Mermaid since tables render better
/// as standard markdown on GitHub.
pub fn matrix(topology: &Topology) -> String {
    let mut out = String::new();
    out.push_str("# Tenant-Environment Matrix\n\n");

    // Collect counts: (tenant, environment) -> count
    let mut counts: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for cp in &topology.config_paths {
        *counts
            .entry((&cp.tenant, &cp.environment))
            .or_insert(0) += 1;
    }

    let tenants: BTreeSet<&str> = topology.taxonomy.tenants.iter().map(|s| s.as_str()).collect();
    let envs: BTreeSet<&str> = topology.taxonomy.environments.iter().map(|s| s.as_str()).collect();

    if tenants.is_empty() || envs.is_empty() {
        out.push_str("No environment data available.\n");
        return out;
    }

    let env_list: Vec<&str> = envs.iter().copied().collect();
    let has_production = env_list.iter().any(|e| *e == "production");

    // Header row
    out.push_str("| Tenant |");
    for env in &env_list {
        out.push_str(&format!(" {} |", env));
    }
    out.push('\n');

    // Separator row
    out.push_str("|--------|");
    for _ in &env_list {
        out.push_str("-----|");
    }
    out.push('\n');

    // Data rows
    for tenant in &tenants {
        out.push_str(&format!("| {} |", tenant));
        for env in &env_list {
            let count = counts.get(&(*tenant, *env)).copied().unwrap_or(0);
            let cell = if count == 0 {
                " - ".to_string()
            } else if has_production && *env == "production" {
                format!(" **{}** ", count)
            } else {
                format!(" {} ", count)
            };
            out.push_str(&format!("{}|", cell));
        }
        out.push('\n');
    }

    // Totals row
    out.push_str("| **Total** |");
    for env in &env_list {
        let total: usize = tenants
            .iter()
            .map(|t| counts.get(&(*t, *env)).copied().unwrap_or(0))
            .sum();
        let cell = if total == 0 {
            " - ".to_string()
        } else if has_production && *env == "production" {
            format!(" **{}** ", total)
        } else {
            format!(" {} ", total)
        };
        out.push_str(&format!("{}|", cell));
    }
    out.push('\n');

    out.push('\n');
    out.push_str(&format!(
        "{} tenants, {} environments, {} config files\n",
        tenants.len(),
        envs.len(),
        topology.config_paths.len(),
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use dq_scan::argocd::{AppSetInfo, GeneratorType};
    use dq_scan::environments::{ConfigPath, Taxonomy};
    use dq_scan::helm::{ChartDependency, ChartInfo, ChartType};
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    fn empty_topology() -> Topology {
        Topology {
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
        }
    }

    #[test]
    fn chart_deps_empty() {
        let md = chart_deps(&empty_topology());
        assert!(md.contains("No chart data available"));
    }

    #[test]
    fn chart_deps_with_dependencies() {
        let charts = vec![
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
            ChartInfo {
                name: "web".to_string(),
                version: "0.1.0".to_string(),
                description: "Web app".to_string(),
                chart_type: ChartType::Application,
                dependencies: vec![],
                chart_dir: "charts/web".to_string(),
            },
        ];
        let edges = vec![Edge::ChartDependency {
            parent: "monitoring".to_string(),
            child: "prometheus".to_string(),
        }];
        let topo = Topology {
            charts,
            edges,
            ..empty_topology()
        };
        let md = chart_deps(&topo);
        assert!(md.contains("```mermaid"));
        assert!(md.contains("monitoring"));
        assert!(md.contains("prometheus"));
        assert!(md.contains("external"));
        assert!(md.contains("Standalone Charts"));
        assert!(md.contains("web"));
    }

    #[test]
    fn deploy_graph_empty() {
        let md = deploy_graph(&empty_topology());
        assert!(md.contains("No deployment data available"));
    }

    #[test]
    fn deploy_graph_with_deployment() {
        let appsets = vec![AppSetInfo {
            name: "web-v2".to_string(),
            generator_type: GeneratorType::Cluster,
            cluster_selectors: vec![],
            chart_path: Some("charts/web".to_string()),
            value_files: vec![],
            helm_parameters: vec![],
            excluded_tenants: vec![],
            git_file_paths: vec![],
            source_file: "web.yaml".to_string(),
        }];
        let charts = vec![ChartInfo {
            name: "web".to_string(),
            version: "1.0.0".to_string(),
            description: "Web app".to_string(),
            chart_type: ChartType::Application,
            dependencies: vec![],
            chart_dir: "charts/web".to_string(),
        }];
        let edges = vec![Edge::AppSetToChart {
            appset: "web-v2".to_string(),
            chart: "web".to_string(),
        }];
        let topo = Topology {
            appsets,
            charts,
            edges,
            ..empty_topology()
        };
        let md = deploy_graph(&topo);
        assert!(md.contains("```mermaid"));
        assert!(md.contains("web-v2"));
        assert!(md.contains("web"));
        assert!(md.contains("cluster"));
        assert!(md.contains("fill:#1f6feb"));
    }

    #[test]
    fn matrix_empty() {
        let md = matrix(&empty_topology());
        assert!(md.contains("No environment data available"));
    }

    #[test]
    fn matrix_with_data() {
        let config_paths = vec![
            ConfigPath {
                tenant: "acme".to_string(),
                environment: "production".to_string(),
                cloud_provider: "AWS".to_string(),
                region: Some("us-east-1".to_string()),
                file_type: "helm_values".to_string(),
                path: PathBuf::from("/repo/environments/acme/production/AWS/helm_values_files/us-east-1/values.yaml"),
            },
            ConfigPath {
                tenant: "acme".to_string(),
                environment: "staging".to_string(),
                cloud_provider: "AWS".to_string(),
                region: Some("us-east-1".to_string()),
                file_type: "helm_values".to_string(),
                path: PathBuf::from("/repo/environments/acme/staging/AWS/helm_values_files/us-east-1/values.yaml"),
            },
        ];
        let topo = Topology {
            config_paths,
            taxonomy: Taxonomy {
                tenants: BTreeSet::from(["acme".to_string()]),
                environments: BTreeSet::from(["production".to_string(), "staging".to_string()]),
                cloud_providers: BTreeSet::from(["AWS".to_string()]),
                regions: BTreeMap::new(),
            },
            ..empty_topology()
        };
        let md = matrix(&topo);
        assert!(md.contains("| Tenant |"));
        assert!(md.contains("acme"));
        assert!(md.contains("**1**")); // production bolded
        assert!(md.contains("| **Total** |"));
    }

    #[test]
    fn sanitize_id_replaces_special_chars() {
        assert_eq!(sanitize_id("my-chart"), "my_chart");
        assert_eq!(sanitize_id("a.b/c"), "a_b_c");
        assert_eq!(sanitize_id("simple"), "simple");
    }
}
