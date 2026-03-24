//! Tenant x Environment x Cloud heatmap visualization.
//!
//! Generates an HTML table where:
//! - Rows = tenants
//! - Column groups = cloud providers, sub-columns = environments
//! - Cells = count of config files for that tenant/env/cloud combination
//! - Color intensity scales with the count

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use dq_scan::topology::Topology;

use crate::html;

/// Cell intensity thresholds for the heatmap color scale.
fn cell_color(count: usize) -> &'static str {
    match count {
        0 => "background: #161b22;",
        1..=5 => "background: #0e4429; color: #7ee787;",
        6..=20 => "background: #006d32; color: #f0f6fc;",
        _ => "background: #26a641; color: #f0f6fc; font-weight: 600;",
    }
}

/// Render the tenant-environment-cloud heatmap as a self-contained HTML page.
pub fn render(topology: &Topology) -> Result<String> {
    // Collect counts: (tenant, environment, cloud_provider) -> count
    let mut counts: BTreeMap<(&str, &str, &str), usize> = BTreeMap::new();
    for cp in &topology.config_paths {
        *counts
            .entry((&cp.tenant, &cp.environment, &cp.cloud_provider))
            .or_insert(0) += 1;
    }

    // Build sorted sets for axes
    let tenants: BTreeSet<&str> = topology.taxonomy.tenants.iter().map(|s| s.as_str()).collect();
    let clouds: BTreeSet<&str> = topology
        .taxonomy
        .cloud_providers
        .iter()
        .map(|s| s.as_str())
        .collect();
    let envs: BTreeSet<&str> = topology
        .taxonomy
        .environments
        .iter()
        .map(|s| s.as_str())
        .collect();

    let env_list: Vec<&str> = envs.iter().copied().collect();
    let cloud_list: Vec<&str> = clouds.iter().copied().collect();

    let mut body = String::new();
    body.push_str("<h1>Tenant / Environment / Cloud Matrix</h1>\n");
    body.push_str("<h2>Config file counts per tenant, environment, and cloud provider</h2>\n");

    // Legend
    body.push_str("<div class=\"legend\">\n");
    body.push_str("  <span class=\"legend-item\" style=\"background:#161b22;\">0</span>\n");
    body.push_str(
        "  <span class=\"legend-item\" style=\"background:#0e4429;color:#7ee787;\">1-5</span>\n",
    );
    body.push_str(
        "  <span class=\"legend-item\" style=\"background:#006d32;color:#f0f6fc;\">6-20</span>\n",
    );
    body.push_str(
        "  <span class=\"legend-item\" style=\"background:#26a641;color:#f0f6fc;\">21+</span>\n",
    );
    body.push_str("</div>\n");

    if tenants.is_empty() || clouds.is_empty() || envs.is_empty() {
        body.push_str("<p>No environment data available.</p>\n");
        return Ok(html::document("Matrix - dq viz", CSS, &body));
    }

    body.push_str("<div class=\"table-wrap\">\n");
    body.push_str("<table>\n");

    // Header row 1: cloud provider group headers
    body.push_str("<thead>\n<tr><th rowspan=\"2\" class=\"tenant-col\">Tenant</th>\n");
    for cloud in &cloud_list {
        body.push_str(&format!(
            "<th colspan=\"{}\" class=\"cloud-header\">{}</th>\n",
            env_list.len(),
            html::escape(cloud)
        ));
    }
    body.push_str("</tr>\n");

    // Header row 2: environment sub-headers
    body.push_str("<tr>\n");
    for _cloud in &cloud_list {
        for env in &env_list {
            body.push_str(&format!(
                "<th class=\"env-header\">{}</th>\n",
                html::escape(env)
            ));
        }
    }
    body.push_str("</tr>\n</thead>\n");

    // Data rows
    body.push_str("<tbody>\n");
    for tenant in &tenants {
        body.push_str("<tr>\n");
        body.push_str(&format!(
            "<td class=\"tenant-cell\">{}</td>\n",
            html::escape(tenant)
        ));
        for cloud in &cloud_list {
            for env in &env_list {
                let count = counts.get(&(*tenant, *env, *cloud)).copied().unwrap_or(0);
                let style = cell_color(count);
                let display = if count == 0 {
                    String::from("-")
                } else {
                    count.to_string()
                };
                body.push_str(&format!(
                    "<td class=\"count-cell\" style=\"{style}\" title=\"{t} / {e} / {c}: {n} files\">{display}</td>\n",
                    t = html::escape(tenant),
                    e = html::escape(env),
                    c = html::escape(cloud),
                    n = count,
                ));
            }
        }
        body.push_str("</tr>\n");
    }
    body.push_str("</tbody>\n");

    // Summary row
    body.push_str("<tfoot>\n<tr><td class=\"tenant-cell\"><strong>Total</strong></td>\n");
    for cloud in &cloud_list {
        for env in &env_list {
            let total: usize = tenants
                .iter()
                .map(|t| counts.get(&(*t, *env, *cloud)).copied().unwrap_or(0))
                .sum();
            let style = cell_color(total);
            let display = if total == 0 {
                String::from("-")
            } else {
                total.to_string()
            };
            body.push_str(&format!(
                "<td class=\"count-cell\" style=\"{style}\"><strong>{display}</strong></td>\n"
            ));
        }
    }
    body.push_str("</tr>\n</tfoot>\n");

    body.push_str("</table>\n</div>\n");

    // Summary stats
    body.push_str(&format!(
        "<p class=\"stats\">{} tenants, {} environments, {} cloud providers, {} config files</p>\n",
        tenants.len(),
        envs.len(),
        clouds.len(),
        topology.config_paths.len(),
    ));

    body.push_str("<p class=\"nav\"><a href=\"index.html\">Back to index</a></p>\n");

    Ok(html::document("Matrix - dq viz", CSS, &body))
}

