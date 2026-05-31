# cargo-promote

Crate publishing and branch promotion pipeline for Rust projects.
Publishes to private registries (Gitea/cratebox) and optionally crates.io,
with a configurable branch-based promotion pipeline for moving code through
environments.

## Install

```bash
cargo install --path .
```

## Commands

```bash
# Publish current crate to the first pipeline stage (or cratebox by default)
cargo-promote publish

# Publish a specific workspace member
cargo-promote publish -p my-crate

# Publish to a named registry directly
cargo-promote publish --registry cratebox

# Publish + promote through all pipeline stages
cargo-promote ship

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

### Branch Pipeline (coming soon)

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

1. Push to `develop` → CI runs → on green, `cargo-promote bump` writes version
   and `promote.lock`, then `cargo-promote branch --from develop` merges to `staging`
2. CI on `staging` → on green, `cargo-promote branch --from staging` merges to
   `production`
3. `cargo-promote publish` on `production` tags the release — a tag-triggered CI
   workflow handles registry publishing

`promote.lock` tracks the version and a SHA-256 hash of publishable source files
(`src/`, `Cargo.toml`, `Cargo.lock`). The hash is verified at every branch hop to
ensure nothing mutated mid-pipeline.

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

- Registry: cratebox at `http://100.105.75.7:3000/api/packages/joe/cargo`
  (override with `REGISTRY_URL` and `REGISTRY_USER` env vars)
- Pipeline: cratebox → crates-io (crates-io requires confirmation)

## Build

```bash
cargo build --release
cargo test
cargo clippy
```
