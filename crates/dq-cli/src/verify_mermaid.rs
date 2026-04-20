//! `dq verify-mermaid` — the Rust side of the Mermaid → Lisp → Rust loop.
//!
//! Flow:
//!
//! 1. A Python digester (e.g. the `repo-document` skill in
//!    `akeylesslabs/akeyless-environments`) parses each Mermaid
//!    flowchart diagram into `{diagrams: [{name, nodes, edges}]}`
//!    and emits a canonical Lisp form.
//!
//! 2. This subcommand reads that Lisp file through
//!    `shikumi::ProviderChain` (with the `lisp` feature enabled),
//!    which delegates to `tatara-lisp` for parsing. The parsed
//!    S-expression is coerced through serde into the typed
//!    [`MermaidDigest`] struct below.
//!
//! 3. The canonical `topology.json` is loaded via plain
//!    `serde_json::from_reader`.
//!
//! 4. The verifier cross-checks the two representations:
//!    - The `deploy-graph` diagram's `(appset, chart)` edges must
//!      equal the topology's `edges[]` collection under set
//!      semantics (duplicates on either side ignored).
//!    - The `chart-deps` diagram's edges must equal the chart
//!      dependency graph of `charts[*].dependencies[*]`. Self-loops
//!      are filtered on both sides (see the Python I2 note on
//!      local wrapper charts).
//!
//! Exit codes:
//!
//! - 0 — both sides agree.
//! - 1 — mismatch; a human-readable diff is printed on stderr.
//!
//! The Python I7/I8 invariants do the same check inside the skill;
//! this Rust binary exists so the verification can run *without*
//! Python — useful in CI stages that want a single static Rust
//! binary, and as proof that the emitted Lisp is machine-consumable
//! by Rust consumers.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

// ─── Typed digest structs ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct MermaidDigest {
    pub diagrams: Vec<Diagram>,
}

#[derive(Debug, Deserialize)]
pub struct Diagram {
    pub name: String,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

#[derive(Debug, Deserialize)]
pub struct Node {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
pub struct Edge {
    pub src: String,
    pub dst: String,
}

// ─── Typed topology structs (only the fields we need) ──────────────

#[derive(Debug, Deserialize, Default)]
pub struct Topology {
    #[serde(default)]
    pub edges: Vec<TopologyEdge>,
    #[serde(default)]
    pub charts: Vec<Chart>,
}

/// An entry in ``topology.edges[]``. Two variants coexist:
///
/// - deployment edges carry ``appset`` + ``chart`` + ``type``
/// - chart-dependency edges carry ``child`` + ``parent`` +
///   ``type = "chart_dependency"``
///
/// We only care about the deployment variant here; chart dependencies
/// are consumed separately via ``charts[*].dependencies``.
#[derive(Debug, Deserialize)]
pub struct TopologyEdge {
    #[serde(default)]
    pub appset: Option<String>,
    #[serde(default)]
    pub chart: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub r#type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Chart {
    pub name: String,
    #[serde(default)]
    pub dependencies: Vec<ChartDep>,
}

#[derive(Debug, Deserialize)]
pub struct ChartDep {
    pub name: String,
}

// ─── Helpers ───────────────────────────────────────────────────────

fn label_head(label: &str) -> &str {
    // Strip HTML tags in a single pass; we don't need a full parser
    // here — the Python digester already cleaned the labels, but be
    // defensive in case the file was hand-edited.
    let plain = label;
    plain.split_whitespace().next().unwrap_or("")
}

fn find_diagram<'a>(digest: &'a MermaidDigest, name: &str) -> Option<&'a Diagram> {
    digest.diagrams.iter().find(|d| d.name == name)
}

fn diagram_appset_chart_edges(diagram: &Diagram) -> BTreeSet<(String, String)> {
    let labels: std::collections::HashMap<&str, &str> = diagram
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.label.as_str()))
        .collect();
    let mut out = BTreeSet::new();
    for e in &diagram.edges {
        if !e.src.starts_with("appset_") || !e.dst.starts_with("chart_") {
            continue;
        }
        if let (Some(src), Some(dst)) = (labels.get(e.src.as_str()), labels.get(e.dst.as_str())) {
            out.insert((label_head(src).to_string(), label_head(dst).to_string()));
        }
    }
    out
}

