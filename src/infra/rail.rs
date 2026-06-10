use crate::domain::PromoteError;
use crate::domain::traits::RailBumper;
use std::path::PathBuf;
use std::process::Command;

/// Adapter: invokes `cargo rail` as a subprocess to perform a patch bump.
pub struct ProcessRailBumper {
    pub repo_root: PathBuf,
}

impl ProcessRailBumper {
    pub fn new(repo_root: PathBuf) -> Self {
        Self { repo_root }
    }
}

impl RailBumper for ProcessRailBumper {
    fn patch_bump(&self, package: &str) -> Result<String, PromoteError> {
        let status = Command::new("cargo")
            .args(["rail", "release", "run", package, "--bump=patch", "--skip-publish", "--yes"])
            .current_dir(&self.repo_root)
            .status()
            .map_err(|e| PromoteError::Other(e.into()))?;

        if !status.success() {
            return Err(PromoteError::Other(anyhow::anyhow!(
                "cargo rail release run {package} --bump=patch failed"
            )));
        }

        // Read the updated version from Cargo.toml.
        let cargo_toml = std::fs::read_to_string(self.repo_root.join("Cargo.toml"))
            .map_err(|e| PromoteError::Other(e.into()))?;

        for line in cargo_toml.lines() {
            if line.starts_with("version = \"") {
                let ver = line
                    .trim_start_matches("version = \"")
                    .trim_end_matches('"');
                return Ok(ver.to_string());
            }
        }

        Err(PromoteError::Other(anyhow::anyhow!(
            "could not read version from Cargo.toml after rail patch bump"
        )))
    }
}
