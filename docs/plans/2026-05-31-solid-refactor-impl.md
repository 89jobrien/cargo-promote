# Plan: SOLID Refactor Implementation

## Goal

Apply hexagonal architecture cleanup to cargo-promote: inject all ports
via trait objects on `Api`, consolidate `LocalGit`, add domain-level
`Notifier`, and extract shell calls from domain code.

## Architecture

- Crate affected: `cargo-promote` (single crate)
- New traits: `PipelineRunner`, `Notifier` in `src/domain/traits/mod.rs`
- New adapter: `SpawnNotifier`, `NoopNotifier` in `src/infra/notify.rs`
- Consolidated: `LocalGit` in `src/infra/git/local/mod.rs`
- Rewritten: `Api` in `src/lib.rs`
- Data flow unchanged; only wiring and abstraction boundaries move

## Tech Stack

- Rust 2024 edition, existing deps only
- No new crate dependencies

## Tasks

### Task 1: Add `PipelineRunner` trait

**Crate**: `cargo-promote`
**File(s)**: `src/domain/traits/mod.rs`
**Run**: `cargo nextest run`

1. Write failing test in `src/domain/pipeline/mod.rs`:

   ```rust
   #[test]
   fn pipeline_engine_implements_pipeline_runner() {
       let pub_ = RecordingPublisher::new();
       let engine = PipelineEngine::new(&pub_, |_| true);
       let runner: &dyn crate::domain::traits::PipelineRunner = &engine;
       let opts = PublishOpts {
           skip_confirm: true,
           ..Default::default()
       };
       runner
           .run_stage(&test_crate(), &Stage { registry: reg("s", false) }, &opts)
           .expect("should succeed via trait object");
   }
   ```

   Run: `cargo nextest run -- pipeline_engine_implements_pipeline_runner`
   Expected: FAIL (trait does not exist)

2. Add `PipelineRunner` trait to `src/domain/traits/mod.rs`:

   ```rust
   use super::{CrateRef, Pipeline, PromoteError, PublishOpts, Stage};

   /// Port: drive a crate through pipeline stages.
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

3. Implement `PipelineRunner` for `PipelineEngine<P>` in
   `src/domain/pipeline/mod.rs`:

   ```rust
   use super::traits::PipelineRunner;

   impl<P: Publisher> PipelineRunner for PipelineEngine<P> {
       fn run_stage(
           &self,
           krate: &CrateRef,
           stage: &Stage,
           opts: &PublishOpts,
       ) -> Result<(), PromoteError> {
           self.run_stage(krate, stage, opts)
       }

       fn run_full(
           &self,
           krate: &CrateRef,
           pipeline: &Pipeline,
           opts: &PublishOpts,
       ) -> Result<(), PromoteError> {
           self.run_full(krate, pipeline, opts)
       }

       fn promote_next(
           &self,
           krate: &CrateRef,
           pipeline: &Pipeline,
           current_stage: &str,
           opts: &PublishOpts,
       ) -> Result<(), PromoteError> {
           self.promote_next(krate, pipeline, current_stage, opts)
       }
   }
   ```

   Note: The inherent methods and trait methods share names. Rename the
   inherent methods to avoid ambiguity:

   - `run_stage` -> keep as-is (trait delegates to inherent)

   Actually, since the trait impl just calls `self.method()`, and Rust
   resolves inherent methods over trait methods on concrete types, this
   works without renaming. The trait impl body calls the inherent
   method. Callers using `&dyn PipelineRunner` dispatch through the
   trait vtable.

4. Verify:

   ```
   cargo nextest run    -> all green
   cargo clippy -- -D warnings  -> zero warnings
   ```

5. Commit: `git commit -m "feat: add PipelineRunner trait"`

### Task 2: Add `Notifier` trait and adapters

**Crate**: `cargo-promote`
**File(s)**: `src/domain/traits/mod.rs`, `src/infra/notify.rs`,
`src/infra/mod.rs`
**Run**: `cargo nextest run`

1. Write failing test in `src/infra/notify.rs`:

   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;
       use crate::domain::deferral::{Deferral, DeferralKind, DeferralStatus};

       fn sample_deferral() -> Deferral {
           Deferral {
               ticket: "d-test".to_string(),
               crate_name: "mycrate".to_string(),
               version: "0.1.0".to_string(),
               from_stage: "staging".to_string(),
               to_stage: "production".to_string(),
               status: DeferralStatus::Pending,
               kind: DeferralKind::Registry,
               deferred_at: "20260531.120000".to_string(),
               source_hash: "sha256:abc".to_string(),
               command: vec![],
               reason: String::new(),
           }
       }

       #[test]
       fn noop_notifier_returns_ok() {
           let n = NoopNotifier;
           let result = n.on_deferred(&sample_deferral());
           assert!(result.is_ok());
       }

       #[test]
       fn spawn_notifier_with_no_command_returns_ok() {
           let n = SpawnNotifier {
               command: vec![],
           };
           let result = n.on_deferred(&sample_deferral());
           assert!(result.is_ok());
       }
   }
   ```

   Run: `cargo nextest run -- noop_notifier_returns_ok`
   Expected: FAIL (module does not exist)

