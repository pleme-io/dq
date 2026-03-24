//! HTML document utilities.
//!
//! Provides helpers for generating self-contained HTML pages with the
//! GitHub-dark color scheme used across all visualizations.

/// Wrap body content in a complete HTML document with inline styles.
///
/// All generated pages share a common base style (dark theme, system fonts)
/// with per-page CSS injected via the `css` parameter.
pub fn document(title: &str, css: &str, body: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif; background: #0d1117; color: #c9d1d9; padding: 2rem; }}
h1 {{ color: #f0f6fc; margin-bottom: 0.5rem; }}
h2 {{ color: #8b949e; font-weight: 400; font-size: 1rem; margin-bottom: 2rem; }}
a {{ color: #58a6ff; text-decoration: none; }}
a:hover {{ text-decoration: underline; }}
{css}
</style>
</head>
<body>
{body}
</body>
</html>"#
    )
}

/// Escape HTML special characters in a string.
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
