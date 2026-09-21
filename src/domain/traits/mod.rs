//! Ports that isolate promotion workflows from external systems.

use super::deferral::Deferral;
use super::{CrateInfo, CrateRef, Pipeline, PromoteError, PublishOpts, Registry, Stage};

/// Port: publish a crate to a registry.
pub trait Publisher {
    /// Publishes a crate to a registry with the requested options.
    fn publish(
        &self,
        krate: &CrateRef,
        registry: &Registry,
        opts: &PublishOpts,
    ) -> Result<(), PromoteError>;
}

impl<T: Publisher> Publisher for &T {
    fn publish(
        &self,
        krate: &CrateRef,
        registry: &Registry,
        opts: &PublishOpts,
    ) -> Result<(), PromoteError> {
        (**self).publish(krate, registry, opts)
    }
}

/// Port: query a registry for crate information.
pub trait RegistryQuery {
    /// Lists crates and their latest versions from a registry.
    fn list_crates(&self, registry: &Registry) -> Result<Vec<CrateInfo>, PromoteError>;

    /// Check whether `name@version` already exists in the registry.
    fn crate_exists(
        &self,
        registry: &Registry,
        name: &str,
        version: &str,
    ) -> Result<bool, PromoteError> {
        let _ = (registry, name, version);
        Ok(false)
    }
}

/// Port: perform fast-forward merges between branches.
pub trait BranchMerger {
    /// Fast-forwards the target branch to the source branch.
    fn fast_forward(&self, source: &str, target: &str) -> Result<(), PromoteError>;
}

impl<T: BranchMerger> BranchMerger for &T {
    fn fast_forward(&self, source: &str, target: &str) -> Result<(), PromoteError> {
        (**self).fast_forward(source, target)
    }
}

/// Port: push branches and tags to a remote.
pub trait RemotePusher {
    /// Pushes a branch to the configured remote.
    fn push_branch(&self, branch: &str) -> Result<(), PromoteError>;
    /// Pushes a tag to the configured remote.
    fn push_tag(&self, tag: &str) -> Result<(), PromoteError>;
}

impl<T: RemotePusher> RemotePusher for &T {
    fn push_branch(&self, branch: &str) -> Result<(), PromoteError> {
        (**self).push_branch(branch)
    }
    fn push_tag(&self, tag: &str) -> Result<(), PromoteError> {
        (**self).push_tag(tag)
    }
}

/// Port: create and manage git tags.
pub trait Tagger {
    /// Creates a tag with the given name and message.
    fn create_tag(&self, name: &str, message: &str) -> Result<(), PromoteError>;
}

impl<T: Tagger> Tagger for &T {
    fn create_tag(&self, name: &str, message: &str) -> Result<(), PromoteError> {
        (**self).create_tag(name, message)
    }
}

/// Port: stage, commit, and push changes to a local repository.
pub trait GitCommitter {
    /// Stages the given repository-relative paths.
    fn stage(&self, files: &[&str]) -> Result<(), PromoteError>;
    /// Commits staged changes with the given message.
    fn commit(&self, message: &str) -> Result<(), PromoteError>;
    /// Pushes the current `HEAD` to the configured remote.
    fn push_head(&self) -> Result<(), PromoteError>;
}

impl<T: GitCommitter> GitCommitter for &T {
    fn stage(&self, files: &[&str]) -> Result<(), PromoteError> {
        (**self).stage(files)
    }
    fn commit(&self, message: &str) -> Result<(), PromoteError> {
        (**self).commit(message)
    }
    fn push_head(&self) -> Result<(), PromoteError> {
        (**self).push_head()
    }
}

/// Composite port: all local git operations needed by the pipeline.
pub trait GitOps: BranchMerger + RemotePusher + Tagger + GitCommitter {}

impl<T: BranchMerger + RemotePusher + Tagger + GitCommitter> GitOps for T {}

/// Port: drive a crate through pipeline stages.
pub trait PipelineRunner {
    /// Publishes a crate through one pipeline stage.
    fn run_stage(
        &self,
        krate: &CrateRef,
        stage: &Stage,
        opts: &PublishOpts,
    ) -> Result<(), PromoteError>;

    /// Runs every pipeline stage in order.
    fn run_full(
        &self,
        krate: &CrateRef,
        pipeline: &Pipeline,
        opts: &PublishOpts,
    ) -> Result<(), PromoteError>;

    /// Publishes a crate to the stage after `current_stage`.
    fn promote_next(
        &self,
        krate: &CrateRef,
        pipeline: &Pipeline,
        current_stage: &str,
        opts: &PublishOpts,
    ) -> Result<(), PromoteError>;
}