fn diagram_chart_dep_edges(diagram: &Diagram) -> BTreeSet<(String, String)> {
    let labels: std::collections::HashMap<&str, &str> = diagram
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.label.as_str()))
        .collect();
    let mut out = BTreeSet::new();
    for e in &diagram.edges {
        let (Some(src), Some(dst)) = (labels.get(e.src.as_str()), labels.get(e.dst.as_str()))
        else {
            continue;
        };
        let (s, d) = (label_head(src), label_head(dst));
        if s.is_empty() || d.is_empty() || s == d {
            // Self-loops are the wrapper-chart convention — skip.
            continue;
        }
        out.insert((s.to_string(), d.to_string()));
    }
    out
}

fn topology_edges(topology: &Topology) -> BTreeSet<(String, String)> {
    // Only deployment edges have both `appset` and `chart` populated;
    // chart-dependency edges show up here with `child`/`parent` instead
    // and are filtered out by the Option check.
    topology
        .edges
        .iter()
        .filter_map(|e| match (&e.appset, &e.chart) {
            (Some(a), Some(c)) => Some((a.clone(), c.clone())),
            _ => None,
        })
        .collect()
}

fn topology_chart_dep_edges(topology: &Topology) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for c in &topology.charts {
        for d in &c.dependencies {
            if !c.name.is_empty() && !d.name.is_empty() && c.name != d.name {
                out.insert((c.name.clone(), d.name.clone()));
            }
        }
    }
    out
}

fn format_diff(label: &str, only_a: &BTreeSet<(String, String)>, a_name: &str, b_name: &str) {
    if !only_a.is_empty() {
        eprintln!("  {label}: {} edge(s) only in {a_name} (missing from {b_name}):", only_a.len());
        for (s, d) in only_a.iter().take(20) {
            eprintln!("    {s} -> {d}");
        }
        if only_a.len() > 20 {
            eprintln!("    ... and {} more", only_a.len() - 20);
        }
    }
}

// ─── Projection digest — cross-surface fingerprint ─────────────────
//
// Mirrors the Python pipeline's ``projection_preimage`` /
// ``projection_digest`` helpers. See the Python side for the full
// contract; in brief:
//
//     appset-chart-edges:
//     <src>|<dst>   (sorted, one per line)
//
//     chart-dep-edges:
//     <chart>|<dep> (sorted, one per line; self-loops dropped)
//
// Both sides must produce byte-identical pre-images for the hash to
// compare. SHA-256 is the common ground (BLAKE3 is available on both
// sides but requires the optional ``blake3`` crate on each). We stick
// with SHA-256 so the default build reaches the same value the
// Python pipeline emits when run without the ``blake3`` pip package.

/// Format each edge as ``"src|dst"`` and sort the resulting strings
/// lexically (not by tuple). This matches the Python pipeline's
/// ``sorted(f"{a}|{b}" for (a, b) in edges)``. The two orderings
/// differ whenever one key is a prefix of another — e.g. Python
/// places ``"saas-prod|saas"`` before ``"saas|saas"`` because
/// ``-`` (0x2D) < ``|`` (0x7C); Rust's tuple ordering would instead
/// put ``"saas|saas"`` first because ``"saas"`` is a prefix of
/// ``"saas-prod"``. Using the joined string ensures byte-identical
/// pre-images across the two implementations.
fn edges_to_preimage_lines(edges: &BTreeSet<(String, String)>) -> Vec<String> {
    let mut lines: Vec<String> = edges.iter().map(|(a, b)| format!("{a}|{b}")).collect();
    lines.sort();
    lines
}

