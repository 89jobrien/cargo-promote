use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use cargo_promote::config::Config;
use cargo_promote::domain::depgraph;
use cargo_promote::domain::manifest::{self, ManifestDescription};
use cargo_promote::domain::pipeline::PipelineEngine;
use cargo_promote::domain::traits::RegistryQuery;
use cargo_promote::domain::{CrateRef, Pipeline, PublishOpts, Stage};
use cargo_promote::infra::cargo::CargoPublisher;
use cargo_promote::infra::git::gitea::GiteaRegistry;

#[derive(Parser)]
#[command(
    name = "cargo-promote",
    about = "Publish crates through configurable promotion pipelines"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Publish a crate to the first stage of a pipeline
    Publish {
        #[arg(short, long)]
        path: Option<PathBuf>,
        #[arg(short = 'p', long)]
        package: Option<String>,
        #[arg(long)]
        allow_dirty: bool,
        #[arg(long)]
        pipeline: Option<String>,
        #[arg(long)]
        registry: Option<String>,
    },

    /// Promote a crate from one pipeline stage to the next
    Promote {
        #[arg(short = 'p', long)]
        package: Option<String>,
        #[arg(short, long)]
        path: Option<PathBuf>,
        #[arg(short = 'y', long)]
        yes: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        pipeline: Option<String>,
        #[arg(long)]
        from: Option<String>,
    },

    /// Run all stages of a pipeline
    Ship {
        #[arg(short = 'p', long)]
        package: Option<String>,
        #[arg(short, long)]
        path: Option<PathBuf>,
        #[arg(long)]
        allow_dirty: bool,
        #[arg(short = 'y', long)]
        yes: bool,
        #[arg(long)]
        pipeline: Option<String>,
    },

    /// List crates in a registry
    List {
        #[arg(long)]
        registry: Option<String>,
    },

    /// Show local crate versions
    Status {
        #[arg(short, long)]
        path: Option<PathBuf>,
    },

    /// Publish all crates under a directory in dependency order
    PublishAll {
        /// Root directory to scan (defaults to ~/dev)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Allow dirty working directories
        #[arg(long)]
        allow_dirty: bool,

        /// Dry run — show publish order without publishing
        #[arg(long)]
        dry_run: bool,

        /// Registry to publish to (defaults to pipeline first stage)
        #[arg(long)]
        registry: Option<String>,

        /// Repos to skip (comma-separated)
        #[arg(
            long,
            default_value = "maestro,maestro-feat-minibox-provider,maestro-slides,seaography,langchainx,prusti-dev,hyperdocker-main,sandbox"
        )]
        skip: String,
    },
}

/// Shared runtime context for all pipeline commands.
struct App<'a> {
    engine: &'a PipelineEngine<CargoPublisher>,
    cfg: &'a Config,
}

