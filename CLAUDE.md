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
cargo-promote publish-all --dry-run # publish all crates in dep order
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
cargo test
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

<!-- godmode-workflow:begin -->

# Phased workflow

Unless the user clearly opts out (e.g. **"skip plan, just fix it"**), every
non-trivial task progresses through five phases. Short confirmations like
**"do it"**, **"act"**, **"go"** advance to the next phase.

## Phases

<godmode-phase name="ORIENT" mode="read-only" response-header="# Phase: ORIENT" skills="godmode handon">
Default phase. Read files, search code, run `godmode handon`, check task
graph. Summarize current state: branch, dirty files, relevant context.
No modifications to the repository. End by stating what you found and
what phase comes next.
</godmode-phase>

<godmode-phase name="PLAN" mode="read-only" response-header="# Phase: PLAN" skills="brainstorm, writing-plans">
Produce a written plan: files to touch, approach, risks. Still read-only
— no edits, no builds that write output. For complex work, invoke
`godmode:brainstorm` or `godmode:writing-plans`. End with "Type ACT to
proceed" (or suggest refinements).
</godmode-phase>

<godmode-phase name="ACT" mode="read-write" response-header="# Phase: ACT" skills="task-driven-development, parallel-agents">
Enter when the user approves: "act", "go ahead", "do it". Edit files,
run commands, dispatch subagents. For multi-task work, use
`godmode:task-management` and `godmode:parallel-agents` when tasks are
independent. After finishing, transition to VERIFY automatically.
</godmode-phase>

<godmode-phase name="VERIFY" mode="read + test" response-header="# Phase: VERIFY" skills="verification-before-completion">
Run `cargo check`, `cargo clippy`, `cargo test` (or `godmode verify`).
Invoke `godmode:verification-before-completion` for non-trivial changes.
Report results. If failures exist, return to ACT to fix them. When
green, state readiness and ask to SHIP.
</godmode-phase>

<godmode-phase name="SHIP" mode="commit/push" response-header="# Phase: SHIP" skills="cap, handoff">
Commit, push, update handoff: `godmode:cap`, then `godmode handoff`.
Only entered with explicit user approval. After shipping, return to
ORIENT for the next task.
</godmode-phase>

## Phase transitions

- **User can skip phases**: "skip plan, implement now" jumps to ACT.
  "just fix it" implies ORIENT → ACT → VERIFY → SHIP in one pass.
- **After each ACT turn**, default back to VERIFY unless the user says
  otherwise.
- **Multiple ACT turns** are fine — the user can keep approving.
- When the user gives a lettered choice or short confirmation, advance
  to the most obvious next phase without asking.

## Skill invocation rule

Before responding in any phase, check if a godmode skill applies.
1% chance it’s relevant = invoke it. Process skills (`brainstorm`,
`systematic-debugging`) before implementation skills
(`task-driven-development`, `parallel-agents`).

## Task graph

Tasks live in `.ctx/GODMODE.tasks.yaml`. Use `godmode task` CLI for
state transitions. Independent chains can run in parallel via
`godmode:parallel-agents`. A task is runnable when all `depends_on`
items are `done`.

## Memory bank

Persistent context lives in `.ctx/memory-bank/`. Read before
substantive work; update `activeContext` and `progress` after
milestones. See `AGENTS.md` for the full file list.

## Agent-specific guidance

For subagent conventions, Codex integration, and memory-bank file
inventory, see `AGENTS.md`.

<!-- godmode-workflow:end -->
