use super::traits::Publisher;
use super::{CrateRef, Pipeline, PromoteError, PublishOpts, Stage};

/// Drives a crate through pipeline stages.
pub struct PipelineEngine<P: Publisher> {
    publisher: P,
    confirmer: Box<dyn Fn(&str) -> bool>,
}

impl<P: Publisher> PipelineEngine<P> {
    pub fn new(publisher: P, confirmer: impl Fn(&str) -> bool + 'static) -> Self {
        Self {
            publisher,
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