impl App<'_> {
    fn resolve_pipeline(&self, name: Option<&str>) -> Result<&Pipeline> {
        self.cfg
            .pipeline(name)
            .ok_or_else(|| anyhow::anyhow!("pipeline '{}' not found", name.unwrap_or("default")))
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    let cfg = Config::load(&cwd)?;

    let publisher = CargoPublisher;
    let engine = PipelineEngine::new(publisher, |prompt| {
        eprintln!("=> {prompt} [y/N]");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok();
        input.trim().eq_ignore_ascii_case("y")
    });

    let app = App {
        engine: &engine,
        cfg: &cfg,
    };

    match cli.cmd {
        Cmd::Publish {
            path,
            package,
            allow_dirty,
            pipeline,
            registry,
        } => {
            let krate = manifest::resolve_crate(path.as_deref(), package.as_deref())?;
            let opts = PublishOpts {
                allow_dirty,
                ..Default::default()
            };

            if let Some(reg_name) = registry {
                let reg = app
                    .cfg
                    .registry(&reg_name)
                    .ok_or_else(|| anyhow::anyhow!("unknown registry '{reg_name}'"))?;
                let stage = Stage {
                    registry: reg.clone(),
                };
                app.engine.run_stage(&krate, &stage, &opts)?;
            } else {
                let pl = app.resolve_pipeline(pipeline.as_deref())?;
                let first = pl.stages.first().context("pipeline has no stages")?;
                app.engine.run_stage(&krate, first, &opts)?;
            }
            Ok(())
        }

        Cmd::Promote {
            package,
            path,
            yes,
            dry_run,
            pipeline,
            from,
        } => {
            let krate = manifest::resolve_crate(path.as_deref(), package.as_deref())?;
            let opts = PublishOpts {
                skip_confirm: yes,
                dry_run,
                ..Default::default()
            };
            let pl = app.resolve_pipeline(pipeline.as_deref())?;
            let from_stage = from
                .as_deref()
                .unwrap_or_else(|| &pl.stages[0].registry.name);
            app.engine.promote_next(&krate, pl, from_stage, &opts)?;
            Ok(())
        }

        Cmd::Ship {
            package,
            path,
            allow_dirty,
            yes,
            pipeline,
        } => {
            let krate = manifest::resolve_crate(path.as_deref(), package.as_deref())?;
            let opts = PublishOpts {
                allow_dirty,
                skip_confirm: yes,
                ..Default::default()
            };
            let pl = app.resolve_pipeline(pipeline.as_deref())?;
            app.engine.run_full(&krate, pl, &opts)?;
            Ok(())
        }

        Cmd::List { registry } => {
            let query = GiteaRegistry;
            let reg_name = registry.as_deref().unwrap_or("minibox");
            let reg = app
                .cfg
                .registry(reg_name)
                .ok_or_else(|| anyhow::anyhow!("unknown registry '{reg_name}'"))?;
            let crates = query.list_crates(reg)?;
            if crates.is_empty() {
                println!("  (no crates published)");
            } else {
                for c in &crates {
                    println!("  {} v{}", c.name, c.max_version);
                }
                println!("\n  {} crate(s) total", crates.len());
            }
            Ok(())
        }

        Cmd::PublishAll {
            path,
            allow_dirty,
            dry_run,
            registry,
            skip,
        } => {
            let root = path.unwrap_or_else(|| {
                PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("dev")
            });
            let skip_list: Vec<&str> = skip.split(',').map(|s| s.trim()).collect();
            let nodes = depgraph::scan_workspace_tree(&root, &skip_list)?;

            let publishable: Vec<_> = nodes.iter().filter(|n| !n.unpublishable).collect();

            let order =
                depgraph::topo_sort(&publishable.iter().map(|n| (*n).clone()).collect::<Vec<_>>())?;

            // Report path-only deps
            let blocked: Vec<_> = publishable
                .iter()
                .filter(|n| !n.path_only_deps.is_empty())
                .collect();
            if !blocked.is_empty() {
                eprintln!("=== BLOCKED (path-only deps, no version field) ===");
                for n in &blocked {
                    eprintln!("  {} -> {:?}", n.name, n.path_only_deps);
                }
                eprintln!();
            }

            let publishable_names: HashSet<&str> = publishable
                .iter()
                .filter(|n| n.path_only_deps.is_empty())
                .map(|n| n.name.as_str())
                .collect();

            let publish_order: Vec<_> = order
                .iter()
                .filter(|name| publishable_names.contains(name.as_str()))
                .collect();

            eprintln!("=== PUBLISH ORDER ({} crates) ===", publish_order.len());
            for (i, name) in publish_order.iter().enumerate() {
                eprintln!("  {:3}. {}", i + 1, name);
            }

            if dry_run {
                eprintln!("\n(dry run — nothing published)");
                return Ok(());
            }

            eprintln!();

            let reg_name = registry.as_deref().unwrap_or("minibox");
            let reg = app
                .cfg
                .registry(reg_name)
                .ok_or_else(|| anyhow::anyhow!("unknown registry '{reg_name}'"))?;
            let stage = Stage {
                registry: reg.clone(),
            };
            let opts = PublishOpts {
                allow_dirty,
                dry_run: false,
                skip_confirm: true,
            };

            let node_map: HashMap<&str, &depgraph::CrateNode> =
                nodes.iter().map(|n| (n.name.as_str(), n)).collect();

            let mut ok = 0usize;
            let mut failed = Vec::new();
            for name in &publish_order {
                let node = node_map[name.as_str()];
                let krate = CrateRef {
                    name: node.name.clone(),
                    version: node.version.clone(),
                    manifest_path: node.manifest_path.clone(),
                };
                match app.engine.run_stage(&krate, &stage, &opts) {
                    Ok(()) => ok += 1,
                    Err(e) => {
                        eprintln!("  FAIL: {} — {}", name, e);
                        failed.push(name.as_str());
                    }
                }
            }

            eprintln!("\n=== SUMMARY ===");
            eprintln!("  Published: {ok}");
            eprintln!("  Failed: {}", failed.len());
            eprintln!("  Blocked (path-only): {}", blocked.len());
            if !failed.is_empty() {
                eprintln!("  Failed crates: {}", failed.join(", "));
            }
            Ok(())
        }

        Cmd::Status { path } => {
            let desc = manifest::describe_manifest(path.as_deref())?;
            match desc {
                ManifestDescription::Workspace(members) => {
                    eprintln!("Workspace with {} members:", members.len());
                    for m in &members {
                        println!("  {} v{}", m.name, m.version);
                    }
                }
                ManifestDescription::Single(info) => {
                    println!("  {} v{}", info.name, info.version);
                }
            }
            Ok(())
        }
    }
}
