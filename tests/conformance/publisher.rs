//! Conformance tests for the Publisher port.
//!
//! Spec: P1-P4 from .ctx/conformance-spec.md
//! These tests use an InMemoryPublisher to verify the contract
//! without network access.

use cargo_promote::domain::traits::Publisher;
use cargo_promote::domain::{CrateRef, PromoteError, PublishOpts, Registry};
use std::cell::RefCell;
use std::path::PathBuf;

/// Test double that records calls and can be configured to fail.
struct InMemoryPublisher {
    calls: RefCell<Vec<PublishCall>>,
    should_fail: bool,
}

#[derive(Debug, Clone)]
struct PublishCall {
    crate_name: String,
    crate_version: String,
    registry_name: String,
    allow_dirty: bool,
    dry_run: bool,
}

impl InMemoryPublisher {
    fn succeeding() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            should_fail: false,
        }
    }

    fn failing() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            should_fail: true,
        }
    }

    fn calls(&self) -> Vec<PublishCall> {
        self.calls.borrow().clone()
    }
}

impl Publisher for InMemoryPublisher {
    fn publish(
        &self,
        krate: &CrateRef,
        registry: &Registry,
        opts: &PublishOpts,
    ) -> Result<(), PromoteError> {
        self.calls.borrow_mut().push(PublishCall {
            crate_name: krate.name.clone(),
            crate_version: krate.version.clone(),
            registry_name: registry.name.clone(),
            allow_dirty: opts.allow_dirty,
            dry_run: opts.dry_run,
        });

        if self.should_fail {
            Err(PromoteError::PublishFailed {
                registry: registry.name.clone(),
                reason: "simulated failure".to_string(),
            })
        } else {
            Ok(())
        }
    }
}

fn test_registry() -> Registry {
    Registry {
        name: "test-reg".to_string(),
        cargo_name: Some("test-reg".to_string()),
        api_url: None,
        confirm: false,
    }
}

fn test_crate() -> CrateRef {
    CrateRef {
        name: "my-crate".to_string(),
        version: "1.2.3".to_string(),
        manifest_path: PathBuf::from("Cargo.toml"),
    }
}

/// Conformance helper: any Publisher impl must satisfy these invariants.
fn assert_publisher_contract(pub_impl: &impl Publisher) {
    let krate = test_crate();
    let reg = test_registry();
    let opts = PublishOpts::default();

    // Contract: publish with valid inputs returns Ok or a well-formed error
    let result = pub_impl.publish(&krate, &reg, &opts);
    assert!(
        result.is_ok() || matches!(result, Err(PromoteError::PublishFailed { .. })),
        "publish must return Ok or PublishFailed, got: {result:?}"
    );
}

// --- P1: publish with valid inputs returns Ok ---

#[test]
fn conformance_p1_publish_success() {
    let pub_ = InMemoryPublisher::succeeding();
    let result = pub_.publish(&test_crate(), &test_registry(), &PublishOpts::default());
    assert!(result.is_ok(), "P1: successful publish must return Ok");
}

// --- P2: failure returns PublishFailed with registry name ---

#[test]
fn conformance_p2_publish_failure_names_registry() {
    let pub_ = InMemoryPublisher::failing();
    let result = pub_.publish(&test_crate(), &test_registry(), &PublishOpts::default());

    match result {
        Err(PromoteError::PublishFailed { registry, .. }) => {
            assert_eq!(
                registry, "test-reg",
                "P2: PublishFailed must contain the registry name"
            );
        }
        other => panic!("P2: expected PublishFailed, got {other:?}"),
    }
}

// --- P3: publish receives exact inputs (no mutation) ---

#[test]
fn conformance_p3_publish_receives_exact_inputs() {
    let pub_ = InMemoryPublisher::succeeding();
    let krate = CrateRef {
        name: "exact-crate".to_string(),
        version: "9.8.7".to_string(),
        manifest_path: PathBuf::from("/some/path/Cargo.toml"),
    };
    let reg = Registry {
        name: "exact-reg".to_string(),
        cargo_name: Some("exact-cargo".to_string()),
        api_url: Some("http://example.com".to_string()),
        confirm: true,
    };
    let opts = PublishOpts {
        allow_dirty: true,
        dry_run: true,
        ..Default::default()
    };

    pub_.publish(&krate, &reg, &opts)
        .expect("P3: publish should succeed");

    let calls = pub_.calls();
    assert_eq!(calls.len(), 1, "P3: exactly one call recorded");
    let call = &calls[0];
    assert_eq!(call.crate_name, "exact-crate");
    assert_eq!(call.crate_version, "9.8.7");
    assert_eq!(call.registry_name, "exact-reg");
    assert!(call.allow_dirty, "P3: allow_dirty must be forwarded");
    assert!(call.dry_run, "P3: dry_run must be forwarded");
}

// --- P4: dry_run flag is forwarded ---

#[test]
fn conformance_p4_dry_run_forwarded() {
    let pub_ = InMemoryPublisher::succeeding();
    let opts = PublishOpts {
        dry_run: true,
        ..Default::default()
    };

    pub_.publish(&test_crate(), &test_registry(), &opts)
        .expect("P4: dry_run publish should succeed");

    let calls = pub_.calls();
    assert!(
        calls[0].dry_run,
        "P4: dry_run flag must reach the publisher"
    );
}

// --- Conformance suite runner ---

#[test]
fn conformance_publisher_contract_succeeding() {
    assert_publisher_contract(&InMemoryPublisher::succeeding());
}

#[test]
fn conformance_publisher_contract_failing() {
    assert_publisher_contract(&InMemoryPublisher::failing());
}
