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
