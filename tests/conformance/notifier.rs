//! Conformance tests for the Notifier port.

use cargo_promote::domain::deferral::{Deferral, DeferralKind, DeferralStatus};
use cargo_promote::domain::traits::Notifier;
use cargo_promote::infra::notify::{NoopNotifier, SpawnNotifier};

fn sample_deferral() -> Deferral {
    Deferral {
        ticket: "d-test".to_string(),
        crate_name: "mycrate".to_string(),
        version: "0.1.0".to_string(),
        from_stage: "staging".to_string(),
        to_stage: "production".to_string(),
        status: DeferralStatus::Pending,
        kind: DeferralKind::Registry,
        deferred_at: "20260531.120000".to_string(),
        source_hash: "sha256:abc".to_string(),
        command: vec![],
        reason: String::new(),
        pr_number: None,
    }
}

fn assert_notifier_contract(notifier: &dyn Notifier) {
    let d = sample_deferral();
    // Must not panic; may return Ok or Err.
    let _ = notifier.on_deferred(&d);
}

#[test]
fn conformance_noop_notifier() {
    assert_notifier_contract(&NoopNotifier);
}

#[test]
fn conformance_spawn_notifier_empty_command() {
    assert_notifier_contract(&SpawnNotifier { command: vec![] });
}

#[test]
fn conformance_noop_always_ok() {
    let result = NoopNotifier.on_deferred(&sample_deferral());
    assert!(result.is_ok());
}

#[test]
fn conformance_spawn_empty_command_ok() {
    let n = SpawnNotifier { command: vec![] };
    let result = n.on_deferred(&sample_deferral());
    assert!(result.is_ok());
}
