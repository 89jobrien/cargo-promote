pub mod config;
pub mod domain;
pub mod infra;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};

use config::Config;
use domain::deferral::{Deferral, DeferralKind, DeferralStatus};
use domain::depgraph;
use domain::manifest::{self, ManifestDescription};
use domain::pipeline::PipelineEngine;
use domain::traits::{Notifier, PipelineRunner, RegistryQuery};
use domain::version;
use domain::{CrateInfo, CrateRef, Pipeline, PublishOpts, Stage};
use infra::cargo::CargoPublisher;
use infra::git::gitea::GiteaRegistry;
use infra::token::CargoTokenResolver;

/// If autobump is configured, bump the manifest version and return an
/// updated CrateRef.
pub fn maybe_autobump(krate: CrateRef, cfg: &Config) -> Result<CrateRef> {
    let per_pkg = cfg.package_override(&krate.name).and_then(|o| o.autobump);
    let Some(level) = per_pkg.or(cfg.autobump) else {
        return Ok(krate);
    };
    let (old, new) = version::bump_manifest_version(&krate.manifest_path, level)?;
    eprintln!("=> autobump: {} v{old} -> v{new}", krate.name);
    Ok(CrateRef {
        version: new.to_string(),
        ..krate
    })
}

/// Library API for driving promotion pipelines programmatically.
pub struct Api {
    config: Config,
    engine: Box<dyn PipelineRunner>,
    registry_query: Box<dyn RegistryQuery>,
    notifier: Box<dyn Notifier>,
}

impl Api {
    /// Build with default adapters (CargoPublisher, GiteaRegistry,
    /// NoopNotifier) and auto-accepting confirmer.
    pub fn new(dir: &Path) -> Result<Self> {
        Self::with_confirmer(dir, |_| true)
    }

    /// Build with default adapters and a custom confirmer.
    pub fn with_confirmer(dir: &Path, confirmer: impl Fn(&str) -> bool + 'static) -> Result<Self> {
        let config = Config::load(dir)?;
        let engine = PipelineEngine::new(CargoPublisher, confirmer);
        Ok(Self {
            config,
            engine: Box::new(engine),
            registry_query: Box::new(GiteaRegistry::new(std::sync::Arc::new(
                CargoTokenResolver::new(),
            ))),
            notifier: Box::new(infra::notify::NoopNotifier),
        })
    }

    /// Build with default adapters, custom confirmer, and a
    /// notification command.
    pub fn with_notifier(
        dir: &Path,
        confirmer: impl Fn(&str) -> bool + 'static,
        command: Vec<String>,
    ) -> Result<Self> {
        let config = Config::load(dir)?;
        let engine = PipelineEngine::new(CargoPublisher, confirmer);
        Ok(Self {
            config,
            engine: Box::new(engine),
            registry_query: Box::new(GiteaRegistry::new(std::sync::Arc::new(
                CargoTokenResolver::new(),
            ))),
            notifier: Box::new(infra::notify::SpawnNotifier { command }),
        })
    }

    /// Access the loaded configuration.
    pub fn config(&self) -> &Config {
        &self.config
    }

    // -- pipeline helpers --

    fn resolve_pipeline(&self, name: Option<&str>) -> Result<&Pipeline> {
        self.config
            .pipeline(name)
            .ok_or_else(|| anyhow::anyhow!("pipeline '{}' not found", name.unwrap_or("default")))
    }

    /// Publish a crate to the first stage of a pipeline (or a named
    /// registry).
    pub fn publish(
        &self,
        path: Option<&Path>,
        package: Option<&str>,
        allow_dirty: bool,
        pipeline: Option<&str>,
        registry: Option<&str>,
    ) -> Result<()> {
        let krate = manifest::resolve_crate(path, package)?;
        let krate = maybe_autobump(krate, &self.config)?;
        let opts = PublishOpts {
            allow_dirty,
            ..Default::default()
        };

        if let Some(reg_name) = registry {
            let reg = self
                .config
                .registry(reg_name)
                .ok_or_else(|| anyhow::anyhow!("unknown registry '{reg_name}'"))?;
            let stage = Stage {
                registry: reg.clone(),
            };
            self.engine.run_stage(&krate, &stage, &opts)?;
        } else {
            let pl = self.resolve_pipeline(pipeline)?;
            let first = pl.stages.first().context("pipeline has no stages")?;
            self.engine.run_stage(&krate, first, &opts)?;
        }
        Ok(())
    }

