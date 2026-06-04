mod cli;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

use cargo_promote::{Api, PromoteParams, PublishAllParams, PublishParams, ShipParams};
use cli::{interactive_confirmer, Cli, Cmd};

fn api_for(path: Option<&std::path::Path>, cwd: &std::path::Path) -> Result<Api> {
    Api::with_confirmer(path.unwrap_or(cwd), interactive_confirmer)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir().context("cannot determine current directory")?;

    match cli.cmd {
        Cmd::Publish {
            path,
            package,
            allow_dirty,
            pipeline,
            registry,
            force,
        } => {
            let api = api_for(path.as_deref(), &cwd)?;
            api.publish(&PublishParams {
                path: path.as_deref(),
                package: package.as_deref(),
                allow_dirty,
                force,
                pipeline: pipeline.as_deref(),
                registry: registry.as_deref(),
            })
        }

        Cmd::Promote {
            package,
            path,
            yes,
            dry_run,
            pipeline,
            from,
        } => {
            let api = api_for(path.as_deref(), &cwd)?;
            api.promote(&PromoteParams {
                path: path.as_deref(),
                package: package.as_deref(),
                yes,
                dry_run,
                pipeline: pipeline.as_deref(),
                from: from.as_deref(),
            })
        }

        Cmd::Ship {
            package,
            path,
            allow_dirty,
            yes,
            pipeline,
            force,
        } => {
            let api = api_for(path.as_deref(), &cwd)?;
            api.ship(&ShipParams {
                path: path.as_deref(),
                package: package.as_deref(),
                allow_dirty,
                yes,
                force,
                pipeline: pipeline.as_deref(),
            })
        }

        Cmd::List { registry } => {
            let api = api_for(None, &cwd)?;
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
            force,
        } => {
            let api = api_for(None, &cwd)?;
            let root = path.unwrap_or_else(|| {
                PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("dev")
            });
            let skip_list: Vec<&str> = skip.split(',').map(|s| s.trim()).collect();

            let result = api.publish_all(&PublishAllParams {
                root: &root,
                allow_dirty,
                dry_run,
                force,
                registry: registry.as_deref(),
                skip: &skip_list,
            })?;

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
            let api = api_for(path.as_deref(), &cwd)?;
            api.bump(path.as_deref(), package.as_deref(), &cwd)
        }

        Cmd::Branch {
            path,
            from,
            to,
            tag,
        } => {
            let api = api_for(path.as_deref(), &cwd)?;
            api.branch(path.as_deref(), &from, to.as_deref(), &cwd)?;
            if tag {
                api.branch_tag(path.as_deref(), None)?;
            }
            Ok(())
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
            let api = api_for(Some(repo_root), &cwd)?;
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
            let api = api_for(Some(repo_root), &cwd)?;
            let d = api.reject_deferral(&ticket, &reason)?;
            eprintln!(
                "=> rejected {} v{} (was heading to '{}')",
                d.crate_name, d.version, d.to_stage,
            );
            Ok(())
        }

        Cmd::Deferrals { path, pending } => {
            let repo_root = path.as_deref().unwrap_or(&cwd);
            let api = api_for(Some(repo_root), &cwd)?;
            let deferrals = api.deferrals(pending)?;
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
