//! # dq — Universal Infrastructure Data Query Tool
//!
//! Single binary for all data manipulation:
//!   dq '.dependencies | keys' config.yaml
//!   dq -f hcl -t json main.tf
//!   dq merge base.yaml override.yaml
//!   dq tg dag .
//!   dq tg render modules/vpc
//!   dq scan topology /path/to/repo
//!   dq flatten --separator '.' values.yaml
//!   dq diff staging.yaml production.yaml

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "dq", about = "Universal infrastructure data query tool")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// jq expression (when no subcommand is given)
    #[arg(index = 1)]
    expression: Option<String>,

    /// Input file(s) — stdin if omitted
    #[arg(index = 2)]
    files: Vec<PathBuf>,

    /// Input format (auto-detected from extension if omitted)
    #[arg(short = 'f', long = "from")]
    from_format: Option<String>,

    /// Output format (default: json)
    #[arg(short = 't', long = "to", default_value = "json")]
    to_format: String,

    /// Raw output (no JSON quotes for strings)
    #[arg(short = 'r', long = "raw")]
    raw: bool,

    /// Compact output (no pretty-printing)
    #[arg(short = 'c', long = "compact")]
    compact: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Deep merge multiple files (left to right, last wins)
    Merge {
        /// Input files to merge
        files: Vec<PathBuf>,
        /// Merge strategy: shallow, deep, replace
        #[arg(short, long, default_value = "deep")]
        strategy: String,
        /// Output format
        #[arg(short = 't', long = "to", default_value = "json")]
        to_format: String,
    },

    /// Diff two files
    Diff {
        /// First file (base)
        a: PathBuf,
        /// Second file (changed)
        b: PathBuf,
        /// Output format
        #[arg(short = 't', long = "to", default_value = "json")]
        to_format: String,
    },

    /// Flatten nested maps to dot-separated keys
    Flatten {
        /// Input file
        file: PathBuf,
        /// Key separator
        #[arg(short, long, default_value = ".")]
        separator: String,
    },

    /// Unflatten dot-separated keys back to nested maps
    Unflatten {
        /// Input file
        file: PathBuf,
        /// Key separator
        #[arg(short, long, default_value = ".")]
        separator: String,
    },

    /// Convert between formats
    Convert {
        /// Input file
        file: PathBuf,
        /// Output format
        #[arg(short = 't', long = "to")]
        to_format: String,
    },

    /// Terragrunt operations
    #[command(name = "tg")]
    Terragrunt {
        #[command(subcommand)]
        command: TerragruntCommands,
    },

    /// Scan infrastructure repositories for topology and configuration
    Scan {
        #[command(subcommand)]
        command: ScanCommands,
    },

    /// Verify a Mermaid → Lisp digest matches a canonical topology.json.
    ///
    /// Closes the loop on the render path: `dq scan viz --format
    /// mermaid` produces diagrams, an external tool digests those
    /// diagrams back into a typed Lisp form, and this subcommand
    /// cross-checks the digest against the topology the diagrams
    /// claim to represent. Only compiled when the `lisp` feature is
    /// enabled (it pulls shikumi + tatara-lisp through the Lisp
    /// provider).
    #[cfg(feature = "lisp")]
    #[command(name = "verify-mermaid")]
    VerifyMermaid {
        /// Path to mermaid-digest.lisp (produced by the repo-document
        /// skill or any other Mermaid → Lisp digester).
        digest: PathBuf,
        /// Path to the canonical topology.json to verify against.
        topology: PathBuf,
    },
}

#[derive(Subcommand)]
enum TerragruntCommands {
    /// Build and display dependency DAG
    Dag {
        /// Root directory
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Output format: json, dot, tree
        #[arg(short = 't', long = "to", default_value = "json")]
        format: String,
    },

    /// Render a module with includes resolved
    Render {
        /// Module path
        path: PathBuf,
        /// Output format
        #[arg(short = 't', long = "to", default_value = "json")]
        format: String,
    },

    /// List all modules
    List {
        /// Root directory
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Output format: text (one path per line, default) or json
        /// (structured record per module with dependency counts).
        #[arg(short = 't', long = "to", default_value = "text")]
        format: String,
    },

