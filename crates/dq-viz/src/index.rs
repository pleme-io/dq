//! Index page generator.
//!
//! Produces a landing page that links to all generated visualization files
//! with descriptions of each view.

use crate::html;

/// Known visualization metadata: (filename, title, description).
const PAGES: &[(&str, &str, &str)] = &[
    (
        "matrix.html",
        "Tenant / Environment / Cloud Matrix",
        "Heatmap of config file counts per tenant, environment, and cloud provider combination.",
    ),
    (
        "deploy-graph.html",
        "Deployment Graph",
        "ApplicationSets grouped by the Helm chart they deploy, with generator type badges.",
    ),
    (
        "chart-deps.html",
        "Chart Dependencies",
        "Helm chart dependency tree showing parent/child relationships and external dependencies.",
    ),
];

/// Render the index page linking to all generated visualizations.
pub fn render(generated: &[String]) -> String {
    let mut body = String::new();
    body.push_str("<h1>dq viz</h1>\n");
    body.push_str("<h2>GitOps topology visualizations</h2>\n");

    body.push_str("<div class=\"card-list\">\n");
    for (filename, title, description) in PAGES {
        if generated.iter().any(|g| g == filename) {
            body.push_str(&format!(
                "<a href=\"{filename}\" class=\"card\">\n  <div class=\"card-title\">{title}</div>\n  <div class=\"card-desc\">{description}</div>\n</a>\n",
            ));
        }
    }
    body.push_str("</div>\n");

    body.push_str(&format!(
        "<p class=\"stats\">{} visualizations generated</p>\n",
        generated.len()
    ));

    html::document("dq viz", CSS, &body)
}

const CSS: &str = r#"
.card-list { display: flex; flex-direction: column; gap: 1rem; max-width: 640px; }
.card { display: block; background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 1.25rem; transition: border-color 0.15s; }
.card:hover { border-color: #58a6ff; text-decoration: none; }
.card-title { font-size: 1.1rem; font-weight: 600; color: #f0f6fc; margin-bottom: 0.35rem; }
.card-desc { font-size: 0.875rem; color: #8b949e; }
.stats { color: #484f58; font-size: 0.8rem; margin-top: 2rem; }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_all_pages() {
        let generated = vec![
            "matrix.html".to_string(),
            "deploy-graph.html".to_string(),
            "chart-deps.html".to_string(),
        ];
        let html = render(&generated);
        assert!(html.contains("dq viz"));
        assert!(html.contains("matrix.html"));
        assert!(html.contains("deploy-graph.html"));
        assert!(html.contains("chart-deps.html"));
        assert!(html.contains("3 visualizations generated"));
    }

    #[test]
    fn render_partial() {
        let generated = vec!["matrix.html".to_string()];
        let html = render(&generated);
        assert!(html.contains("matrix.html"));
        assert!(!html.contains("deploy-graph.html"));
    }
}