2. Add `Notifier` trait to `src/domain/traits/mod.rs`:

   ```rust
   use super::deferral::Deferral;

   /// Port: notify external systems about promotion events.
   pub trait Notifier {
       fn on_deferred(&self, deferral: &Deferral) -> Result<(), PromoteError>;
   }
   ```

3. Create `src/infra/notify.rs`:

   ```rust
   use crate::domain::deferral::Deferral;
   use crate::domain::traits::Notifier;
   use crate::domain::PromoteError;

   /// Adapter: fire a shell command on deferral (best-effort).
   pub struct SpawnNotifier {
       pub command: Vec<String>,
   }

   impl Notifier for SpawnNotifier {
       fn on_deferred(&self, _deferral: &Deferral) -> Result<(), PromoteError> {
           if self.command.is_empty() {
               return Ok(());
           }
           match std::process::Command::new(&self.command[0])
               .args(&self.command[1..])
               .spawn()
           {
               Ok(_child) => {
                   eprintln!("=> notification command spawned");
               }
               Err(e) => {
                   eprintln!(
                       "=> notification command failed to start: {e}"
                   );
               }
           }
           Ok(())
       }
   }

   /// Adapter: no-op notifier for tests and library usage.
   pub struct NoopNotifier;

   impl Notifier for NoopNotifier {
       fn on_deferred(&self, _deferral: &Deferral) -> Result<(), PromoteError> {
           Ok(())
       }
   }
   ```

4. Update `src/infra/mod.rs`:

   ```rust
   pub mod cargo;
   pub mod git;
   pub mod notify;
   ```

5. Verify:

   ```
   cargo nextest run    -> all green
   cargo clippy -- -D warnings  -> zero warnings
   ```

6. Commit: `git commit -m "feat: add Notifier trait with SpawnNotifier and NoopNotifier"`

### Task 3: Consolidate `LocalGit`

**Crate**: `cargo-promote`
**File(s)**: `src/infra/git/local/mod.rs`
**Run**: `cargo nextest run`

