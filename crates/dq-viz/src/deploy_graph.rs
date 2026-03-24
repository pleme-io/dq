//! AppSet-to-Chart deployment graph visualization.
//!
//! Generates a grouped list view showing which ApplicationSets deploy
//! each Helm chart, with colored badges indicating generator type.

use std::collections::BTreeMap;

use anyhow::Result;
use dq_scan::topology::{Edge, Topology};

use crate::html;

/// Badge color for each generator type.
fn generator_badge(gen_type: &str) -> (&'static str, &'static str) {
    match gen_type {
        "cluster" => ("#1f6feb", "cluster"),
        "git" => ("#238636", "git"),
        "matrix" => ("#8957e5", "matrix"),
        "list" => ("#da3633", "list"),
        _ => ("#6e7681", "unknown"),
    }
}

/// Render the AppSet-to-Chart deployment graph as a self-contained HTML page.
pub fn render(topology: &Topology) -> Result<String> {
    // Build chart -> [(appset_name, generator_type)] mapping
    let mut chart_to_appsets: BTreeMap<&str, Vec<(&str, String)>> = BTreeMap::new();
    let mut unlinked_appsets: Vec<(&str, String)> = Vec::new();

    // Index appsets by name for quick lookup
    let appset_map: BTreeMap<&str, &dq_scan::argocd::AppSetInfo> = topology
        .appsets
        .iter()
        .map(|a| (a.name.as_str(), a))
        .collect();

    // Track which appsets have at least one chart edge
    let mut linked_appsets: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();

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

    // Find appsets with no chart link
    for appset in &topology.appsets {
        if !linked_appsets.contains(appset.name.as_str()) {
            unlinked_appsets.push((appset.name.as_str(), appset.generator_type.to_string()));
        }
    }

    // Count value file edges per appset
    let mut appset_vf_count: BTreeMap<&str, usize> = BTreeMap::new();
    for edge in &topology.edges {
        if let Edge::AppSetToValueFile { appset, .. } = edge {
            *appset_vf_count.entry(appset.as_str()).or_insert(0) += 1;
        }
    }

    let mut body = String::new();
    body.push_str("<h1>Deployment Graph</h1>\n");
    body.push_str("<h2>ApplicationSets grouped by the Helm chart they deploy</h2>\n");

    // Legend
    body.push_str("<div class=\"legend\">\n");
    for (color, label) in [
        ("#1f6feb", "cluster"),
        ("#238636", "git"),
        ("#8957e5", "matrix"),
        ("#da3633", "list"),
    ] {
        body.push_str(&format!(
            "  <span class=\"badge\" style=\"background:{color};\">{label}</span>\n"
        ));
    }
    body.push_str("</div>\n");

    if chart_to_appsets.is_empty() && unlinked_appsets.is_empty() {
        body.push_str("<p>No deployment data available.</p>\n");
        return Ok(html::document("Deploy Graph - dq viz", CSS, &body));
    }

    // Charts with their deploying appsets
    body.push_str("<div class=\"card-grid\">\n");
    for (chart_name, appsets) in &chart_to_appsets {
        // Look up chart metadata
        let chart_info = topology.charts.iter().find(|c| c.name == *chart_name);
        let version = chart_info.map(|c| c.version.as_str()).unwrap_or("?");
        let description = chart_info
            .map(|c| c.description.as_str())
            .unwrap_or("");

        body.push_str("<div class=\"card\">\n");
        body.push_str(&format!(
            "  <div class=\"card-header\"><span class=\"chart-icon\">&#x2B22;</span> {}<span class=\"version\">v{}</span></div>\n",
            html::escape(chart_name),
            html::escape(version),
        ));
        if !description.is_empty() {
            body.push_str(&format!(
                "  <div class=\"card-desc\">{}</div>\n",
                html::escape(description)
            ));
        }
        body.push_str("  <div class=\"appset-list\">\n");
        for (appset_name, gen_type) in appsets {
            let (color, label) = generator_badge(gen_type);
            let vf_count = appset_vf_count
                .get(appset_name)
                .copied()
                .unwrap_or(0);
            let vf_badge = if vf_count > 0 {
                format!(
                    " <span class=\"vf-count\" title=\"{vf_count} value file template(s)\">{vf_count} vf</span>"
                )
            } else {
                String::new()
            };
            body.push_str(&format!(
                "    <div class=\"appset-row\"><span class=\"badge\" style=\"background:{color};\">{label}</span> {}{vf_badge}</div>\n",
                html::escape(appset_name),
            ));
        }
        body.push_str("  </div>\n");
        body.push_str("</div>\n");
    }
    body.push_str("</div>\n");

    // Unlinked appsets (no chart reference)
    if !unlinked_appsets.is_empty() {
        body.push_str("<h2 style=\"margin-top:2rem;\">Unlinked ApplicationSets (no chart match)</h2>\n");
        body.push_str("<div class=\"unlinked-list\">\n");
        for (name, gen_type) in &unlinked_appsets {
            let (color, label) = generator_badge(gen_type);
            body.push_str(&format!(
                "  <div class=\"appset-row\"><span class=\"badge\" style=\"background:{color};\">{label}</span> {}</div>\n",
                html::escape(name),
            ));
        }
        body.push_str("</div>\n");
    }

    // Stats
    body.push_str(&format!(
        "<p class=\"stats\">{} charts deployed by {} appsets ({} unlinked)</p>\n",
        chart_to_appsets.len(),
        linked_appsets.len(),
        unlinked_appsets.len(),
    ));

    body.push_str("<p class=\"nav\"><a href=\"index.html\">Back to index</a></p>\n");

    Ok(html::document("Deploy Graph - dq viz", CSS, &body))
}

