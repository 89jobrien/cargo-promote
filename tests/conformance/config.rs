//! Conformance tests for Config loading.
//!
//! Spec: C1-C6 from .ctx/conformance-spec.md

use cargo_promote::config::Config;
use std::path::Path;

// --- C1: missing promote.toml -> default config ---

#[test]
fn conformance_c1_missing_config_returns_defaults() {
    // /tmp will not have a promote.toml
    let cfg =
        Config::load(Path::new("/tmp")).expect("C1: missing config should fall back to defaults");

    let pipeline = cfg.pipeline(None).expect("C1: default pipeline must exist");
    assert_eq!(pipeline.stages.len(), 2, "C1: default has 2 stages");
    assert_eq!(pipeline.stages[0].registry.name, "minibox");
    assert_eq!(pipeline.stages[1].registry.name, "crates-io");
}

// --- C2: empty TOML is valid ---

#[test]
fn conformance_c2_empty_toml_is_valid() {
    let cfg = Config::from_toml("").expect("C2: empty TOML must parse");
    assert!(cfg.pipelines.is_empty(), "C2: no pipelines in empty config");
    assert!(
        cfg.registries.is_empty(),
        "C2: no registries in empty config"
    );
}

// --- C3: pipeline referencing unknown registry errors ---

#[test]
fn conformance_c3_unknown_registry_in_pipeline_errors() {
    let toml = r#"
[pipelines.bad]
stages = ["ghost"]
"#;
    let result = Config::from_toml(toml);
    assert!(result.is_err(), "C3: must error on unknown registry");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("ghost"),
        "C3: error must name the unknown registry, got: {msg}"
    );
}

// --- C4: custom registries and pipelines round-trip ---

#[test]
fn conformance_c4_custom_config_round_trip() {
    let toml = r#"
[registries.alpha]
cargo_name = "alpha-reg"
api_url = "http://alpha.local/api"
confirm = false

[registries.beta]
cargo_name = "beta-reg"
confirm = true

[pipelines.default]
stages = ["alpha", "beta"]

[pipelines.alpha-only]
stages = ["alpha"]
"#;
    let cfg = Config::from_toml(toml).expect("C4: should parse");

    // Registries
    let alpha = cfg.registry("alpha").expect("C4: alpha must exist");
    assert_eq!(alpha.cargo_name.as_deref(), Some("alpha-reg"));
    assert_eq!(alpha.api_url.as_deref(), Some("http://alpha.local/api"));
    assert!(!alpha.confirm);

    let beta = cfg.registry("beta").expect("C4: beta must exist");
    assert!(beta.confirm);

    // Pipelines
    assert_eq!(cfg.pipelines.len(), 2);

    let default = cfg.pipeline(None).expect("C4: default pipeline must exist");
    assert_eq!(default.stages.len(), 2);
    assert_eq!(default.stages[0].registry.name, "alpha");
    assert_eq!(default.stages[1].registry.name, "beta");

    let alpha_only = cfg
        .pipeline(Some("alpha-only"))
        .expect("C4: alpha-only pipeline must exist");
    assert_eq!(alpha_only.stages.len(), 1);
}

// --- C5: pipeline(None) returns "default" ---

#[test]
fn conformance_c5_pipeline_none_returns_default() {
    let cfg = Config::default_config();
    let by_none = cfg.pipeline(None);
    let by_name = cfg.pipeline(Some("default"));

    assert!(by_none.is_some(), "C5: pipeline(None) must return Some");
    assert_eq!(
        by_none.unwrap().name,
        by_name.unwrap().name,
        "C5: pipeline(None) must match pipeline('default')"
    );
}

// --- C6: registry("minibox") exists in defaults ---

#[test]
fn conformance_c6_default_has_minibox_registry() {
    let cfg = Config::default_config();
    let minibox = cfg
        .registry("minibox")
        .expect("C6: minibox registry must exist in defaults");
    assert_eq!(minibox.cargo_name.as_deref(), Some("minibox"));
    assert!(
        !minibox.confirm,
        "C6: minibox should not require confirmation"
    );
}
