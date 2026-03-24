# dq — Universal Infrastructure Data Query Tool

Rust workspace for querying, scanning, merging, and visualizing infrastructure
configurations across GitOps repositories.

## Build & Test

```bash
cargo build
cargo test --lib
```

## Crate Structure

| Crate | Purpose |
|-------|---------|
| `dq-cli` | CLI entry point (`dq` binary) |
| `dq-core` | Core data model and configuration |
| `dq-formats` | Format detection and parsing (YAML, JSON, HCL, TOML) |
| `dq-merge` | Deep merge strategies for config overlays |
| `dq-query` | Query engine for extracting values from parsed configs |
| `dq-scan` | GitOps repository scanner (topology, charts, appsets) |
| `dq-terragrunt` | Terragrunt HCL parsing and dependency resolution |
| `dq-viz` | Static HTML and Mermaid visualization generator |

## CLI Commands

| Command | Purpose |
|---------|---------|
| `dq scan` | Scan a GitOps repository and build topology |
| `dq scan viz` | Generate static HTML visualizations from scan topology |
| `dq scan viz --format mermaid` | Generate Mermaid/markdown output for GitHub-native rendering |
| `dq query` | Query configuration values from parsed files |
| `dq merge` | Deep merge multiple config files |

## Visualization Output (`dq scan viz`)

The `dq-viz` crate generates self-contained HTML files with no external dependencies:

| Page | Content |
|------|---------|
| `index.html` | Landing page linking to all visualizations |
| `matrix.html` | Tenant x Environment x Cloud heatmap of config file counts |
| `deploy_graph.html` | ApplicationSet-to-Chart deployment mapping with generator badges |
| `chart_deps.html` | Chart dependency tree with version information and cycle detection |

With `--format mermaid`, output is Markdown with embedded Mermaid diagrams
suitable for direct rendering in GitHub PRs and READMEs.

## Configuration

`.dq.yaml` at repo root configures scan behavior (chart paths, appset discovery,
tenant/environment patterns).

## Nix Integration

- **flake.nix** — substrate Rust build with crate2nix
- **nixpkgs 25.11** — requires Rust 1.87+ (serde-saphyr dependency)