1. Write failing test in `src/infra/git/local/mod.rs`:

   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;
       use crate::domain::traits::{BranchMerger, RemotePusher, Tagger};
       use std::path::PathBuf;

       #[test]
       fn local_git_has_repo_root_field() {
           let git = LocalGit::new(PathBuf::from("/tmp/test"));
           assert_eq!(git.repo_root, PathBuf::from("/tmp/test"));
       }
   }
   ```

   Run: `cargo nextest run -- local_git_has_repo_root_field`
   Expected: FAIL (`LocalGit::new` does not exist)

2. Rewrite `src/infra/git/local/mod.rs`:

   ```rust
   use crate::domain::PromoteError;
   use crate::domain::traits::{BranchMerger, RemotePusher, Tagger};
   use std::path::{Path, PathBuf};
   use std::process::Command;

   /// Adapter: local git operations via the git CLI.
   pub struct LocalGit {
       pub repo_root: PathBuf,
   }

   impl LocalGit {
       pub fn new(repo_root: PathBuf) -> Self {
           Self { repo_root }
       }

       fn path(&self) -> &Path {
           &self.repo_root
       }

       /// Check if the working tree is dirty.
       pub fn is_dirty(&self) -> Result<bool, PromoteError> {
           let output = Command::new("git")
               .args(["status", "--porcelain"])
               .current_dir(self.path())
               .output()
               .map_err(|e| PromoteError::Other(e.into()))?;
           Ok(!output.stdout.is_empty())
       }

       /// Get the current branch name.
       pub fn current_branch(&self) -> Result<String, PromoteError> {
           let output = Command::new("git")
               .args(["branch", "--show-current"])
               .current_dir(self.path())
               .output()
               .map_err(|e| PromoteError::Other(e.into()))?;
           Ok(String::from_utf8_lossy(&output.stdout)
               .trim()
               .to_string())
       }

       /// Stage files for commit.
       pub fn stage(&self, files: &[&str]) -> Result<(), PromoteError> {
           let mut cmd = Command::new("git");
           cmd.arg("add").current_dir(self.path());
           for f in files {
               cmd.arg(f);
           }
           let status = cmd
               .status()
               .map_err(|e| PromoteError::Other(e.into()))?;
           if !status.success() {
               return Err(PromoteError::Other(anyhow::anyhow!(
                   "git add failed for: {}",
                   files.join(", ")
               )));
           }
           Ok(())
       }

       /// Create a commit with the given message.
       pub fn commit(&self, message: &str) -> Result<(), PromoteError> {
           let status = Command::new("git")
               .args(["commit", "-m", message])
               .current_dir(self.path())
               .status()
               .map_err(|e| PromoteError::Other(e.into()))?;
           if !status.success() {
               return Err(PromoteError::Other(anyhow::anyhow!(
                   "git commit failed"
               )));
           }
           Ok(())
       }

       /// Push the current HEAD to origin.
       pub fn push_head(&self) -> Result<(), PromoteError> {
           let status = Command::new("git")
               .args(["push", "origin", "HEAD"])
               .current_dir(self.path())
               .status()
               .map_err(|e| PromoteError::Other(e.into()))?;
           if !status.success() {
               return Err(PromoteError::Other(anyhow::anyhow!(
                   "git push HEAD failed"
               )));
           }
           Ok(())
       }
   }

   impl BranchMerger for LocalGit {
       fn fast_forward(
           &self,
           source: &str,
           target: &str,
       ) -> Result<(), PromoteError> {
           let _fetch = Command::new("git")
               .args(["fetch", "origin"])
               .current_dir(self.path())
               .output();

           let checkout = Command::new("git")
               .args(["checkout", target])
               .current_dir(self.path())
               .status()
               .map_err(|e| PromoteError::Other(e.into()))?;
           if !checkout.success() {
               return Err(PromoteError::Other(anyhow::anyhow!(
                   "failed to checkout branch '{target}'"
               )));
           }

           let status = Command::new("git")
               .args(["merge", "--ff-only", source])
               .current_dir(self.path())
               .status()
               .map_err(|e| PromoteError::Other(e.into()))?;
           if !status.success() {
               return Err(PromoteError::Other(anyhow::anyhow!(
                   "fast-forward merge from '{source}' to '{target}' failed"
               )));
           }
           Ok(())
       }
   }

   impl RemotePusher for LocalGit {
       fn push_branch(
           &self,
           branch: &str,
       ) -> Result<(), PromoteError> {
           let status = Command::new("git")
               .args(["push", "origin", branch])
               .current_dir(self.path())
               .status()
               .map_err(|e| PromoteError::Other(e.into()))?;
           if !status.success() {
               return Err(PromoteError::Other(anyhow::anyhow!(
                   "failed to push branch '{branch}'"
               )));
           }
           Ok(())
       }

       fn push_tag(
           &self,
           tag: &str,
       ) -> Result<(), PromoteError> {
           let status = Command::new("git")
               .args(["push", "origin", tag])
               .current_dir(self.path())
               .status()
               .map_err(|e| PromoteError::Other(e.into()))?;
           if !status.success() {
               return Err(PromoteError::Other(anyhow::anyhow!(
                   "failed to push tag '{tag}'"
               )));
           }
           Ok(())
       }
   }

   impl Tagger for LocalGit {
       fn create_tag(
           &self,
           name: &str,
           message: &str,
       ) -> Result<(), PromoteError> {
           let status = Command::new("git")
               .args(["tag", "-a", name, "-m", message])
               .current_dir(self.path())
               .status()
               .map_err(|e| PromoteError::Other(e.into()))?;
           if !status.success() {
               return Err(PromoteError::Other(anyhow::anyhow!(
                   "git tag '{name}' failed"
               )));
           }
           Ok(())
       }
   }

   #[cfg(test)]
   mod tests {
       use super::*;

       #[test]
       fn local_git_has_repo_root_field() {
           let git = LocalGit::new(PathBuf::from("/tmp/test"));
           assert_eq!(git.repo_root, PathBuf::from("/tmp/test"));
       }
   }
   ```

3. Verify:

   ```
   cargo nextest run    -> all green
   cargo clippy -- -D warnings  -> zero warnings
   ```

4. Commit: `git commit -m "refactor: consolidate LocalGit with repo_root, delete wrapper structs"`

### Task 4: Extract shell calls from `BranchPipeline::bump`

**Crate**: `cargo-promote`
**File(s)**: `src/domain/pipeline/mod.rs`
**Run**: `cargo nextest run`

1. Update `BranchPipeline::bump` signature to accept `&LocalGit`:

   ```rust
   use crate::infra::git::local::LocalGit;

   impl BranchPipeline {
       pub fn bump(
           krate: &CrateRef,
           stages: &[String],
           repo_path: &std::path::Path,
           git: &LocalGit,
       ) -> Result<(), PromoteError> {
           use crate::domain::promote_lock::PromoteLock;

           let (old_version, new_version) =
               crate::domain::version::bump_manifest_version(
                   &krate.manifest_path,
                   crate::domain::version::BumpLevel::Patch,
               )
               .map_err(PromoteError::Other)?;

           eprintln!(
               "=> bumped {} v{old_version} -> v{new_version}",
               krate.name
           );

           let source_hash = PromoteLock::compute_source_hash(repo_path)
               .map_err(PromoteError::Other)?;

           let entered_pipeline =
               stages.first().cloned().unwrap_or_default();
           let lock = PromoteLock {
               version: new_version.to_string(),
               source_hash,
               bumped_at: chrono::Local::now()
                   .format("%Y%m%d::%H%M%S")
                   .to_string(),
               entered_pipeline,
           };

           lock.write(repo_path).map_err(PromoteError::Other)?;

           git.stage(&["Cargo.toml", "promote.lock"])?;
           git.commit(&format!(
               "bump: {} v{}",
               krate.name, new_version
           ))?;
           git.push_head()?;

           Ok(())
       }
   }
   ```

2. This changes the signature, so update the caller in `src/lib.rs`
   (`Api::bump`):

   ```rust
   pub fn bump(
       &self,
       path: Option<&Path>,
       package: Option<&str>,
       cwd: &Path,
   ) -> Result<()> {
       let krate = manifest::resolve_crate(path, package)?;
       let branch_cfg = self
           .config
           .branch_pipeline
           .as_ref()
           .ok_or_else(|| {
               anyhow::anyhow!(
                   "branch pipeline not configured in promote.toml"
               )
           })?;
       let repo_path = path.unwrap_or(cwd);
       let git = infra::git::local::LocalGit::new(
           repo_path.to_path_buf(),
       );
       domain::pipeline::BranchPipeline::bump(
           &krate,
           &branch_cfg.stages,
           repo_path,
           &git,
       )?;
       Ok(())
   }
   ```

3. Verify:

   ```
   cargo nextest run    -> all green
   cargo clippy -- -D warnings  -> zero warnings
   ```

4. Commit: `git commit -m "refactor: extract git shell calls from BranchPipeline::bump into LocalGit"`

### Task 5: Rewrite `Api` with trait objects

**Crate**: `cargo-promote`
**File(s)**: `src/lib.rs`
**Run**: `cargo nextest run`

1. Write failing test in `src/lib.rs`:

   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;
       use crate::domain::deferral::Deferral;
       use crate::domain::traits::{Notifier, PipelineRunner};
       use crate::domain::{
           CrateInfo, CrateRef, Pipeline, PromoteError, PublishOpts,
           Registry, Stage,
       };
       use std::cell::RefCell;

       struct FakeRunner {
           stages_run: RefCell<Vec<String>>,
       }

       impl FakeRunner {
           fn new() -> Self {
               Self {
                   stages_run: RefCell::new(vec![]),
               }
           }
       }

       impl PipelineRunner for FakeRunner {
           fn run_stage(
               &self,
               _krate: &CrateRef,
               stage: &Stage,
               _opts: &PublishOpts,
           ) -> Result<(), PromoteError> {
               self.stages_run
                   .borrow_mut()
                   .push(stage.registry.name.clone());
               Ok(())
           }

           fn run_full(
               &self,
               krate: &CrateRef,
               pipeline: &Pipeline,
               opts: &PublishOpts,
           ) -> Result<(), PromoteError> {
               for stage in &pipeline.stages {
                   self.run_stage(krate, stage, opts)?;
               }
               Ok(())
           }

           fn promote_next(
               &self,
               _krate: &CrateRef,
               _pipeline: &Pipeline,
               _current_stage: &str,
               _opts: &PublishOpts,
           ) -> Result<(), PromoteError> {
               Ok(())
           }
       }

       struct FakeQuery;

       impl domain::traits::RegistryQuery for FakeQuery {
           fn list_crates(
               &self,
               registry: &Registry,
           ) -> Result<Vec<CrateInfo>, PromoteError> {
               Ok(vec![CrateInfo {
                   name: "fake".to_string(),
                   max_version: "0.1.0".to_string(),
               }])
           }
       }

       struct RecordingNotifier {
           calls: RefCell<Vec<String>>,
       }

       impl RecordingNotifier {
           fn new() -> Self {
               Self {
                   calls: RefCell::new(vec![]),
               }
           }
       }

       impl Notifier for RecordingNotifier {
           fn on_deferred(
               &self,
               deferral: &Deferral,
           ) -> Result<(), PromoteError> {
               self.calls
                   .borrow_mut()
                   .push(deferral.ticket.clone());
               Ok(())
           }
       }

       #[test]
       fn api_with_injected_deps_can_list() {
           let api = Api::builder()
               .config(Config::default_config())
               .engine(Box::new(FakeRunner::new()))
               .registry_query(Box::new(FakeQuery))
               .notifier(Box::new(RecordingNotifier::new()))
               .build()
               .expect("should build");
           let crates = api.list(Some("cratebox")).unwrap();
           assert_eq!(crates.len(), 1);
           assert_eq!(crates[0].name, "fake");
       }
   }
   ```

   Run: `cargo nextest run -- api_with_injected_deps_can_list`
   Expected: FAIL (`Api::builder` does not exist)