    /// Show dependencies of a module
    Deps {
        /// Module path
        path: PathBuf,
        /// Include transitive dependencies
        #[arg(long)]
        transitive: bool,
        /// Root directory for DAG construction
        #[arg(short, long, default_value = ".")]
        root: PathBuf,
    },

    /// Show what depends on a module (reverse deps)
    Rdeps {
        /// Module path
        path: PathBuf,
        /// Root directory for DAG construction
        #[arg(short, long, default_value = ".")]
        root: PathBuf,
    },
}

#[derive(Subcommand)]
enum ScanCommands {
    /// List all ArgoCD ApplicationSets
    Appsets {
        /// Repository root directory
        #[arg(default_value = ".")]
        root: PathBuf,
    },

    /// List all Helm charts
    Charts {
        /// Repository root directory
        #[arg(default_value = ".")]
        root: PathBuf,
    },

    /// Map tenant/environment/cloud/region structure
    Environments {
        /// Repository root directory
        #[arg(default_value = ".")]
        root: PathBuf,
    },

    /// Full deployment topology
    Topology {
        /// Repository root directory
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Output format: json, dot, summary
        #[arg(short = 't', long = "to", default_value = "json")]
        format: String,
    },

    /// Generate static visualizations of the scanned topology
    Viz {
        /// Repository root directory
        #[arg(default_value = ".")]
        root: PathBuf,
        /// Output directory for generated files
        #[arg(short, long, default_value = "docs/dq-scans/viz")]
        output_dir: PathBuf,
        /// Output format: html or mermaid (GitHub-friendly markdown)
        #[arg(short, long, default_value = "html")]
        format: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Merge { files, strategy, to_format }) => cmd_merge(files, &strategy, &to_format),
        Some(Commands::Diff { a, b, to_format }) => cmd_diff(&a, &b, &to_format),
        Some(Commands::Flatten { file, separator }) => cmd_flatten(&file, &separator),
        Some(Commands::Unflatten { file, separator }) => cmd_unflatten(&file, &separator),
        Some(Commands::Convert { file, to_format }) => cmd_convert(&file, &to_format),
        Some(Commands::Terragrunt { command }) => cmd_terragrunt(command),
        Some(Commands::Scan { command }) => cmd_scan(command),
        #[cfg(feature = "lisp")]
        Some(Commands::VerifyMermaid { digest, topology }) => {
            verify_mermaid::run(&digest, &topology)
        }
        None => cmd_query(cli),
    }
}

#[cfg(feature = "lisp")]
mod verify_mermaid;

fn cmd_query(cli: Cli) -> Result<()> {
    let expr = cli.expression.as_deref().unwrap_or(".");

    let input_bytes = if cli.files.is_empty() {
        use std::io::Read;
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf)?;
        buf
    } else {
        std::fs::read(&cli.files[0])?
    };

    let from_format = cli.from_format.as_deref()
        .and_then(dq_formats::FormatKind::from_name)
        .or_else(|| cli.files.first().and_then(|f| {
            dq_formats::detect::detect_from_path(&f.to_string_lossy())
        }))
        .or_else(|| dq_formats::detect::detect_from_content(&input_bytes))
        .unwrap_or(dq_formats::FormatKind::Json);

    let value = dq_formats::parse(from_format, &input_bytes)?;
    let results = dq_query::query(&value, expr)?;

    let to_format = dq_formats::FormatKind::from_name(&cli.to_format)
        .unwrap_or(dq_formats::FormatKind::Json);

    for result in &results {
        if cli.raw {
            if let Some(s) = result.as_str() {
                println!("{s}");
                continue;
            }
        }
        let output = dq_formats::serialize(to_format, result)?;
        std::io::Write::write_all(&mut std::io::stdout(), &output)?;
        println!();
    }

    Ok(())
}

fn cmd_merge(files: Vec<PathBuf>, strategy: &str, to_format: &str) -> Result<()> {
    let strat = match strategy {
        "shallow" => dq_merge::Strategy::Shallow,
        "deep" => dq_merge::Strategy::Deep,
        "replace" => dq_merge::Strategy::Replace,
        other => anyhow::bail!("unknown strategy: {other}"),
    };

    let values: Vec<dq_core::Value> = files.iter()
        .map(|f| {
            let bytes = std::fs::read(f)?;
            dq_formats::parse_auto(&f.to_string_lossy(), &bytes)
                .map_err(|e| anyhow::anyhow!("{}: {e}", f.display()))
        })
        .collect::<Result<_>>()?;

    let merged = dq_merge::merge_stack(&values, strat);
    let fmt = dq_formats::FormatKind::from_name(to_format).unwrap_or(dq_formats::FormatKind::Json);
    let output = dq_formats::serialize(fmt, &merged)?;
    std::io::Write::write_all(&mut std::io::stdout(), &output)?;
    Ok(())
}

