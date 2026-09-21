# Workspace Support for Branch Pipelines

**Date**: 2026-06-04
**Status**: Draft
**Depends on**: 2026-05-31-branch-promotion-pipeline.md

## Problem

`cargo-promote branch` and `cargo-promote bump` assume a single-crate
repository. They read one `Cargo.toml`, hash one `src/` directory, and
write one `promote.lock`. Rust workspaces (like minibox with 10 crates)
cannot use the branch pipeline at all.

Specific failures:

- `cargo-promote bump` fails with "missing package.name" on a virtual
  workspace manifest.
- `cargo-promote branch` fails with "cannot read promote.lock" because
  no lock was ever created.
- `publishable_files()` only looks at root `src/` — misses all member
  crates.

## Goal

Make `bump`, `branch`, `ship`, and the source-hash verification work
for Cargo workspaces. A single `promote.lock` at the workspace root
covers all member crates. The branch pipeline merges the whole repo,
not individual crates.

## Design

### Workspace detection

On startup, check if `Cargo.toml` contains `[workspace]`. If yes,
resolve member crates via `cargo_metadata`. Store the list of member
paths for hashing.

```rust
pub fn is_workspace(root: &Path) -> bool {
    let content = fs::read_to_string(root.join("Cargo.toml"))
        .unwrap_or_default();
    content.contains("[workspace]")
}
```

### Source hash: workspace-aware

`publishable_files()` changes:

- **Single crate** (no `[workspace]`): current behavior (root
  `Cargo.toml` + `Cargo.lock` + `src/**/*.rs`).
- **Workspace**: root `Cargo.toml` + `Cargo.lock` + for each member
  crate: `<member>/Cargo.toml` + `<member>/src/**/*.rs`. Members
  sorted alphabetically for determinism.

Excluded (unchanged): `promote.lock`, `promote.toml`, `tests/`,
`docs/`, `.github/`, `.ctx/`, `.gitignore`, `target/`.

### `bump` in workspace mode

Currently `bump` reads `package.version` from root `Cargo.toml`. In a
workspace, version lives in `workspace.package.version` and members
inherit via `version.workspace = true`.

Behavior:

1. Read `workspace.package.version` from root manifest.
2. Bump it (patch/minor/major per config).
3. Write back to root `Cargo.toml` only. Members that use
   `version.workspace = true` inherit automatically.
4. Compute workspace-aware source hash.
5. Write `promote.lock` at workspace root.
6. Commit `Cargo.toml` + `Cargo.lock` + `promote.lock`.

If some members have independent versions (no `version.workspace =
true`), they are left unchanged. Only the workspace version is bumped.
Per-member bump overrides are out of scope for this spec.

### `branch` in workspace mode

No changes needed to the merge/push logic. `branch` already operates
on the whole repo (git merge, git push). Only the hash verification
path changes — it must use the workspace-aware `publishable_files()`.

### `promote.lock` schema

Unchanged. The lock covers the workspace as a unit:

```yaml
version: "0.30.1"
source_hash: "sha256:..."
bumped_at: "20260604::120000"
entered_pipeline: "develop"
```

`version` is the workspace version. Individual member versions are not
tracked in the lock (they inherit from workspace or are independently
managed and excluded from the pipeline).

### `promote.toml` schema

No schema changes. Workspace repos use the same `[pipeline]` config:

```toml
[pipeline]
stages = ["develop", "next", "staging", "main"]
release_branch = "main"

autobump = "patch"
```

### `-p` / `--package` flag on `bump`

Optional. When provided in a workspace, bump only that member's
version (must have its own `package.version`, not inherited). When
omitted in a workspace, bump `workspace.package.version`. When omitted
in a single crate, bump `package.version` (current behavior).

## Affected code

| File | Change |
| --- | --- |
| `src/domain/promote_lock.rs` | `publishable_files()` workspace branch |
| `src/domain/pipeline/mod.rs` | `bump()` reads workspace version |
| `src/lib.rs` | `bump()` and `branch()` pass workspace context |
| `src/cli.rs` | No changes (flags already exist) |
| `src/config/mod.rs` | No changes |

Estimated: ~100 lines changed, no new files, no new dependencies
(`cargo_metadata` already in deps).

## Workspace-aware `publishable_files` (pseudocode)

```rust
fn publishable_files(repo_root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    files.push_if_exists(repo_root.join("Cargo.toml"));
    files.push_if_exists(repo_root.join("Cargo.lock"));

    if is_workspace(repo_root) {
        let metadata = cargo_metadata(repo_root);
        for member in metadata.workspace_members.sorted() {
            let member_dir = member.manifest_path.parent();
            files.push_if_exists(member_dir.join("Cargo.toml"));
            collect_rust_files(member_dir.join("src"), &mut files);
        }
    } else {
        collect_rust_files(repo_root.join("src"), &mut files);
    }

    files.sort();
    Ok(files)
}
```

## What's out of scope

- Per-member version bumping (independent version strategies)
- Per-member pipeline stages (all members share one pipeline)
- Selective publishing of workspace members (use `publish-all` for
  registry publishing)
- Monorepo with multiple workspaces in subdirectories

## Test plan

- [ ] `bump` on a virtual workspace manifest bumps
      `workspace.package.version`
- [ ] `bump` on a single-crate repo still works (regression)
- [ ] Source hash includes all member `src/` directories
- [ ] Source hash is deterministic across runs
- [ ] `branch` with workspace `promote.lock` verifies correctly
- [ ] `bump -p <member>` bumps only that member's version
- [ ] Error on `bump -p <member>` when member uses
      `version.workspace = true`

## Migration

No breaking changes. Single-crate repos work identically. Workspace
repos gain support by having a `[workspace]` section — detection is
automatic.
