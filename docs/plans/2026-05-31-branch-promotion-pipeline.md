# Branch Promotion Pipeline

**Date**: 2026-05-31
**Status**: Design approved

## Goal

Replace the current registry-only promotion model with a branch-based
pipeline where stages are git branches with CI gates. Code enters the
pipeline on the first branch, gets bumped once, and flows forward
through fast-forward merges. Each stage is triggered by CI success on
the previous stage. The pipeline is stateless — each invocation does
one hop.

## Mental Model

```
feature branch
    |
    v
develop  --[CI green]--> staging --[CI green]--> production
    ^                                                                  |
    | autobump here                                        tag + publish
    | promote.lock written                            (CI on tag does
    |                                                  registry publish)
```

- Version bump happens once, at pipeline entry (first CI gate passes on `develop`).
- `promote.lock` is written at bump time with a hash of publishable source files.
- Each subsequent `branch` invocation verifies the hash before merging forward.
- `publish` creates a git tag on the release branch; a CI workflow on the tag event handles registry publishing.

## Commands

### `cargo-promote branch --from <stage>`

1. Look up `<stage>` in promote.toml, determine the next stage.
2. Validate branch invariant: source branch must be fast-forwardable
   to target. If target has diverged (commits not in source), hard
   stop with error.
3. Read `promote.lock`, recompute source hash, compare. Mismatch = hard stop.
4. Fast-forward merge source → target.
5. Push target branch.

`--to <stage>` optional override for the target. Normally derived
from pipeline order.

### `cargo-promote publish`

1. Verify current branch is the configured `release_branch`.
2. Read version from `Cargo.toml`.
3. Create git tag `v{version}`.
4. Push commit + tag.
5. Done. CI on tag event handles `cargo publish` to registries.

### `cargo-promote ship`

Sugar: run `branch` through all remaining stages from current position, then `publish` at the end.

### `cargo-promote bump`

Standalone version bump for use in CI:

1. Bump version in `Cargo.toml` (level from config or CLI arg).
2. Compute source hash of publishable files (`src/`, `Cargo.toml`, `Cargo.lock`).
3. Write `promote.lock`:
   ```yaml
   version: "0.2.0"
   source_hash: "sha256:abc123..."
   bumped_at: "20260531::180000"
   entered_pipeline: "develop"
   ```
4. Commit both `Cargo.toml` and `promote.lock`.
5. Push.

This is called by CI on the first stage when tests pass, before the first `branch` invocation.

### Unchanged

- `cargo-promote list` — list crates on a registry
- `cargo-promote status` — show local crate versions

## promote.toml Schema

```toml
autobump = "patch"                  # patch | minor | major

[pipeline]
stages = ["develop", "staging", "production"]
release_branch = "production"      # default: last stage

[pipeline.ci]
timeout = 900                       # seconds, default 15 min
# per-stage overrides possible:
# [pipeline.stages.staging]
# timeout = 1800

[registries.cratebox]
cargo_name = "cratebox"

[registries.crates-io]
confirm = true
```

## promote.lock Schema

Written by `cargo-promote bump`, verified by `cargo-promote branch`.

```yaml
version: "0.2.0"
source_hash: "sha256:e3b0c44298..."
bumped_at: "20260531::180000"
entered_pipeline: "develop"
```

### Source Hash

SHA-256 of the concatenated content of publishable files:

- `src/**/*.rs`
- `Cargo.toml`
- `Cargo.lock`

Excludes: `promote.lock`, `promote.toml`, `tests/`, `docs/`,
`.github/`, `.ctx/`, `.gitignore`.

The hash is recomputed at each `branch` invocation and compared to
the lock file. Any mismatch (code changed mid-pipeline) is a hard
stop.

## Architecture

### Ports (traits)

```rust
/// Merge source branch into target via fast-forward.
pub trait BranchMerger {
    fn fast_forward(
        &self, source: &str, target: &str,
    ) -> Result<(), PromoteError>;
}

/// Push a branch or tag to a remote.
pub trait RemotePusher {
    fn push_branch(&self, branch: &str) -> Result<(), PromoteError>;
    fn push_tag(&self, tag: &str) -> Result<(), PromoteError>;
}

/// Create a git tag.
pub trait Tagger {
    fn create_tag(
        &self, name: &str, message: &str,
    ) -> Result<(), PromoteError>;
}
```

### Adapters

- `GitCliMerger` — shells out to `git merge --ff-only`
- `GitCliPusher` — shells out to `git push`
- `GitCliTagger` — shells out to `git tag -a`

gix does not support push or full merges yet. When it does, swap
adapters without touching domain logic.

### Domain

- `BranchPipeline` — new orchestrator analogous to `PipelineEngine`.
  Takes `BranchMerger + RemotePusher` bounds. Owns the
  validate-hash-merge-push loop.
- `PromoteLock` — reads/writes `promote.lock`, computes source
  hashes.
- `version` module — unchanged, already handles bumping.

### Crates affected

Single crate (`cargo-promote`). No new crates.

### New dependencies

- `sha2` — for source hash computation
- No `gix` yet (push/merge not ready). Re-evaluate when gix 0.85+
  ships push support.

## CI Integration Pattern

Each branch has a workflow like:

The first stage (`develop`) has a bump step:

```yaml
# .github/workflows/ci-develop.yml
- run: cargo-promote bump
  if: success()
- run: cargo-promote branch --from develop
  if: success()
```

Subsequent stages:

```yaml
# .github/workflows/ci-staging.yml
name: CI (staging)
on:
  push:
    branches: [staging]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test
      - run: cargo-promote branch --from staging
        if: success()
```

Tag-triggered publish:

```yaml
# .github/workflows/release.yml
on:
  push:
    tags: ["v*"]
jobs:
  publish:
    steps:
      - run: cargo publish --registry cratebox
      - run: cargo publish
```

## Branch Protection

Promotion branches (`staging`, `production`) should have branch
protection rules preventing direct pushes. Only the CI service
account (via cargo-promote) should push to them. cargo-promote
validates this invariant at runtime but does not enforce it —
enforcement lives in GitHub/Gitea branch protection settings.

## What's Out of Scope

- gix integration (blocked on push/merge support)
- Webhook/event-driven CI notification (using CI-triggered chain)
- Multi-crate workspace pipeline (single crate per pipeline for now)
- Rollback/revert automation
- `publish-all` with branch pipelines (keeps existing registry-only
  behavior)

## Migration Path

1. Implement `bump`, `branch`, new `publish` commands
2. Remove old `promote` subcommand
3. Existing `publish` (registry-only) behavior moves to CI workflows
4. `ship` becomes the chain runner
5. Old promote.toml configs with only `[registries]` and
   `[pipelines]` still work — `[pipeline]` section is optional and
   enables branch mode
