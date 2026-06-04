use super::traits::{GitCommitter, PipelineRunner, Publisher, RegistryQuery};
use super::{CrateRef, Pipeline, PromoteError, PublishOpts, Stage};

/// Drives a crate through pipeline stages.
pub struct PipelineEngine<P: Publisher, Q: RegistryQuery> {
    publisher: P,
    registry_query: Q,
    confirmer: Box<dyn Fn(&str) -> bool>,
}

impl<P: Publisher> PipelineEngine<P, NullRegistryQuery> {
    pub fn new(publisher: P, confirmer: impl Fn(&str) -> bool + 'static) -> Self {
        Self {
            publisher,
            registry_query: NullRegistryQuery,
            confirmer: Box::new(confirmer),
        }
    }
}

impl<P: Publisher, Q: RegistryQuery> PipelineEngine<P, Q> {
    /// Construct with an explicit registry query (used by conformance tests).
    pub fn with_query(
        publisher: P,
        registry_query: Q,
        confirmer: impl Fn(&str) -> bool + 'static,
    ) -> Self {
        Self {
            publisher,
            registry_query,
            confirmer: Box::new(confirmer),
        }
    }

    /// Publish to a single stage.
    pub fn run_stage(
        &self,
        krate: &CrateRef,
        stage: &Stage,
        opts: &PublishOpts,
    ) -> Result<(), PromoteError> {
        // Skip-if-already-published guard
        if !opts.force
            && let Ok(true) =
                self.registry_query
                    .crate_exists(&stage.registry, &krate.name, &krate.version)
            {
                eprintln!(
                    "=> {} v{} already exists in '{}', skipping (use --force to override)",
                    krate.name, krate.version, stage.registry.name
                );
                return Ok(());
            }

        if stage.registry.confirm && !opts.skip_confirm && !opts.dry_run {
            let prompt = format!(
                "About to publish {} v{} to '{}'. Continue?",
                krate.name, krate.version, stage.registry.name
            );
            if !(self.confirmer)(&prompt) {
                return Err(PromoteError::Aborted);
            }
        }
        self.publisher.publish(krate, &stage.registry, opts)
    }

    /// Run all stages in the pipeline sequentially.
    pub fn run_full(
        &self,
        krate: &CrateRef,
        pipeline: &Pipeline,
        opts: &PublishOpts,
    ) -> Result<(), PromoteError> {
        for stage in &pipeline.stages {
            self.run_stage(krate, stage, opts)?;
        }
        Ok(())
    }

    /// Advance from `current_stage` to the next stage in the pipeline.
    pub fn promote_next(
        &self,
        krate: &CrateRef,
        pipeline: &Pipeline,
        current_stage: &str,
        opts: &PublishOpts,
    ) -> Result<(), PromoteError> {
        let idx = pipeline
            .stages
            .iter()
            .position(|s| s.registry.name == current_stage)
            .ok_or_else(|| PromoteError::StageNotFound {
                pipeline: pipeline.name.clone(),
                stage: current_stage.to_string(),
            })?;

        let next = pipeline
            .stages
            .get(idx + 1)
            .ok_or_else(|| PromoteError::NoNextStage {
                pipeline: pipeline.name.clone(),
                stage: current_stage.to_string(),
            })?;

        self.run_stage(krate, next, opts)
    }
}

impl<P: Publisher, Q: RegistryQuery> PipelineRunner for PipelineEngine<P, Q> {
    fn run_stage(
        &self,
        krate: &CrateRef,
        stage: &Stage,
        opts: &PublishOpts,
    ) -> Result<(), PromoteError> {
        PipelineEngine::run_stage(self, krate, stage, opts)
    }

    fn run_full(
        &self,
        krate: &CrateRef,
        pipeline: &Pipeline,
        opts: &PublishOpts,
    ) -> Result<(), PromoteError> {
        PipelineEngine::run_full(self, krate, pipeline, opts)
    }

    fn promote_next(
        &self,
        krate: &CrateRef,
        pipeline: &Pipeline,
        current_stage: &str,
        opts: &PublishOpts,
    ) -> Result<(), PromoteError> {
        PipelineEngine::promote_next(self, krate, pipeline, current_stage, opts)
    }
}

/// A no-op registry query that always returns `false` for `crate_exists`.
pub struct NullRegistryQuery;

