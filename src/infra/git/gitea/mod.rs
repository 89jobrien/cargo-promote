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
    pub fn auth_header(token: &str) -> String {
        if token.starts_with("Bearer ") || token.starts_with("bearer ") {
            format!("Authorization: {token}")
        } else {
            format!("Authorization: Bearer {token}")
        }
    }
}

impl GiteaRegistry {
    /// Build an authenticated curl command for the given URL.
    // qual:allow(iosp) reason: "I/O boundary — builds command with optional auth"
    fn curl_cmd(
        &self,
        url: &str,
        registry_name: &str,
        extra_args: &[&str],
    ) -> Result<Command, PromoteError> {
        let token = self.token_resolver.resolve(registry_name)?;
        let mut cmd = Command::new("curl");
        cmd.args(extra_args);
        if let Some(ref secret) = token {
            let header = Self::auth_header(secret.expose_secret());
            cmd.args(["-H", &header]);
        }
        cmd.arg(url);
        Ok(cmd)
    }

    fn require_api_url(registry: &Registry) -> Result<&str, PromoteError> {
        registry
            .api_url
            .as_deref()
            .ok_or_else(|| PromoteError::QueryFailed {
                registry: registry.name.clone(),
                reason: "no api_url configured".to_string(),
            })
    }

    fn run_curl(cmd: &mut Command, registry_name: &str) -> Result<std::process::Output, PromoteError> {
        cmd.output().map_err(|e| PromoteError::QueryFailed {
            registry: registry_name.to_string(),
            reason: format!("failed to run curl: {e}"),
        })
    }
}

impl RegistryQuery for GiteaRegistry {
    // qual:allow(iosp) reason: "I/O boundary — runs curl and interprets status"
    fn crate_exists(
        &self,
        registry: &Registry,
        name: &str,
        version: &str,
    ) -> Result<bool, PromoteError> {
        let api_url = Self::require_api_url(registry)?;
        let url = format!("{api_url}/{name}/{version}");
        let mut cmd = self.curl_cmd(&url, &registry.name, &["-sf", "-o", "/dev/null", "-w", "%{http_code}"])?;
        let output = Self::run_curl(&mut cmd, &registry.name)?;
        let status_code = String::from_utf8_lossy(&output.stdout);
        Ok(status_code.trim() == "200")
    }

    // qual:allow(iosp) reason: "I/O boundary — runs curl and parses JSON response"
    fn list_crates(&self, registry: &Registry) -> Result<Vec<CrateInfo>, PromoteError> {
        let api_url = Self::require_api_url(registry)?;
        let url = format!("{api_url}/api/v1/crates");
        let mut cmd = self.curl_cmd(&url, &registry.name, &["-sf"])?;
        let output = Self::run_curl(&mut cmd, &registry.name)?;

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
