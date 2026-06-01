# Promote Pipeline

Run the cargo-promote pipeline: bump version, merge through branch
stages, publish to registries.

## When to Use

- When the user says "promote", "ship it", "release", "publish",
  "run the pipeline", or "bump and publish"
- After work lands on the first branch stage and is ready to move
  through the pipeline

## Prerequisites

- On the first branch stage (as defined in `promote.toml`
  `[pipeline].stages`)
- Working tree should be clean (committed or stashed)
- Remote configured and reachable
- Private registry reachable (if applicable)

## Step 0: Read the pipeline config

Before running any commands, read `promote.toml` to determine:

- Branch stages (e.g. `[pipeline].stages`)
- Registry stages (e.g. `[pipelines.default].stages`)
- First and last branch stage names
- First and last registry stage names

Use these names in all subsequent commands. Do not assume specific
stage names like "develop" or "cratebox" — always read the config.

## Step 1: Status check

```bash
cargo-promote status
git log --oneline -5
git branch --show-current
```

Verify we are on the first branch stage. If not, ask the user
before switching.

## Step 2: Bump

```bash
cargo-promote bump
```

This will:
- Autobump the version (level from `promote.toml`)
- Compute SHA-256 hash of publishable files
- Write `promote.lock`
- Commit and push

If the push fails (dirty tree, remote ahead), resolve before
continuing.

## Step 3: Branch promotion

Merge through each branch stage sequentially. For a pipeline with
stages `[A, B, C]`:

```bash
cargo-promote branch --from A    # A -> B
cargo-promote branch --from B    # B -> C
```

Each hop verifies the `promote.lock` hash before merging. If
verification fails, STOP and report — source files changed
mid-pipeline.

After all merges, return to the first stage:

```bash
git checkout <first-stage>
```

## Step 4: Registry publish

Publish to the first registry in the pipeline:

```bash
cargo-promote publish --allow-dirty
```

If the version already exists on the registry, the command skips
it automatically (use `--force` to override).

## Step 5: Registry promotion

Promote from the first registry stage to the next:

```bash
cargo-promote promote --from <first-registry> -y
```

If the target registry has `confirm = true` in `promote.toml`
and the user wants manual confirmation, omit `-y`.

For pipelines with more than two registry stages, repeat:

```bash
cargo-promote promote --from <second-registry> -y
```

## Step 6: Verify

Confirm the publish landed:

```bash
cargo-promote list --registry <first-registry>
```

Report the published version and registry states.

## Partial runs

The user may request only part of the pipeline:

| Request                         | Steps to run |
| ------------------------------- | ------------ |
| "just bump"                     | Step 2 only  |
| "merge to next stage"           | Step 3, one hop only |
| "publish to registry"           | Step 4 only  |
| "promote to next registry"      | Step 5 only  |
| "bump and publish"              | Steps 2 + 4  |
| "full pipeline" / "ship it"     | All steps    |

## Deferral flow

If the user wants a gated promotion instead of immediate:

```bash
# Defer instead of immediate promote
cargo-promote defer --from <registry>

# Defer a branch promotion
cargo-promote defer --branch --from <branch-stage>

# Later, confirm or reject
cargo-promote confirm <ticket> --reason "passed QA"
cargo-promote reject <ticket> --reason "failed smoke test"

# Check pending deferrals
cargo-promote deferrals --pending
```

## Troubleshooting

| Problem | Fix |
| ------- | --- |
| `promote.lock hash mismatch` | Source files changed after bump. Re-run `cargo-promote bump` on the first branch stage. |
| `origin does not appear to be a git repository` | Add the correct remote: `git remote add origin <url>` |
| `already exists on registry` | Expected if re-running. Use `--force` to override. |
| `cargo publish` dirty tree error | Pass `--allow-dirty` or commit changes first. |
| `unknown registry` | Check `promote.toml` and `~/.cargo/config.toml` for registry definitions. |

## Configuration reference

Pipeline config lives in `promote.toml` at the repo root. Read it
to determine all stage names, registry names, and options. Key
sections:

- `[registries.<name>]` — registry definitions with `cargo_name`,
  `api_url`, `confirm`
- `[pipelines.<name>]` — ordered list of registry stages
- `[pipeline]` — branch-based stages and `release_branch`
- `autobump` — version bump level (`patch`, `minor`, `major`)
- `[packages.<name>]` — per-crate overrides
- `[forge]` — forge integration for PR-based deferrals
