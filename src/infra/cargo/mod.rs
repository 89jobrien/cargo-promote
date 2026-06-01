use crate::domain::traits::Publisher;
use crate::domain::{CrateRef, PromoteError, PublishOpts, Registry};
use std::process::Command;

/// Adapter: publishes via `cargo publish`.
pub struct CargoPublisher;

impl Publisher for CargoPublisher {
    fn publish(
        &self,
        krate: &CrateRef,
        registry: &Registry,
        opts: &PublishOpts,
    ) -> Result<(), PromoteError> {
        let mut cmd = Command::new("cargo");
        cmd.arg("publish");

        // Use explicit cargo_name if set, otherwise fall back to the
        // registry name itself so we never rely on cargo's default
        // registry setting.
        let cargo_name = registry
            .cargo_name
            .as_deref()
            .unwrap_or(&registry.name);
        cmd.arg("--registry").arg(cargo_name);

        cmd.arg("--manifest-path").arg(&krate.manifest_path);

        if opts.allow_dirty {
            cmd.arg("--allow-dirty");
        }
        if opts.dry_run {
            cmd.arg("--dry-run");
        }

        let label = cargo_name;
        if opts.dry_run {
            eprintln!("=> Dry run: would publish {} to {}", krate.name, label);
        } else {
            eprintln!(
                "=> Publishing {} v{} to {}...",
                krate.name, krate.version, label
            );
        }

        let status = cmd.status().map_err(|e| PromoteError::PublishFailed {
            registry: registry.name.clone(),
            reason: format!("failed to run cargo publish: {e}"),
        })?;

        if !status.success() {
            return Err(PromoteError::PublishFailed {
                registry: registry.name.clone(),
                reason: format!("cargo publish exited with {status}"),
            });
        }

        if !opts.dry_run {
            eprintln!("=> Published {} to {}", krate.name, label);
        }
        Ok(())
    }
}
