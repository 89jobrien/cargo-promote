# cargo-promote

Crate publishing and promotion pipeline for Rust projects.
Publishes to private registries (Gitea/cratebox) and optionally
crates.io, with configurable pipelines, per-package overrides,
deferred promotions, and forge integration.

## Install

```bash
cargo install --path .
```

## Commands

### Publishing

```bash
# Publish current crate to the first pipeline stage
cargo-promote publish

# Publish a specific workspace member
cargo-promote publish -p my-crate

# Publish to a named registry directly
cargo-promote publish --registry cratebox

# Publish even if the version already exists on the registry
cargo-promote publish --force

# Publish + promote through all pipeline stages
cargo-promote ship -p my-crate

# Publish all crates under a directory in dependency order
cargo-promote publish-all --path ~/dev --dry-run
cargo-promote publish-all --force --skip "sandbox,experiments"
```

### Promotion

```bash
# Promote from one pipeline stage to the next
cargo-promote promote -p my-crate --from cratebox

# Bump version and create promote.lock
cargo-promote bump

# Merge branch stage forward
cargo-promote branch --from develop
```

### Deferrals

Defer a promotion for later confirmation (e.g. after manual QA):

```bash
# Defer a registry promotion
cargo-promote defer --from cratebox

# Defer a branch promotion
cargo-promote defer --branch --from develop

# Confirm or reject a pending deferral
cargo-promote confirm <ticket>
cargo-promote reject <ticket> --reason "failed QA"

# List deferrals
cargo-promote deferrals
cargo-promote deferrals --pending
```

### Info

```bash
# List crates on a registry
cargo-promote list
cargo-promote list --registry crates-io

# Show local crate versions
cargo-promote status
```

## Configuration

Place a `promote.toml` in your crate or workspace root:

```toml
# Automatically bump version before publishing (patch | minor | major)
autobump = "patch"

[registries.cratebox]
cargo_name = "cratebox"
api_url = "http://100.105.75.7:3000/api/packages/joe/cargo"

[registries.crates-io]
confirm = true                  # prompt before publishing

[pipelines.default]
stages = ["cratebox", "crates-io"]
```

### Per-Package Overrides

Override autobump, pipeline, or publishing behavior per crate:

```toml
autobump = "patch"

[packages.my-internal-crate]
publish = false                 # skip during publish-all

[packages.important-lib]
autobump = "minor"              # override global autobump
pipeline = "staging-only"       # use a different pipeline
```

### Forge Integration

Connect to a Gitea or GitHub forge for PR-based deferral workflows.
When configured, `defer` creates a PR on the forge, and `confirm`
closes it automatically.

```toml
[forge]
type = "gitea"
url = "http://localhost:3000"
owner = "joe"
repo = "my-project"
token_env = "GITEA_TOKEN"       # env var holding the API token
```

### Branch Pipeline

For environment-based promotion through git branches:

```toml
autobump = "patch"

[pipeline]
stages = ["develop", "staging", "production"]
release_branch = "production"   # default: last stage

[registries.cratebox]
cargo_name = "cratebox"
```

The pipeline works as a CI chain:

1. Push to `develop` -- CI runs -- on green, `cargo-promote bump`
   writes version and `promote.lock`, then
   `cargo-promote branch --from develop` merges to `staging`
2. CI on `staging` -- on green,
   `cargo-promote branch --from staging` merges to `production`
3. `cargo-promote publish` on `production` tags the release -- a
   tag-triggered CI workflow handles registry publishing

`promote.lock` tracks the version and a SHA-256 hash of publishable
source files (`src/`, `Cargo.toml`, `Cargo.lock`). The hash is
verified at every branch hop to ensure nothing mutated mid-pipeline.

### Registry Auto-Discovery

cargo-promote automatically discovers registries from
`.cargo/config.toml` files by walking ancestor directories and
checking `$CARGO_HOME`. Entries in `promote.toml` take precedence
over discovered registries.

## Cargo Registry Setup

Add cratebox to `~/.cargo/config.toml`:

```toml
[registries.cratebox]
index = "sparse+http://<your-host>/api/packages/<user>/cargo/"

[registry]
default = "cratebox"
```

Add your token to `~/.cargo/credentials.toml`:

```toml
[registries.cratebox]
token = "Bearer <your-token>"
```

## Default Behavior

Without a `promote.toml`, cargo-promote uses built-in defaults:

- Registry: cratebox at
  `http://100.105.75.7:3000/api/packages/joe/cargo` (override with
  `REGISTRY_URL` and `REGISTRY_USER` env vars)
- Pipeline: cratebox -> crates-io (crates-io requires confirmation)

## Architecture

Hexagonal architecture with domain ports (traits) and infrastructure
adapters:

- **Domain**: `CrateRef`, `Pipeline`, `Stage`, `PublishOpts`,
  `Deferral`, `PromoteLock`
- **Ports**: `Publisher`, `RegistryQuery`, `PipelineRunner`,
  `BranchMerger`, `RemotePusher`, `Tagger`, `TokenResolver`,
  `Notifier`, `Forge`
- **Adapters**: `CargoPublisher`, `GiteaRegistry`, `GiteaForge`,
  `LocalGit`, `CargoTokenResolver`
- **API**: `Api` facade with `ApiBuilder` for dependency injection

## Build

```bash
cargo build --release
cargo test
cargo clippy
```
