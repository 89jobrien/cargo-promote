use crate::domain::traits::RegistryQuery;
use crate::domain::{CrateInfo, PromoteError, Registry};
use std::process::Command;

// TODO(cargo-utils, #1): add TokenResolver port to resolve registry auth tokens
// via env vars (CARGO_REGISTRIES_{NAME}_TOKEN) then ~/.cargo/credentials.toml,
// wrapped in secrecy::SecretString. Pass Bearer token to curl calls here.
// Ref: release-plz/cargo_utils/src/token.rs

/// Adapter: queries a Gitea cargo packages API for crate listings.
pub struct GiteaRegistry;

impl RegistryQuery for GiteaRegistry {
    fn list_crates(&self, registry: &Registry) -> Result<Vec<CrateInfo>, PromoteError> {
        let api_url = registry
            .api_url
            .as_deref()
            .ok_or_else(|| PromoteError::QueryFailed {
                registry: registry.name.clone(),
                reason: "no api_url configured".to_string(),
            })?;

        let url = format!("{api_url}/api/v1/crates");

        let output = Command::new("curl")
            .args(["-sf", &url])
            .output()
            .map_err(|e| PromoteError::QueryFailed {
                registry: registry.name.clone(),
                reason: format!("failed to run curl: {e}"),
            })?;

        if !output.status.success() {
            return Err(PromoteError::QueryFailed {
                registry: registry.name.clone(),
                reason: format!("registry unreachable at {url}"),
            });
        }

        let body: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|e| PromoteError::QueryFailed {
                registry: registry.name.clone(),
                reason: format!("invalid JSON: {e}"),
            })?;

        let crates = body["crates"]
            .as_array()
            .ok_or_else(|| PromoteError::QueryFailed {
                registry: registry.name.clone(),
                reason: "unexpected response format".to_string(),
            })?;

        Ok(crates
            .iter()
            .map(|c| CrateInfo {
                name: c["name"].as_str().unwrap_or("?").to_string(),
                max_version: c["max_version"].as_str().unwrap_or("?").to_string(),
            })
            .collect())
    }
}