fn cmd_diff(a: &PathBuf, b: &PathBuf, to_format: &str) -> Result<()> {
    let va = dq_formats::parse_auto(&a.to_string_lossy(), &std::fs::read(a)?)?;
    let vb = dq_formats::parse_auto(&b.to_string_lossy(), &std::fs::read(b)?)?;
    let d = dq_merge::diff(&va, &vb);
    let fmt = dq_formats::FormatKind::from_name(to_format).unwrap_or(dq_formats::FormatKind::Json);
    let output = dq_formats::serialize(fmt, &d)?;
    std::io::Write::write_all(&mut std::io::stdout(), &output)?;
    Ok(())
}

fn cmd_flatten(file: &PathBuf, separator: &str) -> Result<()> {
    let v = dq_formats::parse_auto(&file.to_string_lossy(), &std::fs::read(file)?)?;
    let flat = dq_merge::flatten_keys(&v, separator);
    let output = dq_formats::serialize(dq_formats::FormatKind::Json, &flat)?;
    std::io::Write::write_all(&mut std::io::stdout(), &output)?;
    Ok(())
}

fn cmd_unflatten(file: &PathBuf, separator: &str) -> Result<()> {
    let v = dq_formats::parse_auto(&file.to_string_lossy(), &std::fs::read(file)?)?;
    let unflat = dq_merge::unflatten_keys(&v, separator);
    let output = dq_formats::serialize(dq_formats::FormatKind::Json, &unflat)?;
    std::io::Write::write_all(&mut std::io::stdout(), &output)?;
    Ok(())
}

fn cmd_convert(file: &PathBuf, to_format: &str) -> Result<()> {
    let v = dq_formats::parse_auto(&file.to_string_lossy(), &std::fs::read(file)?)?;
    let fmt = dq_formats::FormatKind::from_name(to_format)
        .ok_or_else(|| anyhow::anyhow!("unknown format: {to_format}"))?;
    let output = dq_formats::serialize(fmt, &v)?;
    std::io::Write::write_all(&mut std::io::stdout(), &output)?;
    Ok(())
}

fn cmd_terragrunt(command: TerragruntCommands) -> Result<()> {
    match command {
        TerragruntCommands::Dag { root, format } => {
            let dag = dq_terragrunt::DependencyGraph::from_directory(&root)?;
            match format.as_str() {
                "dot" => print!("{}", dag.to_dot()),
                "json" => {
                    let v = dag.to_value();
                    let output = dq_formats::serialize(dq_formats::FormatKind::Json, &v)?;
                    std::io::Write::write_all(&mut std::io::stdout(), &output)?;
                }
                _ => anyhow::bail!("unsupported format: {format}"),
            }
            Ok(())
        }
        TerragruntCommands::Render { path, format } => {
            let v = dq_terragrunt::render::render_module(&path)?;
            let fmt = dq_formats::FormatKind::from_name(&format).unwrap_or(dq_formats::FormatKind::Json);
            let output = dq_formats::serialize(fmt, &v)?;
            std::io::Write::write_all(&mut std::io::stdout(), &output)?;
            Ok(())
        }
        TerragruntCommands::List { root, format } => {
            let dag = dq_terragrunt::DependencyGraph::from_directory(&root)?;
            match format.as_str() {
                "json" => {
                    // Structured record per module — stable field set
                    // callers can parse without re-scanning: path,
                    // outgoing dep count, incoming dep count, whether
                    // the module declares a source= attribute.
                    use dq_core::Value;
                    use std::sync::Arc;
                    let records: Vec<Value> = dag.graph.node_indices().map(|idx| {
                        let node = &dag.graph[idx];
                        let dep_count = dag.graph.neighbors(idx).count();
                        let dependent_count = dag
                            .graph
                            .neighbors_directed(idx, petgraph::Direction::Incoming)
                            .count();
                        let has_source = node.source.is_some();
                        let mut obj: indexmap::IndexMap<Arc<str>, Value> =
                            indexmap::IndexMap::new();
                        obj.insert(
                            Arc::from("path"),
                            Value::string(node.relative_path.as_str()),
                        );
                        obj.insert(Arc::from("has_source"), Value::bool(has_source));
                        obj.insert(
                            Arc::from("dependency_count"),
                            Value::int(dep_count as i64),
                        );
                        obj.insert(
                            Arc::from("dependent_count"),
                            Value::int(dependent_count as i64),
                        );
                        Value::map(obj)
                    }).collect();
                    let out = dq_formats::serialize(
                        dq_formats::FormatKind::Json,
                        &Value::array(records),
                    )?;
                    std::io::Write::write_all(&mut std::io::stdout(), &out)?;
                }
                _ => {
                    for idx in dag.graph.node_indices() {
                        println!("{}", dag.graph[idx].relative_path);
                    }
                }
            }
            Ok(())
        }
        TerragruntCommands::Deps { path, transitive, root } => {
            let dag = dq_terragrunt::DependencyGraph::from_directory(&root)?;
            let deps = if transitive {
                dag.transitive_dependencies_of(&path)
            } else {
                dag.dependencies_of(&path)
            };
            for dep in deps {
                println!("{}", dep.relative_path);
            }
            Ok(())
        }
        TerragruntCommands::Rdeps { path, root } => {
            let dag = dq_terragrunt::DependencyGraph::from_directory(&root)?;
            for dep in dag.dependents_of(&path) {
                println!("{}", dep.relative_path);
            }
            Ok(())
        }
    }
}

