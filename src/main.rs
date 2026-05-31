use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::Command;

#[derive(Parser)]
#[command(
    name = "cargo-promote",
    about = "Publish to minibox registry and optionally promote to crates.io"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Publish a crate to the minibox registry
    Publish {
        /// Path to the crate (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Specific package name (for workspaces)
        #[arg(short = 'p', long)]
        package: Option<String>,

        /// Allow dirty working directory
        #[arg(long)]
        allow_dirty: bool,
    },

    /// Promote a crate from minibox to crates.io
    Promote {
        /// Specific package name (for workspaces)
        #[arg(short = 'p', long)]
        package: Option<String>,

        /// Path to the crate (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,

        /// Dry run — show what would be published without doing it
        #[arg(long)]
        dry_run: bool,
    },

    /// Publish to minibox then immediately promote to crates.io
    Ship {
        /// Specific package name (for workspaces)
        #[arg(short = 'p', long)]
        package: Option<String>,

        /// Path to the crate (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Allow dirty working directory
        #[arg(long)]
        allow_dirty: bool,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// List crates currently in the minibox registry
    List,

    /// Check which local crates have versions not yet on minibox
    Status {
        /// Path to workspace root (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.cmd {
        Cmd::Publish {
            path,
            package,
            allow_dirty,
        } => publish_to_minibox(path.as_deref(), package.as_deref(), allow_dirty),
        Cmd::Promote {
            package,
            path,
            yes,
            dry_run,
        } => promote_to_cratesio(path.as_deref(), package.as_deref(), yes, dry_run),
        Cmd::Ship {
            package,
            path,
            allow_dirty,
            yes,
        } => {
            publish_to_minibox(path.as_deref(), package.as_deref(), allow_dirty)?;
            promote_to_cratesio(path.as_deref(), package.as_deref(), yes, false)
        }
        Cmd::List => list_registry_crates(),
        Cmd::Status { path } => check_status(path.as_deref()),
    }
}

fn publish_to_minibox(
    path: Option<&std::path::Path>,
    package: Option<&str>,
    allow_dirty: bool,
) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.arg("publish").arg("--registry").arg("minibox");

    if let Some(p) = path {
        cmd.arg("--manifest-path").arg(p.join("Cargo.toml"));
    }
    if let Some(pkg) = package {
        cmd.arg("-p").arg(pkg);
    }
    if allow_dirty {
        cmd.arg("--allow-dirty");
    }

    eprintln!("=> Publishing to minibox registry...");
    let status = cmd.status().context("failed to run cargo publish")?;
    if !status.success() {
        bail!("cargo publish --registry minibox failed");
    }
    eprintln!("=> Published to minibox");
    Ok(())
}

fn promote_to_cratesio(
    path: Option<&std::path::Path>,
    package: Option<&str>,
    skip_confirm: bool,
    dry_run: bool,
) -> Result<()> {
    if !skip_confirm && !dry_run {
        eprintln!("=> About to publish to crates.io. Continue? [y/N]");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            eprintln!("=> Aborted.");
            return Ok(());
        }
    }

    let mut cmd = Command::new("cargo");
    cmd.arg("publish");

    if let Some(p) = path {
        cmd.arg("--manifest-path").arg(p.join("Cargo.toml"));
    }
    if let Some(pkg) = package {
        cmd.arg("-p").arg(pkg);
    }
    if dry_run {
        cmd.arg("--dry-run");
        eprintln!("=> Dry run: would publish to crates.io");
    } else {
        eprintln!("=> Promoting to crates.io...");
    }

    let status = cmd.status().context("failed to run cargo publish")?;
    if !status.success() {
        bail!("cargo publish to crates.io failed");
    }
    if !dry_run {
        eprintln!("=> Published to crates.io");
    }
    Ok(())
}

fn list_registry_crates() -> Result<()> {
    let registry_url =
        std::env::var("REGISTRY_URL").unwrap_or_else(|_| "http://100.105.75.7:3000".to_string());
    let user = std::env::var("REGISTRY_USER").unwrap_or_else(|_| "joe".to_string());

    let url = format!("{}/api/packages/{}/cargo/api/v1/crates", registry_url, user);

    let output = Command::new("curl")
        .args(["-sf", &url])
        .output()
        .context("failed to run curl")?;

    if !output.status.success() {
        bail!("registry unreachable at {}", url);
    }

    let body: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("invalid JSON from registry")?;

    let crates = body["crates"]
        .as_array()
        .context("unexpected response format")?;
    if crates.is_empty() {
        println!("  (no crates published)");
    } else {
        for c in crates {
            println!(
                "  {} v{}",
                c["name"].as_str().unwrap_or("?"),
                c["max_version"].as_str().unwrap_or("?")
            );
        }
        println!("\n  {} crate(s) total", crates.len());
    }
    Ok(())
}

fn check_status(path: Option<&std::path::Path>) -> Result<()> {
    let manifest_path = path
        .map(|p| p.join("Cargo.toml"))
        .unwrap_or_else(|| PathBuf::from("Cargo.toml"));

    let content = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("cannot read {}", manifest_path.display()))?;

    let doc: toml::Value = content.parse().context("invalid Cargo.toml")?;

    // Check if workspace
    if let Some(members) = doc
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
    {
        eprintln!("Workspace with {} members:", members.len());
        for member in members {
            if let Some(m) = member.as_str() {
                let member_manifest = path
                    .unwrap_or(std::path::Path::new("."))
                    .join(m)
                    .join("Cargo.toml");
                if let Ok(c) = std::fs::read_to_string(&member_manifest) {
                    if let Ok(d) = c.parse::<toml::Value>() {
                        let name = d
                            .get("package")
                            .and_then(|p| p.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("?");
                        let version = d
                            .get("package")
                            .and_then(|p| p.get("version"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("?");
                        println!("  {name} v{version}");
                    }
                }
            }
        }
    } else if let Some(pkg) = doc.get("package") {
        let name = pkg.get("name").and_then(|n| n.as_str()).unwrap_or("?");
        let version = pkg.get("version").and_then(|v| v.as_str()).unwrap_or("?");
        println!("  {name} v{version}");
    }

    eprintln!("\n(TODO: compare against registry versions)");
    Ok(())
}