2. Rewrite `Api` struct and add builder in `src/lib.rs`:

   ```rust
   use domain::traits::{Notifier, PipelineRunner, RegistryQuery};

   /// Library API for driving promotion pipelines programmatically.
   pub struct Api {
       config: Config,
       engine: Box<dyn PipelineRunner>,
       registry_query: Box<dyn RegistryQuery>,
       notifier: Box<dyn Notifier>,
   }

   /// Builder for `Api` with injectable dependencies.
   pub struct ApiBuilder {
       config: Option<Config>,
       engine: Option<Box<dyn PipelineRunner>>,
       registry_query: Option<Box<dyn RegistryQuery>>,
       notifier: Option<Box<dyn Notifier>>,
   }

   impl ApiBuilder {
       pub fn config(mut self, config: Config) -> Self {
           self.config = Some(config);
           self
       }

       pub fn engine(
           mut self,
           engine: Box<dyn PipelineRunner>,
       ) -> Self {
           self.engine = Some(engine);
           self
       }

       pub fn registry_query(
           mut self,
           query: Box<dyn RegistryQuery>,
       ) -> Self {
           self.registry_query = Some(query);
           self
       }

       pub fn notifier(
           mut self,
           notifier: Box<dyn Notifier>,
       ) -> Self {
           self.notifier = Some(notifier);
           self
       }

       pub fn build(self) -> Result<Api> {
           Ok(Api {
               config: self
                   .config
                   .ok_or_else(|| anyhow::anyhow!("config required"))?,
               engine: self
                   .engine
                   .ok_or_else(|| anyhow::anyhow!("engine required"))?,
               registry_query: self.registry_query.ok_or_else(|| {
                   anyhow::anyhow!("registry_query required")
               })?,
               notifier: self
                   .notifier
                   .ok_or_else(|| anyhow::anyhow!("notifier required"))?,
           })
       }
   }

   impl Api {
       /// Build with default adapters (CargoPublisher, GiteaRegistry,
       /// NoopNotifier) and auto-accepting confirmer.
       pub fn new(dir: &Path) -> Result<Self> {
           Self::with_confirmer(dir, |_| true)
       }

       /// Build with default adapters and a custom confirmer.
       pub fn with_confirmer(
           dir: &Path,
           confirmer: impl Fn(&str) -> bool + 'static,
       ) -> Result<Self> {
           let config = Config::load(dir)?;
           let engine = PipelineEngine::new(
               CargoPublisher,
               confirmer,
           );
           Ok(Self {
               config,
               engine: Box::new(engine),
               registry_query: Box::new(GiteaRegistry),
               notifier: Box::new(
                   infra::notify::NoopNotifier,
               ),
           })
       }

       /// Build with default adapters, custom confirmer, and a
       /// notification command.
       pub fn with_notifier(
           dir: &Path,
           confirmer: impl Fn(&str) -> bool + 'static,
           command: Vec<String>,
       ) -> Result<Self> {
           let config = Config::load(dir)?;
           let engine = PipelineEngine::new(
               CargoPublisher,
               confirmer,
           );
           Ok(Self {
               config,
               engine: Box::new(engine),
               registry_query: Box::new(GiteaRegistry),
               notifier: Box::new(
                   infra::notify::SpawnNotifier { command },
               ),
           })
       }

       /// Return a builder for full dependency injection.
       pub fn builder() -> ApiBuilder {
           ApiBuilder {
               config: None,
               engine: None,
               registry_query: None,
               notifier: None,
           }
       }

       pub fn config(&self) -> &Config {
           &self.config
       }
   }
   ```

