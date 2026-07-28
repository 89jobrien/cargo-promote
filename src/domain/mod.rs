pub mod deferral;
pub mod depgraph;
pub mod local_manifest;
pub mod manifest;
pub mod pipeline;
pub mod promote_lock;
pub mod traits;
pub mod version;

use std::path::PathBuf;
use version::BumpLevel;

/// Per-package configuration overrides from `[packages.<name>]`.
#[derive(Debug, Clone)]
pub struct PackageOverride {
    pub autobump: Option<BumpLevel>,
    pub pipeline: Option<String>,
    pub publish: Option<bool>,
}

/// A named cargo registry target.
#[derive(Debug, Clone)]
pub struct Registry {
    /// Display name (e.g. "cratebox", "crates-io")
    pub name: String,
    /// Name as known to `cargo publish --registry <cargo_name>`.
    /// `None` means the default registry (crates.io).
    pub cargo_name: Option<String>,
    /// Optional HTTP API URL for querying crate listings.
    pub api_url: Option<String>,
    /// Whether to prompt for confirmation before publishing.
    pub confirm: bool,
}

// qual:allow reason: "builder pattern replaces repeated struct literals across config"
impl Registry {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            cargo_name: Some(name.clone()),
            name,
            api_url: None,
            confirm: false,
        }
    }

    pub fn with_api_url(mut self, url: impl Into<String>) -> Self {
        self.api_url = Some(url.into());
        self
    }

    pub fn with_confirm(mut self) -> Self {
        self.confirm = true;
        self
    }

    pub fn without_cargo_name(mut self) -> Self {
        self.cargo_name = None;
        self
    }
}

/// A stage in a promotion pipeline — publish to one registry.
#[derive(Debug, Clone)]
pub struct Stage {
    pub registry: Registry,
}

/// An ordered sequence of stages a crate moves through.
#[derive(Debug, Clone)]
pub struct Pipeline {
    pub name: String,
    pub stages: Vec<Stage>,
}

/// Reference to a specific crate to publish.
#[derive(Debug, Clone)]
pub struct CrateRef {
    pub name: String,
    pub version: String,
    pub manifest_path: PathBuf,
}

/// Options that apply to any publish operation.
#[derive(Debug, Clone, Default)]
pub struct PublishOpts {
    pub allow_dirty: bool,
    pub dry_run: bool,
    pub skip_confirm: bool,
    pub force: bool,
}

/// Info about a crate in a registry.
#[derive(Debug, Clone)]
pub struct CrateInfo {
    pub name: String,
    pub max_version: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PromoteError {
    #[error("publish failed for registry '{registry}': {reason}")]
    PublishFailed { registry: String, reason: String },

    #[error("registry query failed for '{registry}': {reason}")]
    QueryFailed { registry: String, reason: String },

    #[error("pipeline '{pipeline}' has no stage named '{stage}'")]
    StageNotFound { pipeline: String, stage: String },

    #[error("stage '{stage}' is the last stage in pipeline '{pipeline}' — nothing to promote to")]
    NoNextStage { pipeline: String, stage: String },

    #[error("registry '{name}' not found in configuration")]
    RegistryNotFound { name: String },

    #[error("pipeline '{name}' not found in configuration")]
    PipelineNotFound { name: String },

    #[error("branch pipeline not configured in promote.toml")]
    BranchPipelineNotConfigured,

    #[error("unknown bump level '{level}', expected patch|minor|major")]
    UnknownBumpLevel { level: String },

    #[error("user aborted")]
    Aborted,

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