fn cmd_scan(command: ScanCommands) -> Result<()> {
    match command {
        ScanCommands::Appsets { root } => {
            let result = dq_scan::scan_directory(&root)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let appsets_val = dq_scan::report::appsets_to_value(&result.topology.appsets);
            let output = dq_formats::serialize(dq_formats::FormatKind::Json, &appsets_val)?;
            std::io::Write::write_all(&mut std::io::stdout(), &output)?;
            Ok(())
        }
        ScanCommands::Charts { root } => {
            let result = dq_scan::scan_directory(&root)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let charts_val = dq_scan::report::charts_to_value(&result.topology.charts);
            let output = dq_formats::serialize(dq_formats::FormatKind::Json, &charts_val)?;
            std::io::Write::write_all(&mut std::io::stdout(), &output)?;
            Ok(())
        }
        ScanCommands::Environments { root } => {
            let result = dq_scan::scan_directory(&root)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let env_val = dq_scan::report::environments_to_value(
                &result.topology.config_paths,
                &result.topology.taxonomy,
            );
            let output = dq_formats::serialize(dq_formats::FormatKind::Json, &env_val)?;
            std::io::Write::write_all(&mut std::io::stdout(), &output)?;
            Ok(())
        }
        ScanCommands::Topology { root, format } => {
            let result = dq_scan::scan_directory(&root)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            match format.as_str() {
                "json" => {
                    let val = dq_scan::report::to_value(&result.topology);
                    let output = dq_formats::serialize(dq_formats::FormatKind::Json, &val)?;
                    std::io::Write::write_all(&mut std::io::stdout(), &output)?;
                }
                "dot" => {
                    print!("{}", dq_scan::report::to_dot(&result.topology));
                }
                "summary" => {
                    let val = dq_scan::report::to_summary(&result.topology);
                    let output = dq_formats::serialize(dq_formats::FormatKind::Json, &val)?;
                    std::io::Write::write_all(&mut std::io::stdout(), &output)?;
                }
                other => anyhow::bail!("unsupported format: {other} (use json, dot, or summary)"),
            }
            Ok(())
        }
        ScanCommands::Viz { root, output_dir, format } => {
            let generated = match format.as_str() {
                "mermaid" | "md" | "markdown" => dq_viz::generate_all_mermaid(&root, &output_dir)?,
                _ => dq_viz::generate_all(&root, &output_dir)?,
            };
            for f in &generated {
                eprintln!("  generated: {f}");
            }
            let extra = if format == "html" { 1 } else { 0 }; // +1 for index.html in html mode
            eprintln!(
                "Wrote {} files to {}",
                generated.len() + extra,
                output_dir.display()
            );
            Ok(())
        }
    }
}
