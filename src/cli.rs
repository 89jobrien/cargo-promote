use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "cargo-promote",
    about = "Publish crates through configurable promotion pipelines"
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
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
        /// Publish even if the version already exists in the registry
        #[arg(long)]
        force: bool,
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
        /// Publish even if versions already exist in registries
        #[arg(long)]
        force: bool,
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

        /// Publish even if versions already exist in registries
        #[arg(long)]
        force: bool,
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
        /// Target stage (defaults to next stage in pipeline)
        #[arg(long)]
        to: Option<String>,
        /// After the final branch merge, tag the release
        #[arg(long)]
        tag: bool,
    },

    /// CI promote: FF-merge develop → main, rail patch bump, push commit + tags
    CiPromote {
        /// Git remote name
        #[arg(long, default_value = "origin")]
        remote: String,
        /// Source branch (default: develop)
        #[arg(long, default_value = "develop")]
        from: String,
        /// Target branch (default: main)
        #[arg(long, default_value = "main")]
        to: String,
        /// Crate / package name to pass to cargo-rail
        #[arg(long)]
        package: Option<String>,
        #[arg(short, long)]
        path: Option<PathBuf>,
        /// Print plan, make no changes
        #[arg(long, short = 'n')]
        dry_run: bool,
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

/// Interactive confirmation prompt — reads y/N from stdin.
pub fn interactive_confirmer(prompt: &str) -> bool {
    eprintln!("=> {prompt} [y/N]");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
    input.trim().eq_ignore_ascii_case("y")
}
