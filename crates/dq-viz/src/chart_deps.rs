//! Chart dependency tree visualization.
//!
//! Generates an indented tree view showing chart -> dependency relationships
//! with version numbers and repository sources.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use dq_scan::helm::ChartInfo;
use dq_scan::topology::{Edge, Topology};

use crate::html;

/// Render the chart dependency tree as a self-contained HTML page.
pub fn render(topology: &Topology) -> Result<String> {
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
    let chart_map: BTreeMap<&str, &ChartInfo> = topology
        .charts
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();

    // Separate charts into: with deps, without deps, and dep-only (not a top-level chart)
    let charts_with_deps: Vec<&str> = parent_to_children.keys().copied().collect();

    // Charts that appear in the repo but have no dependencies and are not dependencies
    let standalone: Vec<&str> = topology
        .charts
        .iter()
        .filter(|c| {
            !parent_to_children.contains_key(c.name.as_str())
                && !has_parent.contains(c.name.as_str())
        })
        .map(|c| c.name.as_str())
        .collect();

    let mut body = String::new();
    body.push_str("<h1>Chart Dependencies</h1>\n");
    body.push_str("<h2>Helm chart dependency tree with version and repository info</h2>\n");

    if topology.charts.is_empty() {
        body.push_str("<p>No chart data available.</p>\n");
        return Ok(html::document("Chart Deps - dq viz", CSS, &body));
    }

    // Charts with dependencies
    if !charts_with_deps.is_empty() {
        body.push_str("<div class=\"section\">\n");
        body.push_str("<h3>Charts with dependencies</h3>\n");
        for parent_name in &charts_with_deps {
            render_chart_tree(
                &mut body,
                parent_name,
                &parent_to_children,
                &chart_map,
                topology,
                0,
            );
        }
        body.push_str("</div>\n");
    }

    // Standalone charts
    if !standalone.is_empty() {
        body.push_str("<div class=\"section\">\n");
        body.push_str("<h3>Standalone charts (no dependencies)</h3>\n");
        body.push_str("<div class=\"standalone-grid\">\n");
        for name in &standalone {
            if let Some(info) = chart_map.get(name) {
                body.push_str(&format!(
                    "<div class=\"standalone-card\"><span class=\"chart-name\">{}</span> <span class=\"version\">v{}</span>",
                    html::escape(name),
                    html::escape(&info.version),
                ));
                if !info.description.is_empty() {
                    body.push_str(&format!(
                        "<div class=\"desc\">{}</div>",
                        html::escape(&info.description)
                    ));
                }
                body.push_str("</div>\n");
            }
        }
        body.push_str("</div>\n");
        body.push_str("</div>\n");
    }

    // Summary stats
    let total_deps: usize = parent_to_children.values().map(|v| v.len()).sum();
    body.push_str(&format!(
        "<p class=\"stats\">{} charts total, {} with dependencies ({} dependency edges), {} standalone</p>\n",
        topology.charts.len(),
        charts_with_deps.len(),
        total_deps,
        standalone.len(),
    ));

    body.push_str("<p class=\"nav\"><a href=\"index.html\">Back to index</a></p>\n");

    Ok(html::document("Chart Deps - dq viz", CSS, &body))
}