const CSS: &str = r#"
.legend { margin-bottom: 1.5rem; display: flex; gap: 0.5rem; align-items: center; }
.legend::before { content: 'Generator types: '; color: #8b949e; font-size: 0.875rem; }
.badge { display: inline-block; padding: 0.15rem 0.5rem; border-radius: 12px; font-size: 0.7rem; color: #f0f6fc; font-weight: 600; text-transform: uppercase; letter-spacing: 0.03em; vertical-align: middle; }
.card-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(340px, 1fr)); gap: 1rem; }
.card { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 1rem; }
.card-header { font-size: 1.1rem; font-weight: 600; color: #f0f6fc; margin-bottom: 0.25rem; display: flex; align-items: center; gap: 0.5rem; }
.chart-icon { color: #3fb950; font-size: 0.9rem; }
.version { font-size: 0.75rem; color: #8b949e; font-weight: 400; margin-left: auto; }
.card-desc { font-size: 0.8rem; color: #8b949e; margin-bottom: 0.75rem; }
.appset-list { display: flex; flex-direction: column; gap: 0.35rem; }
.appset-row { font-size: 0.875rem; display: flex; align-items: center; gap: 0.5rem; padding: 0.25rem 0; }
.vf-count { font-size: 0.65rem; color: #8b949e; background: #21262d; padding: 0.1rem 0.4rem; border-radius: 8px; }
.unlinked-list { display: flex; flex-direction: column; gap: 0.35rem; padding: 0.5rem 0; }
.stats { color: #8b949e; font-size: 0.875rem; margin-top: 1.5rem; }
.nav { margin-top: 1.5rem; }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use dq_scan::argocd::{AppSetInfo, GeneratorType};
    use dq_scan::environments::Taxonomy;
    use dq_scan::helm::{ChartInfo, ChartType};
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
        assert!(html.contains("No deployment data available"));
    }

    #[test]
    fn render_with_deployment() {
        let appsets = vec![AppSetInfo {
            name: "web-v2".to_string(),
            generator_type: GeneratorType::Cluster,
            cluster_selectors: vec![],
            chart_path: Some("charts/web".to_string()),
            value_files: vec!["some/path.yaml".to_string()],
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
        let edges = vec![
            Edge::AppSetToChart {
                appset: "web-v2".to_string(),
                chart: "web".to_string(),
            },
            Edge::AppSetToValueFile {
                appset: "web-v2".to_string(),
                path: "some/path.yaml".to_string(),
            },
        ];
        let topo = Topology {
            appsets,
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
        assert!(html.contains("web-v2"));
        assert!(html.contains("v1.0.0"));
        assert!(html.contains("1 vf"));
        assert!(html.contains("1 charts deployed by 1 appsets"));
    }
}
