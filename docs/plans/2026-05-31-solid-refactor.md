# SOLID Refactor: Dependency Injection and Layer Cleanup

**Date**: 2026-05-31
**Status**: Approved design, pending implementation

## Goal

Apply SOLID principles and hexagonal architecture cleanup to cargo-promote.
Eliminate domain/infra boundary violations, make `Api` fully injectable,
consolidate duplicated git abstractions, and introduce domain-level
notification.

## Architecture Changes

### 1. `Api` — all trait objects (Decision L)

Remove the generic parameter from `Api`. All ports use dynamic dispatch.

```rust
pub struct Api {
    config: Config,
    engine: Box<dyn PipelineRunner>,
    registry_query: Box<dyn RegistryQuery>,
    notifier: Box<dyn Notifier>,
}
```

**Rationale**: `Api` is constructed once (in `main.rs` or tests). The
generic `P: Publisher` only served `PipelineEngine` internally and leaked
into every call site. Erasing it simplifies the API surface with no
meaningful performance cost.

### 2. `PipelineRunner` trait (new)

```rust
pub trait PipelineRunner {
    fn run_stage(
        &self,
        krate: &CrateRef,
        stage: &Stage,
        opts: &PublishOpts,
    ) -> Result<(), PromoteError>;

    fn run_full(
        &self,
        krate: &CrateRef,
        pipeline: &Pipeline,
        opts: &PublishOpts,
    ) -> Result<(), PromoteError>;

    fn promote_next(
        &self,
        krate: &CrateRef,
        pipeline: &Pipeline,
        current_stage: &str,
        opts: &PublishOpts,
    ) -> Result<(), PromoteError>;
}
```

`PipelineEngine<P: Publisher>` implements `PipelineRunner`. The existing
generic stays internal to `PipelineEngine` — it is no longer exposed
through `Api`.

### 3. `Notifier` trait — domain-level events (Decision P)

```rust
pub trait Notifier {
    fn on_deferred(&self, deferral: &Deferral) -> Result<(), PromoteError>;
}
```

The trait speaks domain language. Adapters decide delivery semantics:

- `SpawnNotifier` — runs a configured shell command, swallows errors
  (matches current fire-and-forget behavior)
- `NoopNotifier` — does nothing (tests, library usage)

This removes `command: Vec<String>` from `defer_to` and `defer_branch`
signatures. The notifier receives its command config at construction time.

### 4. `LocalGit` consolidation (Decision D)

Replace `LocalGit` (static methods) + `GitCliMerger` + `GitCliPusher` +
`GitCliTagger` with a single struct:

```rust
pub struct LocalGit {
    pub repo_root: PathBuf,
}

impl BranchMerger for LocalGit { ... }
impl RemotePusher for LocalGit { ... }
impl Tagger for LocalGit { ... }
```

Delete `GitCliMerger`, `GitCliPusher`, `GitCliTagger`.

Additional concrete methods (not behind traits):

- `fn stage(&self, files: &[&str]) -> Result<(), PromoteError>`
- `fn commit(&self, message: &str) -> Result<(), PromoteError>`
- `fn push_head(&self) -> Result<(), PromoteError>`
- `fn is_dirty(&self) -> Result<bool, PromoteError>`
- `fn current_branch(&self) -> Result<String, PromoteError>`

### 5. `BranchPipeline::bump` — extract shell calls (Decision K)

Move `std::process::Command` calls out of `bump()` into `LocalGit`
concrete methods. `bump()` accepts `&LocalGit` directly (no trait).

```rust
pub fn bump(
    krate: &CrateRef,
    stages: &[String],
    repo_path: &Path,
    git: &LocalGit,
) -> Result<(), PromoteError> {
    // ... version bump and promote.lock (pure domain) ...
    git.stage(&["Cargo.toml", "promote.lock"])?;
    git.commit(&format!("bump: {} v{}", krate.name, new_version))?;
    git.push_head()?;
    Ok(())
}
```

No new traits — these git operations are mechanical and only need
integration testing, not mocking.

### 6. `confirm_deferral` boilerplate

Self-resolves with Decision D. After consolidation, both `Api::branch`
and `Api::confirm_deferral` construct a single `LocalGit { repo_root }`
instead of three separate wrapper structs.

## Tech Decisions

| Decision                                      | Why                                                                               | Alternative rejected                                                                                |
| --------------------------------------------- | --------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| All trait objects on `Api` (L)                | Uniform style, no generic leakage                                                 | Mixed generic+trait-object (C) — inconsistent extension pattern                                     |
| Domain-level `Notifier` (P)                   | Adapter decides error semantics, removes infra leak                               | Fallible trait (N) — adds `let _ =` boilerplate; Infallible (O) — forecloses critical notifications |
| Concrete `LocalGit` for stage/commit/push (K) | cargo-release and release-plz don't trait-abstract git; integration tests suffice | Traits for stage/commit (F/G/J) — over-abstraction for operations with one implementation           |
| Single `LocalGit` struct (D)                  | Three wrappers shared same `repo_root` and delegated to static methods            | Keep wrappers (E) — more types for no benefit                                                       |

## Out of Scope

- `Config::default_config` env var injection — minor, defer
- Adding new registry backends (GitHub Packages publish) — future work
- Async conversion — not needed for CLI tool
- `RegistryQuery` for crates.io — no API query needed currently

## Files Touched

| File                         | Change                                                                                            |
| ---------------------------- | ------------------------------------------------------------------------------------------------- |
| `src/domain/traits/mod.rs`   | Add `PipelineRunner`, `Notifier`                                                                  |
| `src/domain/pipeline/mod.rs` | `PipelineEngine` implements `PipelineRunner`; `bump()` takes `&LocalGit`                          |
| `src/lib.rs`                 | Rewrite `Api` with trait objects; remove `fire_notification`; remove `command` from defer methods |
| `src/infra/git/local/mod.rs` | Consolidate to `LocalGit { repo_root }`; add `stage`/`commit`/`push_head`; delete 3 wrappers      |
| `src/infra/notify.rs`        | New — `SpawnNotifier`, `NoopNotifier`                                                             |
| `src/main.rs`                | Wire defaults, construct `SpawnNotifier` with command config                                      |
| `tests/conformance/`         | Update for new trait shapes                                                                       |

## Implementation Order

1. Add `PipelineRunner` trait, implement on `PipelineEngine`
2. Add `Notifier` trait + adapters in `infra/notify.rs`
3. Consolidate `LocalGit` (Decision D)
4. Extract shell calls from `bump()` (Decision K)
5. Rewrite `Api` to hold trait objects (Decision L)
6. Update `main.rs` wiring
7. Update tests