    /// Promote a crate from one pipeline stage to the next.
    pub fn promote(
        &self,
        path: Option<&Path>,
        package: Option<&str>,
        yes: bool,
        dry_run: bool,
        pipeline: Option<&str>,
        from: Option<&str>,
    ) -> Result<()> {
        let krate = manifest::resolve_crate(path, package)?;
        let opts = PublishOpts {
            skip_confirm: yes,
            dry_run,
            ..Default::default()
        };
        let pl = self.resolve_pipeline(pipeline)?;
        let from_stage = from.unwrap_or_else(|| &pl.stages[0].registry.name);
        self.engine.promote_next(&krate, pl, from_stage, &opts)?;
        Ok(())
    }

    /// Run all stages of a pipeline sequentially.
    pub fn ship(
        &self,
        path: Option<&Path>,
        package: Option<&str>,
        allow_dirty: bool,
        yes: bool,
        pipeline: Option<&str>,
    ) -> Result<()> {
        let krate = manifest::resolve_crate(path, package)?;
        let krate = maybe_autobump(krate, &self.config)?;
        let opts = PublishOpts {
            allow_dirty,
            skip_confirm: yes,
            ..Default::default()
        };
        let pl = self.resolve_pipeline(pipeline)?;
        self.engine.run_full(&krate, pl, &opts)?;
        Ok(())
    }

    /// List crates in a registry.
    pub fn list(&self, registry: Option<&str>) -> Result<Vec<CrateInfo>> {
        let reg_name = registry.unwrap_or("cratebox");
        let reg = self
            .config
            .registry(reg_name)
            .ok_or_else(|| anyhow::anyhow!("unknown registry '{reg_name}'"))?;
        let crates = self.registry_query.list_crates(reg)?;
        Ok(crates)
    }

    /// Describe local crate versions.
    pub fn status(path: Option<&Path>) -> Result<ManifestDescription> {
        manifest::describe_manifest(path)
    }

    /// Publish all crates under a directory in dependency order.
    pub fn publish_all(
        &self,
        root: &Path,
        allow_dirty: bool,
        dry_run: bool,
        registry: Option<&str>,
        skip: &[&str],
    ) -> Result<PublishAllResult> {
        let nodes = depgraph::scan_workspace_tree(root, skip)?;
        let publishable: Vec<_> = nodes.iter().filter(|n| !n.unpublishable).collect();
        let order =
            depgraph::topo_sort(&publishable.iter().map(|n| (*n).clone()).collect::<Vec<_>>())?;

        let blocked: Vec<_> = publishable
            .iter()
            .filter(|n| !n.path_only_deps.is_empty())
            .collect();

        let publishable_names: HashSet<&str> = publishable
            .iter()
            .filter(|n| n.path_only_deps.is_empty())
            .filter(|n| {
                self.config
                    .package_override(&n.name)
                    .and_then(|o| o.publish)
                    != Some(false)
            })
            .map(|n| n.name.as_str())
            .collect();

        let publish_order: Vec<String> = order
            .iter()
            .filter(|name| publishable_names.contains(name.as_str()))
            .cloned()
            .collect();

        let blocked_names: Vec<String> = blocked.iter().map(|n| n.name.clone()).collect();

        if dry_run {
            return Ok(PublishAllResult {
                publish_order,
                ok: 0,
                failed: vec![],
                blocked: blocked_names,
            });
        }

        let reg_name = registry.unwrap_or("cratebox");
        let reg = self
            .config
            .registry(reg_name)
            .ok_or_else(|| anyhow::anyhow!("unknown registry '{reg_name}'"))?;
        let stage = Stage {
            registry: reg.clone(),
        };
        let opts = PublishOpts {
            allow_dirty,
            skip_confirm: true,
            ..Default::default()
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
            match self.engine.run_stage(&krate, &stage, &opts) {
                Ok(()) => ok += 1,
                Err(e) => {
                    eprintln!("  FAIL: {} -- {}", name, e);
                    failed.push(name.clone());
                }
            }
        }

        Ok(PublishAllResult {
            publish_order,
            ok,
            failed,
            blocked: blocked_names,
        })
    }