3. Update `Api::list` to use injected `registry_query`:

   ```rust
   pub fn list(
       &self,
       registry: Option<&str>,
   ) -> Result<Vec<CrateInfo>> {
       let reg_name = registry.unwrap_or("cratebox");
       let reg = self
           .config
           .registry(reg_name)
           .ok_or_else(|| {
               anyhow::anyhow!("unknown registry '{reg_name}'")
           })?;
       let crates = self.registry_query.list_crates(reg)?;
       Ok(crates)
   }
   ```

4. Update `Api::publish`, `Api::promote`, `Api::ship` to use
   `self.engine` as `&dyn PipelineRunner` (replace
   `self.engine.run_stage(...)` — this works as-is since the method
   names match).

5. Update `Api::defer_to` and `Api::defer_branch`:
   - Remove `command: Vec<String>` parameter
   - After `deferral.write(repo_root)?;`, call
     `self.notifier.on_deferred(&deferral)?;`

6. Remove the `fire_notification` free function.

7. Verify:

   ```
   cargo nextest run    -> all green
   cargo clippy -- -D warnings  -> zero warnings
   ```

8. Commit: `git commit -m "refactor: rewrite Api with trait objects, inject all ports"`

### Task 6: Update `main.rs` wiring

**Crate**: `cargo-promote`
**File(s)**: `src/main.rs`
**Run**: `cargo nextest run`

