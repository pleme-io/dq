//! Static HTML visualization generator for dq scan topology data.
//!
//! Generates self-contained HTML files from a scanned GitOps repository
//! topology. Each visualization is a standalone page with inline CSS and JS,
//! requiring no external dependencies.
//!
//! ## Visualizations
//!
//! - **matrix** -- Tenant x Environment x Cloud heatmap of config file counts.
//! - **deploy_graph** -- AppSet-to-Chart deployment mapping with generator badges.
//! - **chart_deps** -- Chart dependency tree with version information.
//! - **index** -- Landing page linking to all generated visualizations.

pub mod chart_deps;
pub mod deploy_graph;
mod html;
pub mod index;
pub mod matrix;

use std::path::Path;

use anyhow::Result;
use dq_scan::topology::Topology;

/// Generate all visualizations from a pre-built topology.
///
/// Writes self-contained HTML files into `output_dir` and returns the list
/// of filenames generated.
pub fn generate_from_topology(topology: &Topology, output_dir: &Path) -> Result<Vec<String>> {
    std::fs::create_dir_all(output_dir)?;

    let mut generated = Vec::new();

    // 1. Tenant-Environment-Cloud Matrix
    let content = matrix::render(topology)?;
    let path = output_dir.join("matrix.html");
    std::fs::write(&path, &content)?;
    generated.push("matrix.html".to_string());

    // 2. Deployment Graph (AppSet -> Chart)
    let content = deploy_graph::render(topology)?;
    let path = output_dir.join("deploy-graph.html");
    std::fs::write(&path, &content)?;
    generated.push("deploy-graph.html".to_string());

    // 3. Chart Dependencies
    let content = chart_deps::render(topology)?;
    let path = output_dir.join("chart-deps.html");
    std::fs::write(&path, &content)?;
    generated.push("chart-deps.html".to_string());

    // 4. Index page
    let content = index::render(&generated);
    std::fs::write(output_dir.join("index.html"), &content)?;

    Ok(generated)
}

/// Scan a repository root and generate all visualizations.
///
/// This is the main entry point: it runs the scan pipeline internally
/// and then generates HTML output.
pub fn generate_all(root: &Path, output_dir: &Path) -> Result<Vec<String>> {
    let result = dq_scan::scan_directory(root)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    generate_from_topology(&result.topology, output_dir)
}