impl RegistryQuery for NullRegistryQuery {
    fn list_crates(
        &self,
        registry: &super::Registry,
    ) -> Result<Vec<super::CrateInfo>, PromoteError> {
        let _ = registry;
        Ok(vec![])
    }
}

/// Drives a crate through git branch-based stages.
pub struct BranchPipeline;

impl BranchPipeline {
    /// Run a bump operation (version bump + promote.lock creation + commit/push).
    pub fn bump(
        krate: &CrateRef,
        stages: &[String],
        repo_path: &std::path::Path,
        git: &dyn GitCommitter,
    ) -> Result<(), PromoteError> {
        use crate::domain::promote_lock::PromoteLock;

        let (old_version, new_version) = crate::domain::version::bump_manifest_version(
            &krate.manifest_path,
            crate::domain::version::BumpLevel::Patch,
        )
        .map_err(PromoteError::Other)?;

        eprintln!("=> bumped {} v{old_version} -> v{new_version}", krate.name);

        let source_hash =
            PromoteLock::compute_source_hash(repo_path).map_err(PromoteError::Other)?;

        let entered_pipeline = stages.first().cloned().unwrap_or_default();
        let lock = PromoteLock {
            version: new_version.to_string(),
            source_hash,
            bumped_at: chrono::Local::now().format("%Y%m%d::%H%M%S").to_string(),
            entered_pipeline,
        };

        lock.write(repo_path).map_err(PromoteError::Other)?;

        git.stage(&["Cargo.toml", "promote.lock"])?;
        git.commit(&format!("bump: {} v{}", krate.name, new_version))?;
        git.push_head()?;

        Ok(())
    }

    /// Branch from one stage to the next (with hash verification).
    // qual:allow(iosp) reason: "integration root — orchestrates verify + merge + push"
    pub fn branch(
        stages: &[String],
        from_stage: &str,
        merger: &dyn crate::domain::traits::BranchMerger,
        pusher: &dyn crate::domain::traits::RemotePusher,
        repo_path: &std::path::Path,
    ) -> Result<(), PromoteError> {
        use crate::domain::promote_lock::PromoteLock;

        // Find the next stage
        let from_idx = stages
            .iter()
            .position(|s| s == from_stage)
            .ok_or_else(|| PromoteError::Other(anyhow::anyhow!("unknown stage '{from_stage}'")))?;

        let to_stage = stages.get(from_idx + 1).ok_or_else(|| {
            PromoteError::Other(anyhow::anyhow!("no next stage after '{from_stage}'"))
        })?;

        // Read and verify promote.lock
        let lock = PromoteLock::read(repo_path).map_err(PromoteError::Other)?;

        lock.verify_hash(repo_path).map_err(PromoteError::Other)?;

        eprintln!("=> hash verified, merging '{from_stage}' -> '{to_stage}'");

        // Perform fast-forward merge
        merger.fast_forward(from_stage, to_stage)?;

        // Push the target branch
        pusher.push_branch(to_stage)?;

        eprintln!("=> {to_stage} updated and pushed");

        Ok(())
    }