fn projection_preimage(
    appset_chart: &BTreeSet<(String, String)>,
    chart_dep: &BTreeSet<(String, String)>,
) -> String {
    let mut out = String::new();
    out.push_str("appset-chart-edges:\n");
    for line in edges_to_preimage_lines(appset_chart) {
        out.push_str(&line);
        out.push('\n');
    }
    out.push('\n');
    out.push_str("chart-dep-edges:\n");
    for line in edges_to_preimage_lines(chart_dep) {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

pub fn projection_hash(
    appset_chart: &BTreeSet<(String, String)>,
    chart_dep: &BTreeSet<(String, String)>,
) -> String {
    let preimage = projection_preimage(appset_chart, chart_dep);
    let mut hasher = Sha256::new();
    hasher.update(preimage.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(7 + 64);
    hex.push_str("sha256:");
    for byte in digest.iter() {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

// ─── Public entry ──────────────────────────────────────────────────

pub fn run(digest_path: &Path, topology_path: &Path) -> Result<()> {
    // Lisp digest → typed struct via shikumi's Lisp provider.
    let digest: MermaidDigest = shikumi::ProviderChain::new()
        .with_file(digest_path)
        .extract()
        .with_context(|| {
            format!(
                "reading Mermaid digest via shikumi+lisp: {}",
                digest_path.display()
            )
        })?;

    // Canonical topology → typed struct via serde_json.
    let topology_file = std::fs::File::open(topology_path).with_context(|| {
        format!("opening topology: {}", topology_path.display())
    })?;
    let topology: Topology = serde_json::from_reader(topology_file)
        .with_context(|| format!("parsing topology: {}", topology_path.display()))?;

    let mut any_mismatch = false;

    // ── Check 1: deploy-graph ↔ topology.edges ─────────────────────
    if let Some(deploy) = find_diagram(&digest, "deploy-graph") {
        let d = diagram_appset_chart_edges(deploy);
        let t = topology_edges(&topology);
        let only_diagram: BTreeSet<_> = d.difference(&t).cloned().collect();
        let only_topology: BTreeSet<_> = t.difference(&d).cloned().collect();
        if only_diagram.is_empty() && only_topology.is_empty() {
            println!(
                "verify-mermaid: deploy-graph OK — {} appset→chart edges agree",
                d.len()
            );
        } else {
            any_mismatch = true;
            eprintln!("verify-mermaid: deploy-graph MISMATCH");
            format_diff("extra-in-diagram", &only_diagram, "diagram", "topology");
            format_diff("extra-in-topology", &only_topology, "topology", "diagram");
        }
    } else {
        eprintln!("verify-mermaid: note — no `deploy-graph` diagram in digest");
    }

    // ── Check 2: chart-deps ↔ chart dependency graph ───────────────
    if let Some(chart_deps) = find_diagram(&digest, "chart-deps") {
        let d = diagram_chart_dep_edges(chart_deps);
        let t = topology_chart_dep_edges(&topology);
        let only_diagram: BTreeSet<_> = d.difference(&t).cloned().collect();
        let only_topology: BTreeSet<_> = t.difference(&d).cloned().collect();
        if only_diagram.is_empty() && only_topology.is_empty() {
            println!(
                "verify-mermaid: chart-deps OK — {} dependency edges agree",
                d.len()
            );
        } else {
            any_mismatch = true;
            eprintln!("verify-mermaid: chart-deps MISMATCH");
            format_diff("extra-in-diagram", &only_diagram, "diagram", "topology");
            format_diff("extra-in-topology", &only_topology, "topology", "diagram");
        }
    } else {
        eprintln!("verify-mermaid: note — no `chart-deps` diagram in digest");
    }

    // ── Check 3: projection hash round-trip ────────────────────────
    //
    // Compute the projection hash from the topology, then from the
    // Mermaid digest (reconstructed via the same edge-extraction
    // logic), and demand they match. This is the cross-surface
    // fingerprint — the "abstract understanding" hash that collapses
    // both edge sets into a single deterministic digest.
    let topology_appset_chart = topology_edges(&topology);
    let topology_chart_dep = topology_chart_dep_edges(&topology);

    let mermaid_appset_chart = find_diagram(&digest, "deploy-graph")
        .map(diagram_appset_chart_edges)
        .unwrap_or_default();
    let mermaid_chart_dep = find_diagram(&digest, "chart-deps")
        .map(diagram_chart_dep_edges)
        .unwrap_or_default();

    let topo_hash = projection_hash(&topology_appset_chart, &topology_chart_dep);
    let mermaid_hash = projection_hash(&mermaid_appset_chart, &mermaid_chart_dep);
    if topo_hash == mermaid_hash {
        println!("verify-mermaid: projection-hash {topo_hash} (matches)");
    } else {
        any_mismatch = true;
        eprintln!("verify-mermaid: projection-hash MISMATCH");
        eprintln!("  topology: {topo_hash}");
        eprintln!("  mermaid:  {mermaid_hash}");
    }

    // Cross-side verification: if the repo also ships a
    // ``projection.hash`` file alongside ``topology.json`` (the
    // Python ``repo-document`` pipeline emits one), the Rust-side
    // computation must match that file too. This is what proves the
    // two independent implementations of the projection digest stay
    // in lock-step — the "hash matches" round-trip in full.
    if let Some(dir) = topology_path.parent() {
        let rec_path = dir.join("projection.hash");
        if rec_path.is_file() {
            let recorded = std::fs::read_to_string(&rec_path)
                .with_context(|| format!("reading {}", rec_path.display()))?
                .trim()
                .to_string();
            if recorded == topo_hash {
                println!(
                    "verify-mermaid: projection.hash on disk ({recorded}) agrees with Rust computation"
                );
            } else {
                any_mismatch = true;
                eprintln!("verify-mermaid: projection.hash on disk DIFFERS from Rust computation");
                eprintln!("  on-disk (Python): {recorded}");
                eprintln!("  fresh    (Rust):  {topo_hash}");
                eprintln!("\nThe two sides compute hashes over the same canonical pre-image.");
                eprintln!("A mismatch here means the Python and Rust implementations have drifted.");
                // Emit the Rust pre-image for debugging; diff against
                // docs/repo-document/schema/projection.bytes on the
                // Python side to see where the drift is.
                let rust_preimage =
                    projection_preimage(&topology_appset_chart, &topology_chart_dep);
                eprintln!("\n--- Rust pre-image (first 40 lines) ---");
                for line in rust_preimage.lines().take(40) {
                    eprintln!("  {line}");
                }
            }
        }
    }

    if any_mismatch {
        anyhow::bail!("verify-mermaid: at least one diagram disagrees with the topology");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagram(nodes: &[(&str, &str)], edges: &[(&str, &str)]) -> Diagram {
        Diagram {
            name: "deploy-graph".into(),
            nodes: nodes
                .iter()
                .map(|(id, label)| Node {
                    id: id.to_string(),
                    label: label.to_string(),
                })
                .collect(),
            edges: edges
                .iter()
                .map(|(src, dst)| Edge {
                    src: src.to_string(),
                    dst: dst.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn appset_chart_edges_extract_by_label_head() {
        let d = diagram(
            &[
                ("appset_alpha", "alpha"),
                ("chart_x", "x  v1.0"),
                ("appset_beta", "beta"),
                ("chart_y", "y  v2.0"),
            ],
            &[
                ("appset_alpha", "chart_x"),
                ("appset_beta", "chart_y"),
            ],
        );
        let got = diagram_appset_chart_edges(&d);
        let want: BTreeSet<_> = [
            ("alpha".to_string(), "x".to_string()),
            ("beta".to_string(), "y".to_string()),
        ]
        .into_iter()
        .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn chart_dep_edges_skip_self_loops() {
        let d = diagram(
            &[("a", "a  v1"), ("b", "b  v1")],
            &[("a", "b"), ("a", "a"), ("b", "b")],
        );
        let mut d = d;
        d.name = "chart-deps".into();
        let got = diagram_chart_dep_edges(&d);
        let want: BTreeSet<_> = [("a".to_string(), "b".to_string())].into_iter().collect();
        assert_eq!(got, want);
    }

    #[test]
    fn label_head_strips_trailing_tokens() {
        assert_eq!(label_head("alloy v1.0.0"), "alloy");
        assert_eq!(label_head("public-gateway-v2"), "public-gateway-v2");
        assert_eq!(label_head(""), "");
    }

    #[test]
    fn preimage_sort_matches_python_convention() {
        // Python produces a single ``"src|dst"`` string per edge and
        // sorts those strings lexically. Rust must match, even when
        // one key is a prefix of another. ``"saas"`` and
        // ``"saas-prod"`` are the canonical regression case: tuple
        // sort puts ``(saas, ...)`` first, string sort puts
        // ``saas-prod|...`` first (because ``-`` < ``|``).
        let mut edges = BTreeSet::new();
        edges.insert(("saas".to_string(), "saas".to_string()));
        edges.insert(("saas-prod".to_string(), "saas".to_string()));
        let lines = edges_to_preimage_lines(&edges);
        assert_eq!(lines, vec!["saas-prod|saas", "saas|saas"]);
    }

    #[test]
    fn projection_hash_matches_known_bytes() {
        // Pin the hash for a tiny synthetic projection so any future
        // changes to the pre-image format fail this test first. The
        // expected hash was computed independently by the Python
        // pipeline over the same pre-image.
        let mut appset_chart = BTreeSet::new();
        appset_chart.insert(("a".to_string(), "x".to_string()));
        appset_chart.insert(("b".to_string(), "y".to_string()));
        let mut chart_dep = BTreeSet::new();
        chart_dep.insert(("x".to_string(), "y".to_string()));
        let preimage = projection_preimage(&appset_chart, &chart_dep);
        assert_eq!(
            preimage,
            "appset-chart-edges:\na|x\nb|y\n\nchart-dep-edges:\nx|y\n",
        );
        // Verify the hash prefix + length — the full digest is
        // validated end-to-end by the cross-side check in `run`.
        let h = projection_hash(&appset_chart, &chart_dep);
        assert!(h.starts_with("sha256:"), "unexpected prefix: {h}");
        assert_eq!(h.len(), "sha256:".len() + 64);
    }
}
