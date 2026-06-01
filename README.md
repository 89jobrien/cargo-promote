# cargo-promote

Crate publishing and promotion pipeline for Rust projects.
Publishes to private registries (Gitea/cratebox) and optionally
crates.io, with configurable pipelines, per-package overrides,
deferred promotions, and forge integration.

## Install

```bash
cargo install cargo-promote
# or from git
cargo install --git https://github.com/89jobrien/cargo-promote
# or from a local checkout
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

# Select a named pipeline
cargo-promote publish --pipeline staging-only

# Publish even if the version already exists on the registry
cargo-promote publish --force

# Publish + promote through all pipeline stages
cargo-promote ship -p my-crate

# Ship with auto-confirm and force
cargo-promote ship -p my-crate -y --force

# Publish all crates under a directory in dependency order
cargo-promote publish-all --path ~/dev --dry-run
cargo-promote publish-all --force --skip "sandbox,experiments"
cargo-promote publish-all --registry cratebox --allow-dirty
```

### Promotion

```bash
# Promote from one pipeline stage to the next
cargo-promote promote -p my-crate --from cratebox

# Promote with auto-confirm and dry-run
cargo-promote promote -p my-crate --from cratebox -y --dry-run

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

# Defer with a notification command
cargo-promote defer --from cratebox --command notify-send "promotion deferred"

# Confirm or reject a pending deferral
cargo-promote confirm <ticket>
cargo-promote confirm <ticket> --reason "passed QA"
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

Connect to a Gitea or GitHub forge for PR-based deferral workflows
and release creation. When configured, `defer` creates a PR on the
forge, and `confirm` comments and closes it automatically. The
`Forge` trait also supports `create_release` for tag-based releases.

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

`promote.lock` (YAML) tracks the version and a SHA-256 hash of
publishable files: `Cargo.toml`, `Cargo.lock`, and all `*.rs` files
under `src/`. The hash is verified at every branch hop to ensure
nothing mutated mid-pipeline.

### Registry Auto-Discovery

cargo-promote automatically discovers registries from
`.cargo/config.toml` files by walking ancestor directories and
checking `$CARGO_HOME`. Entries in `promote.toml` take precedence
over discovered registries.

## Architecture

Hexagonal architecture with domain ports (traits) and infrastructure
adapters:

- **Domain types**: `CrateRef`, `CrateInfo`, `Pipeline`, `Stage`,
  `PublishOpts`, `Deferral`, `DeferralKind`, `DeferralStatus`,
  `PromoteLock`, `ManifestDescription`
- **Ports (traits)**: `Publisher`, `RegistryQuery`, `PipelineRunner`,
  `BranchMerger`, `RemotePusher`, `Tagger`, `TokenResolver`,
  `Notifier`, `Forge`
- **Adapters**: `CargoPublisher`, `GiteaRegistry`, `GitHubRegistry`,
  `GiteaForge`, `LocalGit` (implements `BranchMerger`,
  `RemotePusher`, `Tagger`), `CargoTokenResolver`, `SpawnNotifier`
- **API**: `Api` facade with `ApiBuilder` for dependency injection
- **Config**: `Config` with per-package overrides and registry
  auto-discovery from `.cargo/config.toml`

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
- `publish-all` skips a default set of repos (maestro, sandbox, etc.)

## Build

```bash
cargo build --release
cargo test
cargo clippy
```