/// Port: resolve authentication tokens for registries.
pub trait TokenResolver {
    /// Resolves an optional authentication token for a registry.
    fn resolve(&self, registry_name: &str) -> Result<Option<secrecy::SecretString>, PromoteError>;
}

/// Port: persist and query deferral tickets.
pub trait DeferralStore {
    /// Persists a deferral ticket.
    fn save(&self, deferral: &Deferral) -> Result<(), PromoteError>;
    /// Loads a deferral ticket by ID.
    fn load(&self, ticket: &str) -> Result<Deferral, PromoteError>;
    /// Lists every stored deferral ticket.
    fn list_all(&self) -> Result<Vec<Deferral>, PromoteError>;
    /// Lists stored deferral tickets that are still pending.
    fn list_pending(&self) -> Result<Vec<Deferral>, PromoteError>;
}

/// Port: notify external systems about promotion events.
pub trait Notifier {
    /// Notifies an external system that a promotion was deferred.
    fn on_deferred(&self, deferral: &Deferral) -> Result<(), PromoteError>;
}

/// Result of a fast-forward feasibility check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfStatus {
    /// Both refs point to the same commit — nothing to promote.
    InSync,
    /// `to` is a strict ancestor of `from` — FF merge is possible.
    Promotable,
    /// Branches have diverged — FF is not possible.
    Diverged,
}

/// Port: CI-level branch promote operations (fetch, divergence check, FF merge, push).
pub trait CiBranchPromoter {
    /// Fetches the specified branches from a remote.
    fn fetch(&self, remote: &str, branches: &[&str]) -> Result<(), PromoteError>;
    /// Resolve `{remote}/{branch}` to a short (7-char) SHA.
    fn remote_sha(&self, remote: &str, branch: &str) -> Result<String, PromoteError>;
    /// Classifies whether `to` can be fast-forwarded to `from`.
    fn ff_status(&self, remote: &str, from: &str, to: &str) -> Result<FfStatus, PromoteError>;
    /// Checks out `to` and fast-forwards it to `remote/from`.
    fn checkout_and_ff_merge(&self, remote: &str, from: &str, to: &str)
    -> Result<(), PromoteError>;
    /// Pushes a branch to the specified remote.
    fn push_branch_to(&self, remote: &str, branch: &str) -> Result<(), PromoteError>;
    /// Pushes all local tags to the specified remote.
    fn push_all_tags_to(&self, remote: &str) -> Result<(), PromoteError>;
}

/// Port: bump the patch version of a package via cargo-rail.
pub trait RailBumper {
    /// Run `cargo rail release run <package> --bump=patch --skip-publish --yes`.
    /// Returns the new version string.
    fn patch_bump(&self, package: &str) -> Result<String, PromoteError>;
}

/// Port: interact with a code forge (Gitea, GitHub, etc.).
pub trait Forge {
    /// Create a release on the forge.
    fn create_release(&self, tag: &str, name: &str, body: &str) -> Result<(), PromoteError>;

    /// Create a pull request. Returns the PR number.
    fn create_pr(
        &self,
        title: &str,
        body: &str,
        head: &str,
        base: &str,
    ) -> Result<u64, PromoteError>;

    /// Comment on a pull request (or issue).
    fn comment_pr(&self, pr_number: u64, body: &str) -> Result<(), PromoteError>;

    /// Close a pull request.
    fn close_pr(&self, pr_number: u64) -> Result<(), PromoteError>;
}

/// No-op implementation of `Forge` for when no forge is configured.
pub struct NoopForge;

impl Forge for NoopForge {
    fn create_release(&self, _tag: &str, _name: &str, _body: &str) -> Result<(), PromoteError> {
        Ok(())
    }

    fn create_pr(
        &self,
        _title: &str,
        _body: &str,
        _head: &str,
        _base: &str,
    ) -> Result<u64, PromoteError> {
        Ok(0)
    }

    fn comment_pr(&self, _pr_number: u64, _body: &str) -> Result<(), PromoteError> {
        Ok(())
    }

    fn close_pr(&self, _pr_number: u64) -> Result<(), PromoteError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_forge_create_release_returns_ok() {
        let forge = NoopForge;
        assert!(
            forge
                .create_release("v0.1.0", "Release 0.1.0", "body")
                .is_ok()
        );
    }

    #[test]
    fn noop_forge_create_pr_returns_zero() {
        let forge = NoopForge;
        let pr = forge.create_pr("title", "body", "head", "base").unwrap();
        assert_eq!(pr, 0);
    }

    #[test]
    fn noop_forge_comment_pr_returns_ok() {
        let forge = NoopForge;
        assert!(forge.comment_pr(1, "comment").is_ok());
    }

    #[test]
    fn noop_forge_close_pr_returns_ok() {
        let forge = NoopForge;
        assert!(forge.close_pr(1).is_ok());
    }
}
