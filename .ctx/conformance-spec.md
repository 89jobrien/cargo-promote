# Conformance Spec — cargo-promote

## Port Boundaries

### 1. Publisher (trait: `domain::traits::Publisher`)

Contract:

- P1: `publish()` with valid inputs returns `Ok(())`
- P2: `publish()` failure returns `PromoteError::PublishFailed` with the
  registry name in the error
- P3: `publish()` receives the exact `CrateRef`, `Registry`, and
  `PublishOpts` passed by the caller (no mutation)
- P4: `dry_run` flag must not cause side effects (adapter-level, but
  the contract requires the flag is forwarded)

Impls: `CargoPublisher` (infra, shells out — not testable in
conformance without network; verified via `InMemoryPublisher`)

### 2. RegistryQuery (trait: `domain::traits::RegistryQuery`)

Contract:

- Q1: `list_crates()` with no `api_url` returns
  `PromoteError::QueryFailed` naming the registry
- Q2: `list_crates()` with an unreachable URL returns
  `PromoteError::QueryFailed`
- Q3: `list_crates()` success returns a `Vec<CrateInfo>` with
  non-empty `name` fields
- Q4: `list_crates()` on an empty registry returns `Ok(vec![])`

Impls: `GiteaRegistry`, `GitHubRegistry`

### 3. PipelineEngine (domain, generic over Publisher)

Contract:

- E1: `run_full()` invokes `publish()` once per stage, in order
- E2: `run_stage()` on a `confirm=true` registry calls the confirmer;
  denial returns `PromoteError::Aborted` and does NOT call `publish()`
- E3: `run_stage()` with `skip_confirm=true` never calls confirmer
- E4: `run_stage()` with `dry_run=true` never calls confirmer
- E5: `promote_next()` from an unknown stage returns
  `PromoteError::StageNotFound`
- E6: `promote_next()` from the last stage returns
  `PromoteError::NoNextStage`
- E7: `promote_next()` from a valid mid-stage publishes only the next
  stage

### 4. Config (loads `promote.toml` -> domain types)

Contract:

- C1: Missing `promote.toml` returns default config (minibox ->
  crates-io)
- C2: Empty TOML is valid (no registries, no pipelines)
- C3: Pipeline referencing unknown registry is an error naming the
  registry
- C4: Custom registries and pipelines round-trip correctly
- C5: `pipeline(None)` returns the pipeline named "default"
- C6: `registry("minibox")` returns the minibox registry from defaults

## Hexagonal Architecture Audit

- Domain layer (`domain/`) has zero infra imports — verified by grep
- All adapters (`infra/`) import only from `domain::traits` and
  `domain` types
- `main.rs` is the only file that imports from both `domain` and
  `infra` (composition root)
- No adapter calls another adapter
