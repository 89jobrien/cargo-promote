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

    /// Check if a specific crate@version already exists on the registry.
    /// TODO(#6): implement in GiteaRegistry adapter
    fn crate_exists(
        &self,
        _registry: &Registry,
        _name: &str,
        _version: &str,
    ) -> Result<bool, PromoteError> {
        // Default: assume not published (backwards-compatible).
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

/// Port: notify external systems about promotion events.
pub trait Notifier {
    fn on_deferred(&self, deferral: &Deferral) -> Result<(), PromoteError>;
}

/// Port: interact with a git forge (Gitea, GitHub, GitLab) for
/// PR/release management.
/// TODO(#8): implement GiteaForge and GitHubForge adapters
pub trait Forge {
    /// Create a release for the given tag.
    fn create_release(&self, tag: &str, body: &str) -> Result<(), PromoteError>;

    /// Open a pull request and return its number.
    fn create_pr(
        &self,
        title: &str,
        body: &str,
        head: &str,
        base: &str,
    ) -> Result<u64, PromoteError>;

    /// Post a comment on an existing PR.
    fn comment_pr(&self, pr_number: u64, body: &str) -> Result<(), PromoteError>;

    /// Close a PR.
    fn close_pr(&self, pr_number: u64) -> Result<(), PromoteError>;
}

/// No-op forge for environments without forge access.
pub struct NoopForge;

impl Forge for NoopForge {
    fn create_release(&self, _tag: &str, _body: &str) -> Result<(), PromoteError> {
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