    /// Publish (create git tag on release branch).
    pub fn publish(
        krate: &CrateRef,
        _release_branch: &str,
        tagger: &dyn crate::domain::traits::Tagger,
        pusher: &dyn crate::domain::traits::RemotePusher,
    ) -> Result<(), PromoteError> {
        let tag = format!("v{}", krate.version);
        let message = format!("Release {} v{}", krate.name, krate.version);

        tagger.create_tag(&tag, &message)?;
        eprintln!("=> created tag '{tag}'");

        pusher.push_tag(&tag)?;
        eprintln!("=> pushed tag '{tag}'");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::traits::Publisher;
    use crate::domain::{CrateRef, PublishOpts, Registry, Stage};
    use std::cell::RefCell;
    use std::path::PathBuf;

    struct RecordingPublisher {
        published_to: RefCell<Vec<String>>,
    }

    impl RecordingPublisher {
        fn new() -> Self {
            Self {
                published_to: RefCell::new(Vec::new()),
            }
        }

        fn published(&self) -> Vec<String> {
            self.published_to.borrow().clone()
        }
    }

    impl Publisher for RecordingPublisher {
        fn publish(
            &self,
            _krate: &CrateRef,
            registry: &Registry,
            _opts: &PublishOpts,
        ) -> Result<(), PromoteError> {
            self.published_to.borrow_mut().push(registry.name.clone());
            Ok(())
        }
    }

    fn test_crate() -> CrateRef {
        CrateRef {
            name: "test-crate".to_string(),
            version: "0.1.0".to_string(),
            manifest_path: PathBuf::from("Cargo.toml"),
        }
    }

    fn reg(name: &str, confirm: bool) -> Registry {
        Registry {
            name: name.to_string(),
            cargo_name: Some(name.to_string()),
            api_url: None,
            confirm,
        }
    }

    fn two_stage_pipeline() -> Pipeline {
        Pipeline {
            name: "default".to_string(),
            stages: vec![
                Stage {
                    registry: reg("staging", false),
                },
                Stage {
                    registry: reg("production", true),
                },
            ],
        }
    }

    #[test]
    fn run_full_publishes_all_stages_in_order() {
        let pub_ = RecordingPublisher::new();
        let engine = PipelineEngine::new(&pub_, |_| true);
        let opts = PublishOpts {
            skip_confirm: true,
            ..Default::default()
        };

        engine
            .run_full(&test_crate(), &two_stage_pipeline(), &opts)
            .expect("should succeed");

        assert_eq!(pub_.published(), vec!["staging", "production"]);
    }

    #[test]
    fn run_stage_with_confirm_aborts_on_deny() {
        let pub_ = RecordingPublisher::new();
        let engine = PipelineEngine::new(&pub_, |_| false);
        let stage = Stage {
            registry: reg("production", true),
        };

        let result = engine.run_stage(&test_crate(), &stage, &PublishOpts::default());
        assert!(matches!(result, Err(PromoteError::Aborted)));
        assert!(pub_.published().is_empty());
    }

    #[test]
    fn run_stage_skips_confirm_when_flag_set() {
        let pub_ = RecordingPublisher::new();
        let engine = PipelineEngine::new(&pub_, |_| panic!("should not be called"));
        let stage = Stage {
            registry: reg("production", true),
        };
        let opts = PublishOpts {
            skip_confirm: true,
            ..Default::default()
        };

        engine
            .run_stage(&test_crate(), &stage, &opts)
            .expect("should succeed");
        assert_eq!(pub_.published(), vec!["production"]);
    }

    #[test]
    fn promote_next_advances_to_correct_stage() {
        let pub_ = RecordingPublisher::new();
        let engine = PipelineEngine::new(&pub_, |_| true);
        let opts = PublishOpts {
            skip_confirm: true,
            ..Default::default()
        };

        engine
            .promote_next(&test_crate(), &two_stage_pipeline(), "staging", &opts)
            .expect("should succeed");

        assert_eq!(pub_.published(), vec!["production"]);
    }

    #[test]
    fn promote_next_errors_on_unknown_stage() {
        let pub_ = RecordingPublisher::new();
        let engine = PipelineEngine::new(&pub_, |_| true);

        let result = engine.promote_next(
            &test_crate(),
            &two_stage_pipeline(),
            "nonexistent",
            &PublishOpts::default(),
        );
        assert!(matches!(result, Err(PromoteError::StageNotFound { .. })));
    }

    #[test]
    fn pipeline_engine_implements_pipeline_runner() {
        let pub_ = RecordingPublisher::new();
        let engine = PipelineEngine::new(&pub_, |_| true);
        let runner: &dyn crate::domain::traits::PipelineRunner = &engine;
        let opts = PublishOpts {
            skip_confirm: true,
            ..Default::default()
        };
        runner
            .run_stage(
                &test_crate(),
                &Stage {
                    registry: reg("staging", false),
                },
                &opts,
            )
            .expect("should succeed via trait object");
        assert_eq!(pub_.published(), vec!["staging"]);
    }

    // ── BranchPipeline tests ──────────────────────────────────────

    use crate::domain::traits::{BranchMerger, RemotePusher, Tagger};

    struct MockMerger {
        called: RefCell<Vec<(String, String)>>,
    }

    impl MockMerger {
        fn new() -> Self {
            Self {
                called: RefCell::new(Vec::new()),
            }
        }
    }

    impl BranchMerger for MockMerger {
        fn fast_forward(&self, source: &str, target: &str) -> Result<(), PromoteError> {
            self.called
                .borrow_mut()
                .push((source.to_string(), target.to_string()));
            Ok(())
        }
    }

    struct MockPusher {
        branches: RefCell<Vec<String>>,
        tags: RefCell<Vec<String>>,
    }

    impl MockPusher {
        fn new() -> Self {
            Self {
                branches: RefCell::new(Vec::new()),
                tags: RefCell::new(Vec::new()),
            }
        }
    }

    impl RemotePusher for MockPusher {
        fn push_branch(&self, branch: &str) -> Result<(), PromoteError> {
            self.branches.borrow_mut().push(branch.to_string());
            Ok(())
        }
        fn push_tag(&self, tag: &str) -> Result<(), PromoteError> {
            self.tags.borrow_mut().push(tag.to_string());
            Ok(())
        }
    }

    struct MockTagger {
        tags: RefCell<Vec<(String, String)>>,
    }

    impl MockTagger {
        fn new() -> Self {
            Self {
                tags: RefCell::new(Vec::new()),
            }
        }
    }

    impl Tagger for MockTagger {
        fn create_tag(&self, name: &str, message: &str) -> Result<(), PromoteError> {
            self.tags
                .borrow_mut()
                .push((name.to_string(), message.to_string()));
            Ok(())
        }
    }

    #[test]
    fn branch_pipeline_branch_merges_to_next_stage() {
        // Set up a temp dir with a valid promote.lock
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Create minimal source files so hash computes
        std::fs::write(root.join("Cargo.toml"), "name = \"test\"").unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn main() {}").unwrap();

        // Write a promote.lock with the correct hash
        let hash =
            crate::domain::promote_lock::PromoteLock::compute_source_hash(root).unwrap();
        let lock = crate::domain::promote_lock::PromoteLock {
            version: "0.1.0".to_string(),
            source_hash: hash,
            bumped_at: "20260601::120000".to_string(),
            entered_pipeline: "develop".to_string(),
        };
        lock.write(root).unwrap();

        let stages = vec![
            "develop".to_string(),
            "staging".to_string(),
            "main".to_string(),
        ];
        let merger = MockMerger::new();
        let pusher = MockPusher::new();

        BranchPipeline::branch(&stages, "develop", &merger, &pusher, root).unwrap();

        assert_eq!(
            merger.called.borrow().as_slice(),
            &[("develop".to_string(), "staging".to_string())]
        );
        assert_eq!(pusher.branches.borrow().as_slice(), &["staging"]);
    }

    #[test]
    fn branch_pipeline_branch_errors_on_unknown_stage() {
        let dir = tempfile::tempdir().unwrap();
        let stages = vec!["develop".to_string(), "main".to_string()];
        let merger = MockMerger::new();
        let pusher = MockPusher::new();

        let result =
            BranchPipeline::branch(&stages, "nonexistent", &merger, &pusher, dir.path());
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("nonexistent"),
            "error should mention the unknown stage"
        );
    }

