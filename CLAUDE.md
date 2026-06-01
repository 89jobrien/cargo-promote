# cargo-promote

Publish crates to the cratebox registry (Gitea packages on jobrien-vm)
and optionally promote them to crates.io.

## Architecture

- **Registry**: Gitea 1.25 cargo packages API at
  `http://100.105.75.7:3000/api/packages/joe/cargo/` (Tailscale)
- **Public endpoint**: `https://jobrien-vm.taila01bd5.ts.net/`
  (Tailscale Funnel, for CI/GitHub Actions)
- **Auth**: Tailscale network (no token for reads, Bearer token for
  publish stored in `~/.cargo/credentials.toml`)
- **Sparse index**: `sparse+http://100.105.75.7:3000/api/packages/joe/cargo/`

## Commands

```bash
cargo-promote publish              # publish cwd crate to cratebox
cargo-promote publish -p foo       # publish workspace member
cargo-promote promote -p foo       # promote from cratebox to crates.io
cargo-promote ship -p foo          # publish + promote in one step
cargo-promote list                 # list all crates on cratebox
cargo-promote status               # show local crate versions
cargo-promote bump                 # bump version + create promote.lock
cargo-promote branch --from dev    # merge branch stage forward
cargo-promote defer --from cratebox          # defer registry promotion
cargo-promote defer --branch --from develop  # defer branch promotion
cargo-promote confirm <ticket>     # confirm deferred promotion
cargo-promote reject <ticket>      # reject deferred promotion
cargo-promote deferrals            # list all deferrals
cargo-promote deferrals --pending  # list pending only
```

## Build & Test

```bash
cargo build --release
cargo clippy
```

## Mise Tasks (global)

```bash
mise run registry:health           # ping registry
mise run registry:crates           # list crates
mise run registry:search -- <q>    # search by name
mise run registry:ui               # open Gitea packages in browser
```

## Config files

- `~/.cargo/config.toml` — registry definition
- `~/.cargo/credentials.toml` — Bearer token for publish
- `~/.config/mise/config.toml` — env vars and tasks

## Future

- Source replacement (buffer/proxy mode) — commented out in config.toml
- `status` command comparing local vs registry versions
- Self-orchestrating registry pipeline (auto-publish on confirm)