    /// Bump version and create promote.lock.
    pub fn bump(&self, path: Option<&Path>, package: Option<&str>, cwd: &Path) -> Result<()> {
        let krate = manifest::resolve_crate(path, package)?;
        let branch_cfg = self
            .config
            .branch_pipeline
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("branch pipeline not configured in promote.toml"))?;
        let repo_path = path.unwrap_or(cwd);
        let git = infra::git::local::LocalGit::new(repo_path.to_path_buf());
        domain::pipeline::BranchPipeline::bump(&krate, &branch_cfg.stages, repo_path, &git)?;
        Ok(())
    }

    /// Branch from one stage to the next.
    pub fn branch(&self, path: Option<&Path>, from: &str, cwd: &Path) -> Result<()> {
        let branch_cfg = self
            .config
            .branch_pipeline
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("branch pipeline not configured in promote.toml"))?;
        let repo_root = path.unwrap_or(cwd);
        let git = infra::git::local::LocalGit::new(repo_root.to_path_buf());
        domain::pipeline::BranchPipeline::branch(&branch_cfg.stages, from, &git, &git, repo_root)?;
        Ok(())
    }

    /// Defer a crate's promotion to the next pipeline stage.
    ///
    /// Creates a pending deferral ticket and fires a notification.
    /// The promotion is provisional until confirmed or rejected.
    pub fn defer_to(
        &self,
        path: Option<&Path>,
        package: Option<&str>,
        from: &str,
        pipeline: Option<&str>,
        repo_root: &Path,
    ) -> Result<Deferral> {
        let krate = manifest::resolve_crate(path, package)?;
        let pl = self.resolve_pipeline(pipeline)?;

        let from_idx = pl
            .stages
            .iter()
            .position(|s| s.registry.name == from)
            .ok_or_else(|| anyhow::anyhow!("unknown stage '{from}' in pipeline"))?;
        let to_stage = pl
            .stages
            .get(from_idx + 1)
            .ok_or_else(|| anyhow::anyhow!("no next stage after '{from}'"))?;

        let source_hash = domain::promote_lock::PromoteLock::compute_source_hash(repo_root)?;

        let ticket = Deferral::ticket_id(&krate.name);
        let now = chrono::Local::now();

        let deferral = Deferral {
            ticket: ticket.clone(),
            crate_name: krate.name.clone(),
            version: krate.version.clone(),
            from_stage: from.to_string(),
            to_stage: to_stage.registry.name.clone(),
            status: DeferralStatus::Pending,
            kind: DeferralKind::Registry,
            deferred_at: now.format("%Y%m%d.%H%M%S").to_string(),
            source_hash,
            command: vec![],
            reason: String::new(),
        };

        deferral.write(repo_root)?;
        self.notifier.on_deferred(&deferral)?;
        Ok(deferral)
    }

    /// Defer a branch promotion (merge from one stage branch to the
    /// next). Verifies the promote.lock hash before creating the
    /// ticket.
    // qual:allow(iosp) reason: "integration root — orchestrates validation + deferral"
    pub fn defer_branch(
        &self,
        path: Option<&Path>,
        package: Option<&str>,
        from: &str,
        repo_root: &Path,
    ) -> Result<Deferral> {
        let krate = manifest::resolve_crate(path, package)?;
        let branch_cfg = self
            .config
            .branch_pipeline
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("branch pipeline not configured in promote.toml"))?;

        let from_idx = branch_cfg
            .stages
            .iter()
            .position(|s| s == from)
            .ok_or_else(|| anyhow::anyhow!("unknown branch stage '{from}'"))?;
        let to_stage = branch_cfg
            .stages
            .get(from_idx + 1)
            .ok_or_else(|| anyhow::anyhow!("no next stage after '{from}'"))?;

        // Verify promote.lock hash before deferring.
        let lock = domain::promote_lock::PromoteLock::read(repo_root)?;
        lock.verify_hash(repo_root)?;

        let ticket = Deferral::ticket_id(&krate.name);
        let now = chrono::Local::now();

        let deferral = Deferral {
            ticket,
            crate_name: krate.name.clone(),
            version: krate.version.clone(),
            from_stage: from.to_string(),
            to_stage: to_stage.clone(),
            status: DeferralStatus::Pending,
            kind: DeferralKind::Branch,
            deferred_at: now.format("%Y%m%d.%H%M%S").to_string(),
            source_hash: lock.source_hash.clone(),
            command: vec![],
            reason: String::new(),
        };

        deferral.write(repo_root)?;
        self.notifier.on_deferred(&deferral)?;
        Ok(deferral)
    }

    /// Confirm a pending deferral. For branch deferrals, this
    /// automatically executes the merge and push. The ticket is
    /// only marked confirmed after the merge succeeds — if the
    /// merge fails, the ticket remains pending.
    // qual:allow(iosp) reason: "integration root — orchestrates validation + merge + confirm"
    pub fn confirm_deferral(
        &self,
        repo_root: &Path,
        ticket: &str,
        reason: &str,
    ) -> Result<Deferral> {
        let d = Deferral::read(repo_root, ticket)?;
        if d.status != DeferralStatus::Pending {
            anyhow::bail!("deferral '{}' is already {:?}", ticket, d.status,);
        }

        if d.kind == DeferralKind::Branch {
            let branch_cfg =
                self.config.branch_pipeline.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("branch pipeline not configured in promote.toml")
                })?;

            // Re-verify hash before merging.
            let lock = domain::promote_lock::PromoteLock::read(repo_root)?;
            lock.verify_hash(repo_root)?;

            let git = infra::git::local::LocalGit::new(repo_root.to_path_buf());

            // Merge first — only mark confirmed if this succeeds.
            domain::pipeline::BranchPipeline::branch(
                &branch_cfg.stages,
                &d.from_stage,
                &git,
                &git,
                repo_root,
            )?;

            eprintln!(
                "=> branch merge complete: '{}' -> '{}'",
                d.from_stage, d.to_stage,
            );
        }

        // Status update happens after side effects succeed.
        let d = Deferral::confirm(repo_root, ticket, reason)?;
        Ok(d)
    }

    /// Reject a pending deferral. No side effects beyond status
    /// update.
    pub fn reject_deferral(repo_root: &Path, ticket: &str, reason: &str) -> Result<Deferral> {
        Deferral::reject(repo_root, ticket, reason)
    }

    /// List all deferrals (optionally filtered to pending only).
    // qual:allow(iosp) reason: "thin delegation with filter flag"
    pub fn deferrals(repo_root: &Path, pending_only: bool) -> Result<Vec<Deferral>> {
        if pending_only {
            Deferral::list_pending(repo_root)
        } else {
            Deferral::list(repo_root)
        }
    }
}

/// Result of a `publish_all` operation.
#[derive(Debug)]
pub struct PublishAllResult {
    /// Crates in topological publish order.
    pub publish_order: Vec<String>,
    /// Number of successfully published crates.
    pub ok: usize,
    /// Names of crates that failed to publish.
    pub failed: Vec<String>,
    /// Names of crates blocked by path-only dependencies.
    pub blocked: Vec<String>,
}