const CSS: &str = r#"
.legend { margin-bottom: 1.5rem; display: flex; gap: 0.5rem; align-items: center; }
.legend::before { content: 'Scale: '; color: #8b949e; font-size: 0.875rem; }
.legend-item { padding: 0.25rem 0.75rem; border-radius: 4px; font-size: 0.8rem; border: 1px solid #30363d; }
.table-wrap { overflow-x: auto; margin-bottom: 1.5rem; }
table { border-collapse: collapse; min-width: 100%; }
th, td { padding: 0.5rem 0.75rem; border: 1px solid #30363d; text-align: center; white-space: nowrap; }
.tenant-col { text-align: left; position: sticky; left: 0; background: #0d1117; z-index: 1; }
.tenant-cell { text-align: left; font-weight: 500; position: sticky; left: 0; background: #0d1117; z-index: 1; }
.cloud-header { background: #161b22; color: #58a6ff; font-size: 0.875rem; text-transform: uppercase; letter-spacing: 0.05em; }
.env-header { background: #161b22; color: #8b949e; font-size: 0.75rem; }
.count-cell { font-size: 0.875rem; min-width: 3rem; cursor: default; }
tfoot td { border-top: 2px solid #58a6ff; }
.stats { color: #8b949e; font-size: 0.875rem; margin-top: 1rem; }
.nav { margin-top: 1.5rem; }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use dq_scan::environments::{ConfigPath, Taxonomy};
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    #[test]
    fn render_empty_topology() {
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
        assert!(html.contains("No environment data available"));
    }

    #[test]
    fn render_single_cell() {
        let config_paths = vec![ConfigPath {
            tenant: "acme".to_string(),
            environment: "production".to_string(),
            cloud_provider: "AWS".to_string(),
            region: Some("us-east-1".to_string()),
            file_type: "helm_values".to_string(),
            path: PathBuf::from("/repo/environments/acme/production/AWS/helm_values_files/us-east-1/values.yaml"),
        }];
        let mut regions = BTreeMap::new();
        regions.insert("AWS".to_string(), BTreeSet::from(["us-east-1".to_string()]));
        let topo = Topology {
            appsets: vec![],
            charts: vec![],
            config_paths,
            edges: vec![],
            taxonomy: Taxonomy {
                tenants: BTreeSet::from(["acme".to_string()]),
                environments: BTreeSet::from(["production".to_string()]),
                cloud_providers: BTreeSet::from(["AWS".to_string()]),
                regions,
            },
        };
        let html = render(&topo).unwrap();
        assert!(html.contains("acme"));
        assert!(html.contains("AWS"));
        assert!(html.contains("production"));
        assert!(html.contains("1 tenants"));
    }
}