1. Update `Cmd::Defer` to no longer pass `command` to `api.defer_to` /
   `api.defer_branch`. Instead, pass `command` when constructing `Api`:

   ```rust
   Cmd::Defer {
       package,
       path,
       from,
       pipeline,
       branch,
       command,
   } => {
       let dir = path.as_deref().unwrap_or(&cwd);
       let api = Api::with_notifier(
           dir,
           interactive_confirmer,
           command,
       )?;
       let repo_root = path.as_deref().unwrap_or(&cwd);
       let deferral = if branch {
           api.defer_branch(
               path.as_deref(),
               package.as_deref(),
               &from,
               repo_root,
           )?
       } else {
           api.defer_to(
               path.as_deref(),
               package.as_deref(),
               &from,
               pipeline.as_deref(),
               repo_root,
           )?
       };
       eprintln!(
           "=> deferred {} v{} from '{}' to '{}' [ticket: {}]",
           deferral.crate_name,
           deferral.version,
           deferral.from_stage,
           deferral.to_stage,
           deferral.ticket,
       );
       Ok(())
   }
   ```

2. Update `Api::branch` and `Api::confirm_deferral` to use
   `LocalGit::new(repo_root.to_path_buf())` instead of constructing
   `GitCliMerger` + `GitCliPusher`:

   ```rust
   pub fn branch(
       &self,
       path: Option<&Path>,
       from: &str,
       cwd: &Path,
   ) -> Result<()> {
       let branch_cfg = self
           .config
           .branch_pipeline
           .as_ref()
           .ok_or_else(|| {
               anyhow::anyhow!(
                   "branch pipeline not configured in promote.toml"
               )
           })?;
       let repo_root = path.unwrap_or(cwd);
       let git = infra::git::local::LocalGit::new(
           repo_root.to_path_buf(),
       );
       domain::pipeline::BranchPipeline::branch(
           &branch_cfg.stages,
           from,
           &git,
           &git,
           repo_root,
       )?;
       Ok(())
   }
   ```

