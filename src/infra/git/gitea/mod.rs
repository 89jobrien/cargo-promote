pub mod forge;

use std::sync::Arc;

use secrecy::ExposeSecret;

use crate::domain::traits::{RegistryQuery, TokenResolver};
use crate::domain::{CrateInfo, PromoteError, Registry};
use std::process::Command;

/// Adapter: queries a Gitea cargo packages API for crate listings.
pub struct GiteaRegistry {
    token_resolver: Arc<dyn TokenResolver>,
}

impl GiteaRegistry {
    pub fn new(token_resolver: Arc<dyn TokenResolver>) -> Self {
        Self { token_resolver }
    }
}

impl GiteaRegistry {
    /// Build an Authorization header, handling tokens that already
    /// include the `Bearer ` prefix (as stored in credentials.toml).
    fn auth_header(token: &str) -> String {
        if token.starts_with("Bearer ") || token.starts_with("bearer ") {
            format!("Authorization: {token}")
        } else {
            format!("Authorization: Bearer {token}")
        }
    }
}

impl RegistryQuery for GiteaRegistry {
    fn crate_exists(
        &self,
        registry: &Registry,
        name: &str,
        version: &str,
    ) -> Result<bool, PromoteError> {
        let api_url = registry
            .api_url
            .as_deref()
            .ok_or_else(|| PromoteError::QueryFailed {
                registry: registry.name.clone(),
                reason: "no api_url configured".to_string(),
            })?;

        let url = format!("{api_url}/{name}/{version}");

        let token = self.token_resolver.resolve(&registry.name)?;

        let mut cmd = Command::new("curl");
        cmd.args(["-sf", "-o", "/dev/null", "-w", "%{http_code}"]);

        if let Some(ref secret) = token {
            let header = Self::auth_header(secret.expose_secret());
            cmd.args(["-H", &header]);
        }

        cmd.arg(&url);

        let output = cmd.output().map_err(|e| PromoteError::QueryFailed {
            registry: registry.name.clone(),
            reason: format!("failed to run curl: {e}"),
        })?;

        let status_code = String::from_utf8_lossy(&output.stdout);
        Ok(status_code.trim() == "200")
    }

    fn list_crates(&self, registry: &Registry) -> Result<Vec<CrateInfo>, PromoteError> {
        let api_url = registry
            .api_url
            .as_deref()
            .ok_or_else(|| PromoteError::QueryFailed {
                registry: registry.name.clone(),
                reason: "no api_url configured".to_string(),
            })?;

        // api_url includes the cargo index base, e.g.
        // "http://host:port/api/packages/{owner}/cargo"
        // The crates listing lives at {api_url}/api/v1/crates
        let url = format!("{api_url}/api/v1/crates");

        let token = self.token_resolver.resolve(&registry.name)?;

        let mut cmd = Command::new("curl");
        cmd.args(["-sf"]);

        if let Some(ref secret) = token {
            let header = Self::auth_header(secret.expose_secret());
            cmd.args(["-H", &header]);
        }

        cmd.arg(&url);

        let output = cmd.output().map_err(|e| PromoteError::QueryFailed {
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
