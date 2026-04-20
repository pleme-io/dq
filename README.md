# dq — universal infrastructure data query tool

[![release](https://github.com/pleme-io/dq/actions/workflows/release.yml/badge.svg)](https://github.com/pleme-io/dq/actions/workflows/release.yml)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](./LICENSE)

Rust workspace for querying, scanning, merging, and visualizing infrastructure configurations across GitOps repositories. Parses YAML, JSON, HCL, and TOML; emits HTML dashboards and Mermaid diagrams.

## Install

### Prebuilt binaries

Every tagged release publishes binaries for four targets on [GitHub Releases](https://github.com/pleme-io/dq/releases):

| Target | Binary |
|--------|--------|
| Apple Silicon macOS | `dq-darwin-arm64` |
| Intel macOS | `dq-darwin-x86_64` |
| ARM64 Linux | `dq-linux-arm64` |
| x86_64 Linux | `dq-linux-x86_64` |

Each binary has a `.sha256` sidecar. Download, verify, `chmod +x`, move onto `PATH`.

### Nix

```bash
nix run github:pleme-io/dq          # default build, slim dep tree
nix run github:pleme-io/dq#dq-full  # with lisp feature (verify-mermaid)
```

### From source

```bash
cargo install --git https://github.com/pleme-io/dq --bin dq --features lisp
```

## Usage

```bash
dq scan .                          # scan GitOps repo, build topology
dq scan viz                        # emit HTML dashboards (matrix, deploy graph, chart deps)
dq scan viz --format mermaid       # GitHub-native Markdown with embedded Mermaid
dq query <file> <path>             # extract values from a parsed config
dq merge <base> <overlay>...       # deep-merge config overlays
```

Configure scan behavior via `.dq.yaml` at the repo root (chart paths, appset discovery, tenant/environment patterns).

## Crate layout

| Crate | Purpose |
|-------|---------|
| `dq-cli` | CLI entry point (`dq` binary) |
| `dq-core` | Core data model and configuration |
| `dq-formats` | Format detection and parsing (YAML/JSON/HCL/TOML) |
| `dq-merge` | Deep-merge strategies for config overlays |
| `dq-query` | Query engine for extracting values |
| `dq-scan` | GitOps repository scanner (topology, charts, appsets) |
| `dq-terragrunt` | Terragrunt HCL parsing + dependency resolution |
| `dq-viz` | Static HTML + Mermaid visualization generator |

## Building

```bash
cargo build                 # debug build of every workspace member
cargo test --lib            # unit tests
nix build .                 # hermetic crate2nix build (default: slim)
nix build .#dq-full         # build with `lisp` feature
```

## License

MIT. See [LICENSE](./LICENSE).
