use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use cargo_promote::Api;

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

        /// Dry run -- show publish order without publishing
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

    /// Bump version and create promote.lock
    Bump {
        #[arg(short, long)]
        path: Option<PathBuf>,
        #[arg(short = 'p', long)]
        package: Option<String>,
    },

    /// Branch from one stage to the next
    Branch {
        #[arg(short, long)]
        path: Option<PathBuf>,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: Option<String>,
    },

    /// Defer promotion to the next stage (provisional, pending confirmation)
    Defer {
        #[arg(long)]
        package: Option<String>,
        #[arg(long)]
        path: Option<PathBuf>,
        #[arg(long)]
        from: String,
        #[arg(long)]
        pipeline: Option<String>,
        /// Defer a branch pipeline merge instead of a registry publish
        #[arg(long)]
        branch: bool,
        /// Notification command to fire (non-blocking)
        #[arg(long, num_args = 1..)]
        command: Vec<String>,
    },

    /// Confirm a pending deferral
    Confirm {
        /// Deferral ticket ID
        ticket: String,
        #[arg(short, long)]
        path: Option<PathBuf>,
        #[arg(long, default_value = "")]
        reason: String,
    },

    /// Reject a pending deferral
    Reject {
        /// Deferral ticket ID
        ticket: String,
        #[arg(short, long)]
        path: Option<PathBuf>,
        #[arg(long, default_value = "")]
        reason: String,
    },

    /// List deferrals
    Deferrals {
        #[arg(short, long)]
        path: Option<PathBuf>,
        /// Show only pending deferrals
        #[arg(long)]
        pending: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir().context("cannot determine current directory")?;

    let interactive_confirmer = |prompt: &str| -> bool {
        eprintln!("=> {prompt} [y/N]");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok();
        input.trim().eq_ignore_ascii_case("y")
    };

    match cli.cmd {
        Cmd::Publish {
            path,
            package,
            allow_dirty,
            pipeline,
            registry,
        } => {
            let dir = path.as_deref().unwrap_or(&cwd);
            let api = Api::with_confirmer(dir, interactive_confirmer)?;
            api.publish(
                path.as_deref(),
                package.as_deref(),
                allow_dirty,
                pipeline.as_deref(),
                registry.as_deref(),
            )
        }

        Cmd::Promote {
            package,
            path,
            yes,
            dry_run,
            pipeline,
            from,
        } => {
            let dir = path.as_deref().unwrap_or(&cwd);
            let api = Api::with_confirmer(dir, interactive_confirmer)?;
            api.promote(
                path.as_deref(),
                package.as_deref(),
                yes,
                dry_run,
                pipeline.as_deref(),
                from.as_deref(),
            )
        }

        Cmd::Ship {
            package,
            path,
            allow_dirty,
            yes,
            pipeline,
        } => {
            let dir = path.as_deref().unwrap_or(&cwd);
            let api = Api::with_confirmer(dir, interactive_confirmer)?;
            api.ship(
                path.as_deref(),
                package.as_deref(),
                allow_dirty,
                yes,
                pipeline.as_deref(),
            )
        }

        Cmd::List { registry } => {
            let api = Api::with_confirmer(&cwd, interactive_confirmer)?;
            let crates = api.list(registry.as_deref())?;
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
            let api = Api::with_confirmer(&cwd, interactive_confirmer)?;
            let root = path.unwrap_or_else(|| {
                PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("dev")
            });
            let skip_list: Vec<&str> = skip.split(',').map(|s| s.trim()).collect();

            let result =
                api.publish_all(&root, allow_dirty, dry_run, registry.as_deref(), &skip_list)?;

            eprintln!(
                "=== PUBLISH ORDER ({} crates) ===",
                result.publish_order.len()
            );
            for (i, name) in result.publish_order.iter().enumerate() {
                eprintln!("  {:3}. {}", i + 1, name);
            }

            if dry_run {
                eprintln!("\n(dry run -- nothing published)");
            } else {
                eprintln!("\n=== SUMMARY ===");
                eprintln!("  Published: {}", result.ok);
                eprintln!("  Failed: {}", result.failed.len());
                eprintln!("  Blocked (path-only): {}", result.blocked.len());
                if !result.failed.is_empty() {
                    eprintln!("  Failed crates: {}", result.failed.join(", "));
                }
            }

            if !result.blocked.is_empty() {
                eprintln!(
                    "=== BLOCKED (path-only deps) ===\n  {}",
                    result.blocked.join(", ")
                );
            }

            Ok(())
        }

        Cmd::Status { path } => {
            let desc = Api::status(path.as_deref())?;
            match desc {
                cargo_promote::domain::manifest::ManifestDescription::Workspace(members) => {
                    eprintln!("Workspace with {} members:", members.len());
                    for m in &members {
                        println!("  {} v{}", m.name, m.version);
                    }
                }
                cargo_promote::domain::manifest::ManifestDescription::Single(info) => {
                    println!("  {} v{}", info.name, info.version);
                }
            }
            Ok(())
        }

        Cmd::Bump { path, package } => {
            let dir = path.as_deref().unwrap_or(&cwd);
            let api = Api::with_confirmer(dir, interactive_confirmer)?;
            api.bump(path.as_deref(), package.as_deref(), &cwd)
        }

        Cmd::Branch { path, from, to: _ } => {
            let dir = path.as_deref().unwrap_or(&cwd);
            let api = Api::with_confirmer(dir, interactive_confirmer)?;
            api.branch(path.as_deref(), &from, &cwd)
        }

        Cmd::Defer {
            package,
            path,
            from,
            pipeline,
            branch,
            command,
        } => {
            let dir = path.as_deref().unwrap_or(&cwd);
            let api = Api::with_notifier(dir, interactive_confirmer, command)?;
            let repo_root = path.as_deref().unwrap_or(&cwd);
            let deferral = if branch {
                api.defer_branch(path.as_deref(), package.as_deref(), &from, repo_root)?
            } else {
                api.defer_to(
                    path.as_deref(),
                    package.as_deref(),
                    &from,
                    pipeline.as_deref(),
                    repo_root,
                )?
            };
            eprintln!(
                "=> deferred {} v{} from '{}' to '{}' [ticket: {}]",
                deferral.crate_name,
                deferral.version,
                deferral.from_stage,
                deferral.to_stage,
                deferral.ticket,
            );
            Ok(())
        }

        Cmd::Confirm {
            ticket,
            path,
            reason,
        } => {
            let repo_root = path.as_deref().unwrap_or(&cwd);
            let api = Api::with_confirmer(repo_root, interactive_confirmer)?;
            let d = api.confirm_deferral(repo_root, &ticket, &reason)?;
            eprintln!(
                "=> confirmed {} v{} -> '{}'",
                d.crate_name, d.version, d.to_stage,
            );
            Ok(())
        }

        Cmd::Reject {
            ticket,
            path,
            reason,
        } => {
            let repo_root = path.as_deref().unwrap_or(&cwd);
            let d = Api::reject_deferral(repo_root, &ticket, &reason)?;
            eprintln!(
                "=> rejected {} v{} (was heading to '{}')",
                d.crate_name, d.version, d.to_stage,
            );
            Ok(())
        }

        Cmd::Deferrals { path, pending } => {
            let repo_root = path.as_deref().unwrap_or(&cwd);
            let deferrals = Api::deferrals(repo_root, pending)?;
            if deferrals.is_empty() {
                println!("  (no deferrals)");
            } else {
                for d in &deferrals {
                    println!(
                        "  [{}] {} v{} {} -> {} ({})",
                        d.ticket,
                        d.crate_name,
                        d.version,
                        d.from_stage,
                        d.to_stage,
                        match d.status {
                            cargo_promote::domain::deferral::DeferralStatus::Pending => "pending",
                            cargo_promote::domain::deferral::DeferralStatus::Confirmed =>
                                "confirmed",
                            cargo_promote::domain::deferral::DeferralStatus::Rejected => "rejected",
                        },
                    );
                }
            }
            Ok(())
        }
    }
}