3. Same pattern for `confirm_deferral` — replace `GitCliMerger` and
   `GitCliPusher` construction with `LocalGit::new(...)`.

4. Verify:

   ```
   cargo nextest run    -> all green
   cargo clippy -- -D warnings  -> zero warnings
   ```

5. Commit: `git commit -m "refactor: update main.rs wiring for trait-object Api"`

### Task 7: Add conformance tests for new traits

**Crate**: `cargo-promote`
**File(s)**: `tests/conformance/pipeline_runner.rs`,
`tests/conformance/notifier.rs`, `tests/conformance/mod.rs`
**Run**: `cargo nextest run`

1. Create `tests/conformance/pipeline_runner.rs`:

   ```rust
   //! Conformance tests for the PipelineRunner port.

   use cargo_promote::domain::traits::PipelineRunner;
   use cargo_promote::domain::{
       CrateRef, Pipeline, PromoteError, PublishOpts, Registry, Stage,
   };
   use std::cell::RefCell;
   use std::path::PathBuf;

   struct InMemoryRunner {
       stages_run: RefCell<Vec<String>>,
   }

   impl InMemoryRunner {
       fn new() -> Self {
           Self {
               stages_run: RefCell::new(vec![]),
           }
       }

       fn stages(&self) -> Vec<String> {
           self.stages_run.borrow().clone()
       }
   }

   impl PipelineRunner for InMemoryRunner {
       fn run_stage(
           &self,
           _krate: &CrateRef,
           stage: &Stage,
           _opts: &PublishOpts,
       ) -> Result<(), PromoteError> {
           self.stages_run
               .borrow_mut()
               .push(stage.registry.name.clone());
           Ok(())
       }

       fn run_full(
           &self,
           krate: &CrateRef,
           pipeline: &Pipeline,
           opts: &PublishOpts,
       ) -> Result<(), PromoteError> {
           for stage in &pipeline.stages {
               self.run_stage(krate, stage, opts)?;
           }
           Ok(())
       }

       fn promote_next(
           &self,
           krate: &CrateRef,
           pipeline: &Pipeline,
           current_stage: &str,
           opts: &PublishOpts,
       ) -> Result<(), PromoteError> {
           let idx = pipeline
               .stages
               .iter()
               .position(|s| s.registry.name == current_stage)
               .ok_or_else(|| PromoteError::StageNotFound {
                   pipeline: pipeline.name.clone(),
                   stage: current_stage.to_string(),
               })?;
           let next =
               pipeline.stages.get(idx + 1).ok_or_else(|| {
                   PromoteError::NoNextStage {
                       pipeline: pipeline.name.clone(),
                       stage: current_stage.to_string(),
                   }
               })?;
           self.run_stage(krate, next, opts)
       }
   }

   fn test_crate() -> CrateRef {
       CrateRef {
           name: "test-crate".to_string(),
           version: "0.1.0".to_string(),
           manifest_path: PathBuf::from("Cargo.toml"),
       }
   }

   fn reg(name: &str) -> Registry {
       Registry {
           name: name.to_string(),
           cargo_name: Some(name.to_string()),
           api_url: None,
           confirm: false,
       }
   }

   fn two_stage_pipeline() -> Pipeline {
       Pipeline {
           name: "default".to_string(),
           stages: vec![
               Stage { registry: reg("a") },
               Stage { registry: reg("b") },
           ],
       }
   }

   /// Any PipelineRunner impl must satisfy these invariants.
   fn assert_runner_contract(runner: &dyn PipelineRunner) {
       let krate = test_crate();
       let pl = two_stage_pipeline();
       let opts = PublishOpts::default();

       let result = runner.run_stage(
           &krate,
           &pl.stages[0],
           &opts,
       );
       assert!(
           result.is_ok()
               || matches!(
                   result,
                   Err(PromoteError::PublishFailed { .. })
               ),
           "run_stage must return Ok or PublishFailed"
       );
   }

   #[test]
   fn conformance_runner_contract() {
       assert_runner_contract(&InMemoryRunner::new());
   }

   #[test]
   fn conformance_run_full_visits_all_stages() {
       let runner = InMemoryRunner::new();
       let opts = PublishOpts::default();
       runner
           .run_full(&test_crate(), &two_stage_pipeline(), &opts)
           .expect("run_full should succeed");
       assert_eq!(runner.stages(), vec!["a", "b"]);
   }

   #[test]
   fn conformance_promote_next_errors_on_unknown_stage() {
       let runner = InMemoryRunner::new();
       let result = runner.promote_next(
           &test_crate(),
           &two_stage_pipeline(),
           "nonexistent",
           &PublishOpts::default(),
       );
       assert!(matches!(
           result,
           Err(PromoteError::StageNotFound { .. })
       ));
   }

   #[test]
   fn conformance_promote_next_errors_on_last_stage() {
       let runner = InMemoryRunner::new();
       let result = runner.promote_next(
           &test_crate(),
           &two_stage_pipeline(),
           "b",
           &PublishOpts::default(),
       );
       assert!(matches!(
           result,
           Err(PromoteError::NoNextStage { .. })
       ));
   }
   ```

