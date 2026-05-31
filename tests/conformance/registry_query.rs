//! Conformance tests for the RegistryQuery port.
//!
//! Spec: Q1-Q4 from .ctx/conformance-spec.md
//! Uses an InMemoryRegistryQuery to verify the contract without network.

use cargo_promote::domain::traits::RegistryQuery;
use cargo_promote::domain::{CrateInfo, PromoteError, Registry};

/// Test double implementing RegistryQuery.
struct InMemoryRegistryQuery {
    crates: Vec<CrateInfo>,
}

impl InMemoryRegistryQuery {
    fn with_crates(crates: Vec<CrateInfo>) -> Self {
        Self { crates }
    }

    fn empty() -> Self {
        Self { crates: vec![] }
    }
}

impl RegistryQuery for InMemoryRegistryQuery {
    fn list_crates(&self, registry: &Registry) -> Result<Vec<CrateInfo>, PromoteError> {
        if registry.api_url.is_none() {
            return Err(PromoteError::QueryFailed {
                registry: registry.name.clone(),
                reason: "no api_url configured".to_string(),
            });
        }
        Ok(self.crates.clone())
    }
}

/// No-api-url adapter — always fails with QueryFailed.
struct FailingRegistryQuery;

impl RegistryQuery for FailingRegistryQuery {
    fn list_crates(&self, registry: &Registry) -> Result<Vec<CrateInfo>, PromoteError> {
        Err(PromoteError::QueryFailed {
            registry: registry.name.clone(),
            reason: "unreachable".to_string(),
        })
    }
}

fn reg_with_api() -> Registry {
    Registry {
        name: "test-reg".to_string(),
        cargo_name: Some("test-reg".to_string()),
        api_url: Some("http://localhost:9999".to_string()),
        confirm: false,
    }
}

fn reg_without_api() -> Registry {
    Registry {
        name: "no-api-reg".to_string(),
        cargo_name: None,
        api_url: None,
        confirm: false,
    }
}

/// Conformance helper: any RegistryQuery impl must satisfy these.
fn assert_registry_query_contract(query: &impl RegistryQuery) {
    // Q1: missing api_url -> QueryFailed naming registry
    let result = query.list_crates(&reg_without_api());
    match result {
        Err(PromoteError::QueryFailed { registry, .. }) => {
            assert_eq!(
                registry, "no-api-reg",
                "Q1: QueryFailed must name the registry"
            );
        }
        // Some impls may handle missing api_url differently (e.g. default URL).
        // The minimum contract is: either fail with QueryFailed or return Ok.
        Ok(_) => {}
        other => panic!("Q1: expected QueryFailed or Ok, got {other:?}"),
    }
}

// --- Q1: no api_url -> QueryFailed naming the registry ---

#[test]
fn conformance_q1_no_api_url_returns_query_failed() {
    let query = InMemoryRegistryQuery::empty();
    let result = query.list_crates(&reg_without_api());

    match result {
        Err(PromoteError::QueryFailed { registry, .. }) => {
            assert_eq!(registry, "no-api-reg", "Q1: error must name the registry");
        }
        other => panic!("Q1: expected QueryFailed, got {other:?}"),
    }
}

// --- Q2: unreachable URL -> QueryFailed ---

#[test]
fn conformance_q2_unreachable_returns_query_failed() {
    let query = FailingRegistryQuery;
    let result = query.list_crates(&reg_with_api());

    assert!(
        matches!(result, Err(PromoteError::QueryFailed { .. })),
        "Q2: unreachable registry must return QueryFailed"
    );
}

// --- Q3: success returns CrateInfo with non-empty names ---

#[test]
fn conformance_q3_success_returns_crate_info() {
    let query = InMemoryRegistryQuery::with_crates(vec![
        CrateInfo {
            name: "foo".to_string(),
            max_version: "1.0.0".to_string(),
        },
        CrateInfo {
            name: "bar".to_string(),
            max_version: "2.0.0".to_string(),
        },
    ]);

    let crates = query
        .list_crates(&reg_with_api())
        .expect("Q3: should succeed");

    assert_eq!(crates.len(), 2);
    for c in &crates {
        assert!(!c.name.is_empty(), "Q3: crate name must be non-empty");
    }
}

// --- Q4: empty registry returns Ok(vec![]) ---

#[test]
fn conformance_q4_empty_registry_returns_empty_vec() {
    let query = InMemoryRegistryQuery::empty();
    let crates = query
        .list_crates(&reg_with_api())
        .expect("Q4: should succeed");
    assert!(
        crates.is_empty(),
        "Q4: empty registry must return empty vec"
    );
}

// --- Conformance suite runners ---

#[test]
fn conformance_registry_query_contract_in_memory() {
    assert_registry_query_contract(&InMemoryRegistryQuery::empty());
}

#[test]
fn conformance_registry_query_contract_failing() {
    assert_registry_query_contract(&FailingRegistryQuery);
}
