use crate::domain::traits::RegistryQuery;
use crate::domain::{CrateInfo, PromoteError, Registry};
use std::process::Command;

/// Adapter: queries GitHub Packages / Releases for crate info.
pub struct GitHubRegistry;

impl RegistryQuery for GitHubRegistry {
    fn list_crates(&self, registry: &Registry) -> Result<Vec<CrateInfo>, PromoteError> {
        let api_url = registry
            .api_url
            .as_deref()
            .ok_or_else(|| PromoteError::QueryFailed {
                registry: registry.name.clone(),
                reason: "no api_url configured for GitHub registry".to_string(),
            })?;

        let output = Command::new("curl")
            .args(["-sf", "-H", "Accept: application/vnd.github+json", api_url])
            .output()
            .map_err(|e| PromoteError::QueryFailed {
                registry: registry.name.clone(),
                reason: format!("failed to run curl: {e}"),
            })?;

        if !output.status.success() {
            return Err(PromoteError::QueryFailed {
                registry: registry.name.clone(),
                reason: format!("GitHub API unreachable at {api_url}"),
            });
        }

        let body: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|e| PromoteError::QueryFailed {
                registry: registry.name.clone(),
                reason: format!("invalid JSON: {e}"),
            })?;

        let packages = body.as_array().ok_or_else(|| PromoteError::QueryFailed {
            registry: registry.name.clone(),
            reason: "expected JSON array from GitHub packages API".to_string(),
        })?;

        Ok(packages
            .iter()
            .map(|p| CrateInfo {
                name: p["name"].as_str().unwrap_or("?").to_string(),
                max_version: p["version"].as_str().unwrap_or("?").to_string(),
            })
            .collect())
    }
}
