use super::{CrateInfo, CrateRef, PromoteError, PublishOpts, Registry};

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
