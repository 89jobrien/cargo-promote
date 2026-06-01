//! Conformance tests for the PipelineRunner port.

use cargo_promote::domain::traits::PipelineRunner;
use cargo_promote::domain::{CrateRef, Pipeline, PromoteError, PublishOpts, Registry, Stage};
use std::cell::RefCell;
use std::path::PathBuf;

struct InMemoryRunner {
    stages_run: RefCell<Vec<String>>,
}

impl InMemoryRunner {
    fn new() -> Self {
        Self {
            stages_run: RefCell::new(vec![]),
        }
    }

    fn stages(&self) -> Vec<String> {
        self.stages_run.borrow().clone()
    }
}

impl PipelineRunner for InMemoryRunner {
    fn run_stage(
        &self,
        _krate: &CrateRef,
        stage: &Stage,
        _opts: &PublishOpts,
    ) -> Result<(), PromoteError> {
        self.stages_run
            .borrow_mut()
            .push(stage.registry.name.clone());
        Ok(())
    }

    fn run_full(
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

    fn promote_next(
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

fn test_crate() -> CrateRef {
    CrateRef {
        name: "test-crate".to_string(),
        version: "0.1.0".to_string(),
        manifest_path: PathBuf::from("Cargo.toml"),
    }
}

fn reg(name: &str) -> Registry {
    Registry {
        name: name.to_string(),
        cargo_name: Some(name.to_string()),
        api_url: None,
        confirm: false,
    }
}

fn two_stage_pipeline() -> Pipeline {
    Pipeline {
        name: "default".to_string(),
        stages: vec![Stage { registry: reg("a") }, Stage { registry: reg("b") }],
    }
}

fn assert_runner_contract(runner: &dyn PipelineRunner) {
    let krate = test_crate();
    let pl = two_stage_pipeline();
    let opts = PublishOpts::default();

    let result = runner.run_stage(&krate, &pl.stages[0], &opts);
    assert!(
        result.is_ok() || matches!(result, Err(PromoteError::PublishFailed { .. })),
        "run_stage must return Ok or PublishFailed"
    );
}

#[test]
fn conformance_runner_contract() {
    assert_runner_contract(&InMemoryRunner::new());
}

#[test]
fn conformance_run_full_visits_all_stages() {
    let runner = InMemoryRunner::new();
    let opts = PublishOpts::default();
    runner
        .run_full(&test_crate(), &two_stage_pipeline(), &opts)
        .expect("run_full should succeed");
    assert_eq!(runner.stages(), vec!["a", "b"]);
}

#[test]
fn conformance_promote_next_errors_on_unknown_stage() {
    let runner = InMemoryRunner::new();
    let result = runner.promote_next(
        &test_crate(),
        &two_stage_pipeline(),
        "nonexistent",
        &PublishOpts::default(),
    );
    assert!(matches!(result, Err(PromoteError::StageNotFound { .. })));
}

#[test]
fn conformance_promote_next_errors_on_last_stage() {
    let runner = InMemoryRunner::new();
    let result = runner.promote_next(
        &test_crate(),
        &two_stage_pipeline(),
        "b",
        &PublishOpts::default(),
    );
    assert!(matches!(result, Err(PromoteError::NoNextStage { .. })));
}