2. Create `tests/conformance/notifier.rs`:

   ```rust
   //! Conformance tests for the Notifier port.

   use cargo_promote::domain::deferral::{
       Deferral, DeferralKind, DeferralStatus,
   };
   use cargo_promote::domain::traits::Notifier;
   use cargo_promote::infra::notify::{NoopNotifier, SpawnNotifier};

   fn sample_deferral() -> Deferral {
       Deferral {
           ticket: "d-test".to_string(),
           crate_name: "mycrate".to_string(),
           version: "0.1.0".to_string(),
           from_stage: "staging".to_string(),
           to_stage: "production".to_string(),
           status: DeferralStatus::Pending,
           kind: DeferralKind::Registry,
           deferred_at: "20260531.120000".to_string(),
           source_hash: "sha256:abc".to_string(),
           command: vec![],
           reason: String::new(),
       }
   }

   /// Any Notifier impl must accept a valid deferral without panic.
   fn assert_notifier_contract(notifier: &dyn Notifier) {
       let d = sample_deferral();
       let result = notifier.on_deferred(&d);
       assert!(
           result.is_ok()
               || result.is_err(),
           "on_deferred must return a Result, not panic"
       );
   }

   #[test]
   fn conformance_noop_notifier() {
       assert_notifier_contract(&NoopNotifier);
   }

   #[test]
   fn conformance_spawn_notifier_empty_command() {
       assert_notifier_contract(&SpawnNotifier {
           command: vec![],
       });
   }

   #[test]
   fn conformance_noop_always_ok() {
       let result = NoopNotifier.on_deferred(&sample_deferral());
       assert!(result.is_ok());
   }

   #[test]
   fn conformance_spawn_empty_command_ok() {
       let n = SpawnNotifier { command: vec![] };
       let result = n.on_deferred(&sample_deferral());
       assert!(result.is_ok());
   }
   ```

3. Update `tests/conformance/mod.rs`:

   ```rust
   mod config;
   mod notifier;
   mod pipeline_engine;
   mod pipeline_runner;
   mod publisher;
   mod registry_query;
   ```

4. Verify:

   ```
   cargo nextest run    -> all green
   cargo clippy -- -D warnings  -> zero warnings
   ```

5. Commit: `git commit -m "test: add conformance tests for PipelineRunner and Notifier"`
