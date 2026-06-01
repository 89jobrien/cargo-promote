# Promote Pipeline

Run the cargo-promote pipeline for this repo: bump version, merge
through branch stages, publish to registries.

## When to Use

- When the user says "promote", "ship it", "release", "publish",
  "run the pipeline", or "bump and publish"
- After a batch of work lands on `develop` and is ready to move
  through staging and production

## Prerequisites

- On the `develop` branch (or switch to it)
- Working tree should be clean (committed or stashed)
- `origin` remote configured pointing to GitHub
- Tailscale up (for cratebox registry access)

## Step 1: Status check

Show current state before starting:

```bash
cargo-promote status
git log --oneline -5
git branch --show-current
```

Verify we are on `develop`. If not, ask the user before switching.

## Step 2: Bump

Bump the version and create `promote.lock`:

```bash
cargo-promote bump
```

This will:
- Autobump the patch version (configurable in `promote.toml`)
- Compute SHA-256 hash of publishable files
- Write `promote.lock`
- Commit and push to `develop`

If the push fails (dirty tree, remote ahead), resolve before
continuing.

## Step 3: Branch promotion

Merge through the branch pipeline stages sequentially:

```bash
# develop -> staging
cargo-promote branch --from develop

# staging -> production
cargo-promote branch --from staging
```

Each hop verifies the `promote.lock` hash before merging. If
verification fails, STOP and report — source files changed
mid-pipeline.

After both merges, return to `develop`:

```bash
git checkout develop
```

## Step 4: Registry publish

Publish to the first registry stage (cratebox):

```bash
cargo-promote publish --allow-dirty
```

If the version already exists on the registry, the command skips
it automatically (use `--force` to override).

## Step 5: Registry promotion

Promote from cratebox to crates.io:

```bash
cargo-promote promote --from cratebox -y
```

If the user wants confirmation before publishing to crates.io,
omit `-y`.

## Step 6: Verify

Confirm the publish landed:

```bash
cargo-promote list --registry cratebox | head -5
```

Report the published version and both registry states.

## Partial runs

The user may request only part of the pipeline:

| Request                        | Steps to run |
| ------------------------------ | ------------ |
| "just bump"                    | Step 2 only  |
| "merge to staging"             | Step 3, first command only |
| "publish to cratebox"          | Step 4 only  |
| "promote to crates.io"         | Step 5 only  |
| "bump and publish"             | Steps 2 + 4  |
| "full pipeline" / "ship it"    | All steps    |

## Deferral flow

If the user wants a gated promotion instead of immediate:

```bash
# Defer instead of immediate promote
cargo-promote defer --from cratebox

# Later, confirm or reject
cargo-promote confirm <ticket> --reason "passed QA"
cargo-promote reject <ticket> --reason "failed smoke test"

# Check pending deferrals
cargo-promote deferrals --pending
```

## Troubleshooting

| Problem | Fix |
| ------- | --- |
| `promote.lock hash mismatch` | Source files changed after bump. Re-run `cargo-promote bump` on develop. |
| `origin does not appear to be a git repository` | Add origin remote: `git remote add origin <url>` |
| `already exists on registry` | Expected if re-running. Use `--force` to override. |
| `cargo publish` dirty tree error | Pass `--allow-dirty` or commit changes first. |
| `unknown registry` | Check `promote.toml` and `~/.cargo/config.toml` for registry definitions. |

## Configuration

Pipeline config lives in `promote.toml` at the repo root:

- **Registries**: cratebox (private), crates-io (public, confirm=true)
- **Registry pipeline**: cratebox -> crates-io
- **Branch pipeline**: develop -> staging -> production
- **Autobump**: patch (configurable per-package)
- **Forge**: GitHub (PRs created for deferrals)
