use super::deferral::Deferral;
use super::{CrateInfo, CrateRef, Pipeline, PromoteError, PublishOpts, Registry, Stage};

/// Port: publish a crate to a registry.
pub trait Publisher {
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
    fn fast_forward(&self, source: &str, target: &str) -> Result<(), PromoteError>;
}

impl<T: BranchMerger> BranchMerger for &T {
    fn fast_forward(&self, source: &str, target: &str) -> Result<(), PromoteError> {
        (**self).fast_forward(source, target)
    }
}

/// Port: push branches and tags to a remote.
pub trait RemotePusher {
    fn push_branch(&self, branch: &str) -> Result<(), PromoteError>;
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
    fn create_tag(&self, name: &str, message: &str) -> Result<(), PromoteError>;
}

impl<T: Tagger> Tagger for &T {
    fn create_tag(&self, name: &str, message: &str) -> Result<(), PromoteError> {
        (**self).create_tag(name, message)
    }
}

/// Port: stage, commit, and push changes to a local repository.
pub trait GitCommitter {
    fn stage(&self, files: &[&str]) -> Result<(), PromoteError>;
    fn commit(&self, message: &str) -> Result<(), PromoteError>;
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

/// Port: resolve authentication tokens for registries.
pub trait TokenResolver {
    fn resolve(&self, registry_name: &str) -> Result<Option<secrecy::SecretString>, PromoteError>;
}

/// Port: persist and query deferral tickets.
pub trait DeferralStore {
    fn save(&self, deferral: &Deferral) -> Result<(), PromoteError>;
    fn load(&self, ticket: &str) -> Result<Deferral, PromoteError>;
    fn list_all(&self) -> Result<Vec<Deferral>, PromoteError>;
    fn list_pending(&self) -> Result<Vec<Deferral>, PromoteError>;
}

/// Port: notify external systems about promotion events.
pub trait Notifier {
    fn on_deferred(&self, deferral: &Deferral) -> Result<(), PromoteError>;
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