    #[test]
    fn branch_pipeline_branch_errors_on_last_stage() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::write(root.join("Cargo.toml"), "name = \"test\"").unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn main() {}").unwrap();

        let hash =
            crate::domain::promote_lock::PromoteLock::compute_source_hash(root).unwrap();
        let lock = crate::domain::promote_lock::PromoteLock {
            version: "0.1.0".to_string(),
            source_hash: hash,
            bumped_at: "20260601::120000".to_string(),
            entered_pipeline: "develop".to_string(),
        };
        lock.write(root).unwrap();

        let stages = vec!["develop".to_string(), "main".to_string()];
        let merger = MockMerger::new();
        let pusher = MockPusher::new();

        let result = BranchPipeline::branch(&stages, "main", &merger, &pusher, root);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no next stage"));
    }

    #[test]
    fn branch_pipeline_publish_creates_and_pushes_tag() {
        let krate = test_crate();
        let tagger = MockTagger::new();
        let pusher = MockPusher::new();

        BranchPipeline::publish(&krate, "main", &tagger, &pusher).unwrap();

        assert_eq!(
            tagger.tags.borrow().as_slice(),
            &[("v0.1.0".to_string(), "Release test-crate v0.1.0".to_string())]
        );
        assert_eq!(pusher.tags.borrow().as_slice(), &["v0.1.0"]);
    }

    #[test]
    fn promote_next_errors_on_last_stage() {
        let pub_ = RecordingPublisher::new();
        let engine = PipelineEngine::new(&pub_, |_| true);

        let result = engine.promote_next(
            &test_crate(),
            &two_stage_pipeline(),
            "production",
            &PublishOpts::default(),
        );
        assert!(matches!(result, Err(PromoteError::NoNextStage { .. })));
    }
}