/// Recursively render a chart and its dependencies as indented tree nodes.
fn render_chart_tree(
    body: &mut String,
    name: &str,
    parent_to_children: &BTreeMap<&str, Vec<&str>>,
    chart_map: &BTreeMap<&str, &ChartInfo>,
    topology: &Topology,
    depth: usize,
) {
    let indent_class = format!("depth-{}", depth.min(4));

    if let Some(info) = chart_map.get(name) {
        body.push_str(&format!(
            "<div class=\"tree-node {indent_class}\"><span class=\"connector\">{}</span><span class=\"chart-name\">{}</span> <span class=\"version\">v{}</span> <span class=\"chart-type\">{}</span>",
            if depth == 0 { "&#x25BC;" } else { "&#x251C;&#x2500;" },
            html::escape(name),
            html::escape(&info.version),
            info.chart_type,
        ));
        if !info.description.is_empty() {
            body.push_str(&format!(
                " <span class=\"desc\">{}</span>",
                html::escape(&info.description)
            ));
        }
        body.push_str("</div>\n");
    } else {
        // Dependency not found as a local chart (external)
        // Look up version from parent's dependency list
        let dep_version = find_dep_version(name, topology);
        let dep_repo = find_dep_repo(name, topology);
        body.push_str(&format!(
            "<div class=\"tree-node {indent_class}\"><span class=\"connector\">&#x251C;&#x2500;</span><span class=\"chart-name external\">{}</span> <span class=\"version\">v{}</span>",
            html::escape(name),
            html::escape(&dep_version),
        ));
        if !dep_repo.is_empty() {
            body.push_str(&format!(
                " <span class=\"repo\">{}</span>",
                html::escape(&dep_repo)
            ));
        }
        body.push_str(" <span class=\"external-tag\">external</span>");
        body.push_str("</div>\n");
    }

    // Recurse into children
    if let Some(children) = parent_to_children.get(name) {
        for child in children {
            render_chart_tree(body, child, parent_to_children, chart_map, topology, depth + 1);
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

/// Find the repository URL of a dependency by searching all charts' dependency lists.
fn find_dep_repo(dep_name: &str, topology: &Topology) -> String {
    for chart in &topology.charts {
        for dep in &chart.dependencies {
            if dep.name == dep_name {
                return dep.repository.clone();
            }
        }
    }
    String::new()
}

const CSS: &str = r#"
.section { margin-bottom: 2rem; }
h3 { color: #f0f6fc; font-size: 1rem; margin-bottom: 1rem; border-bottom: 1px solid #30363d; padding-bottom: 0.5rem; }
.tree-node { padding: 0.4rem 0.5rem; font-size: 0.875rem; display: flex; align-items: center; gap: 0.5rem; border-left: 2px solid #30363d; }
.tree-node:hover { background: #161b22; }
.depth-0 { margin-left: 0; border-left-color: #58a6ff; }
.depth-1 { margin-left: 1.5rem; border-left-color: #3fb950; }
.depth-2 { margin-left: 3rem; border-left-color: #d29922; }
.depth-3 { margin-left: 4.5rem; border-left-color: #da3633; }
.depth-4 { margin-left: 6rem; border-left-color: #8957e5; }
.connector { color: #484f58; font-family: monospace; }
.chart-name { color: #f0f6fc; font-weight: 500; }
.chart-name.external { color: #8b949e; font-style: italic; }
.version { color: #8b949e; font-size: 0.75rem; }
.chart-type { font-size: 0.65rem; color: #8b949e; background: #21262d; padding: 0.1rem 0.4rem; border-radius: 8px; }
.desc { color: #6e7681; font-size: 0.75rem; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.repo { color: #484f58; font-size: 0.7rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 20rem; }
.external-tag { font-size: 0.6rem; color: #d29922; background: #2d1b00; padding: 0.1rem 0.35rem; border-radius: 8px; }
.standalone-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 0.75rem; }
.standalone-card { background: #161b22; border: 1px solid #30363d; border-radius: 6px; padding: 0.75rem; font-size: 0.875rem; }
.standalone-card .desc { display: block; color: #6e7681; font-size: 0.75rem; margin-top: 0.25rem; }
.stats { color: #8b949e; font-size: 0.875rem; margin-top: 1rem; }
.nav { margin-top: 1.5rem; }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use dq_scan::environments::Taxonomy;
    use dq_scan::helm::{ChartDependency, ChartType};
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn render_empty() {
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
        let html = render(&topo).unwrap();
        assert!(html.contains("No chart data available"));
    }

    #[test]
    fn render_with_deps() {
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
            appsets: vec![],
            charts,
            config_paths: vec![],
            edges,
            taxonomy: Taxonomy {
                tenants: BTreeSet::new(),
                environments: BTreeSet::new(),
                cloud_providers: BTreeSet::new(),
                regions: BTreeMap::new(),
            },
        };
        let html = render(&topo).unwrap();
        assert!(html.contains("monitoring"));
        assert!(html.contains("prometheus"));
        assert!(html.contains("external"));
        assert!(html.contains("Standalone charts"));
        assert!(html.contains("web"));
    }
}
