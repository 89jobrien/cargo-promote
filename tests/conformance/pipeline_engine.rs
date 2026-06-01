//! Conformance tests for PipelineEngine.
//!
//! Spec: E1-E7 from .ctx/conformance-spec.md

use cargo_promote::domain::pipeline::PipelineEngine;
use cargo_promote::domain::traits::{Publisher, RegistryQuery};
use cargo_promote::domain::{CrateInfo, CrateRef, Pipeline, PromoteError, PublishOpts, Registry, Stage};
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

fn three_stage_pipeline() -> Pipeline {
    Pipeline {
        name: "triple".to_string(),
        stages: vec![
            Stage {
                registry: reg("dev", false),
            },
            Stage {
                registry: reg("staging", false),
            },
            Stage {
                registry: reg("prod", true),
            },
        ],
    }
}

fn skip_opts() -> PublishOpts {
    PublishOpts {
        skip_confirm: true,
        ..Default::default()
    }
}

// --- E1: run_full invokes publish once per stage, in order ---

#[test]
fn conformance_e1_run_full_all_stages_in_order() {
    let pub_ = RecordingPublisher::new();
    let engine = PipelineEngine::new(&pub_, |_| true);

    engine
        .run_full(&test_crate(), &three_stage_pipeline(), &skip_opts())
        .expect("E1: run_full should succeed");

    assert_eq!(
        pub_.published(),
        vec!["dev", "staging", "prod"],
        "E1: must publish to all stages in declaration order"
    );
}

// --- E2: confirm=true + denied -> Aborted, no publish ---

#[test]
fn conformance_e2_confirm_denied_aborts() {
    let pub_ = RecordingPublisher::new();
    let engine = PipelineEngine::new(&pub_, |_| false);
    let stage = Stage {
        registry: reg("prod", true),
    };

    let result = engine.run_stage(&test_crate(), &stage, &PublishOpts::default());

    assert!(
        matches!(result, Err(PromoteError::Aborted)),
        "E2: denied confirmation must return Aborted"
    );
    assert!(
        pub_.published().is_empty(),
        "E2: publish must NOT be called after denial"
    );
}

// --- E3: skip_confirm=true -> confirmer never called ---

#[test]
fn conformance_e3_skip_confirm_bypasses_confirmer() {
    let pub_ = RecordingPublisher::new();
    let engine = PipelineEngine::new(&pub_, |_| {
        panic!("E3: confirmer must not be called when skip_confirm=true")
    });
    let stage = Stage {
        registry: reg("prod", true),
    };

    engine
        .run_stage(&test_crate(), &stage, &skip_opts())
        .expect("E3: should succeed without confirmation");

    assert_eq!(pub_.published(), vec!["prod"]);
}

// --- E4: dry_run=true -> confirmer never called ---

#[test]
fn conformance_e4_dry_run_bypasses_confirmer() {
    let pub_ = RecordingPublisher::new();
    let engine = PipelineEngine::new(&pub_, |_| {
        panic!("E4: confirmer must not be called when dry_run=true")
    });
    let stage = Stage {
        registry: reg("prod", true),
    };
    let opts = PublishOpts {
        dry_run: true,
        ..Default::default()
    };

    engine
        .run_stage(&test_crate(), &stage, &opts)
        .expect("E4: dry_run should succeed without confirmation");
}

// --- E5: promote_next from unknown stage -> StageNotFound ---

#[test]
fn conformance_e5_unknown_stage_returns_stage_not_found() {
    let pub_ = RecordingPublisher::new();
    let engine = PipelineEngine::new(&pub_, |_| true);

    let result = engine.promote_next(
        &test_crate(),
        &three_stage_pipeline(),
        "nonexistent",
        &skip_opts(),
    );

    assert!(
        matches!(result, Err(PromoteError::StageNotFound { .. })),
        "E5: unknown stage must return StageNotFound"
    );
}

// --- E6: promote_next from last stage -> NoNextStage ---

#[test]
fn conformance_e6_last_stage_returns_no_next() {
    let pub_ = RecordingPublisher::new();
    let engine = PipelineEngine::new(&pub_, |_| true);

    let result = engine.promote_next(&test_crate(), &three_stage_pipeline(), "prod", &skip_opts());

    assert!(
        matches!(result, Err(PromoteError::NoNextStage { .. })),
        "E6: last stage must return NoNextStage"
    );
}

// --- E7: promote_next from mid-stage publishes only the next stage ---

#[test]
fn conformance_e7_promote_next_publishes_only_next() {
    let pub_ = RecordingPublisher::new();
    let engine = PipelineEngine::new(&pub_, |_| true);

    engine
        .promote_next(&test_crate(), &three_stage_pipeline(), "dev", &skip_opts())
        .expect("E7: promote_next should succeed");

    assert_eq!(
        pub_.published(),
        vec!["staging"],
        "E7: must publish only the next stage, not subsequent ones"
    );
}

// --- Skip-if-already-published tests (S1-S3) ---

struct StubRegistryQuery {
    exists: bool,
}

impl RegistryQuery for StubRegistryQuery {
    fn list_crates(&self, _registry: &Registry) -> Result<Vec<CrateInfo>, PromoteError> {
        Ok(vec![])
    }

    fn crate_exists(
        &self,
        _registry: &Registry,
        _name: &str,
        _version: &str,
    ) -> Result<bool, PromoteError> {
        Ok(self.exists)
    }
}

// S1: crate_exists=true -> publish is skipped
#[test]
fn conformance_s1_skip_when_already_published() {
    let pub_ = RecordingPublisher::new();
    let query = StubRegistryQuery { exists: true };
    let engine = PipelineEngine::with_query(&pub_, query, |_| true);
    let stage = Stage {
        registry: reg("staging", false),
    };

    engine
        .run_stage(&test_crate(), &stage, &skip_opts())
        .expect("S1: should succeed (skip)");

    assert!(
        pub_.published().is_empty(),
        "S1: publish must NOT be called when crate already exists"
    );
}

// S2: crate_exists=true + force=true -> publish runs
#[test]
fn conformance_s2_force_overrides_skip() {
    let pub_ = RecordingPublisher::new();
    let query = StubRegistryQuery { exists: true };
    let engine = PipelineEngine::with_query(&pub_, query, |_| true);
    let stage = Stage {
        registry: reg("staging", false),
    };
    let opts = PublishOpts {
        force: true,
        skip_confirm: true,
        ..Default::default()
    };

    engine
        .run_stage(&test_crate(), &stage, &opts)
        .expect("S2: should succeed (force)");

    assert_eq!(
        pub_.published(),
        vec!["staging"],
        "S2: publish must be called when force=true even if crate exists"
    );
}

// S3: crate_exists=false -> publish runs normally
#[test]
fn conformance_s3_publish_when_not_exists() {
    let pub_ = RecordingPublisher::new();
    let query = StubRegistryQuery { exists: false };
    let engine = PipelineEngine::with_query(&pub_, query, |_| true);
    let stage = Stage {
        registry: reg("staging", false),
    };

    engine
        .run_stage(&test_crate(), &stage, &skip_opts())
        .expect("S3: should succeed");

    assert_eq!(
        pub_.published(),
        vec!["staging"],
        "S3: publish must be called when crate does not exist"
    );
}
