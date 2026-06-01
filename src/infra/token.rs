use std::path::PathBuf;

use secrecy::SecretString;

use crate::domain::PromoteError;
use crate::domain::traits::TokenResolver;

/// Adapter: resolves registry auth tokens from env vars, then
/// `~/.cargo/credentials.toml`.
pub struct CargoTokenResolver {
    credentials_path: PathBuf,
}

impl Default for CargoTokenResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl CargoTokenResolver {
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        Self {
            credentials_path: PathBuf::from(home).join(".cargo/credentials.toml"),
        }
    }

    /// Build with a custom credentials path (for testing).
    pub fn with_credentials_path(path: PathBuf) -> Self {
        Self {
            credentials_path: path,
        }
    }

    /// Normalize a registry name to the env var form:
    /// uppercase, hyphens to underscores.
    fn env_var_name(registry_name: &str) -> String {
        let norm = registry_name.to_uppercase().replace('-', "_");
        format!("CARGO_REGISTRIES_{norm}_TOKEN")
    }
}

impl TokenResolver for CargoTokenResolver {
    // qual:allow(iosp) reason: "I/O boundary — env lookup + file fallback"
    fn resolve(&self, registry_name: &str) -> Result<Option<SecretString>, PromoteError> {
        // 1. Check CARGO_REGISTRIES_{NAME}_TOKEN
        let env_key = Self::env_var_name(registry_name);
        if let Ok(val) = std::env::var(&env_key) {
            if !val.is_empty() {
                return Ok(Some(SecretString::from(val)));
            }
        }

        // 2. For crates-io, also check CARGO_REGISTRY_TOKEN
        if registry_name == "crates-io" {
            if let Ok(val) = std::env::var("CARGO_REGISTRY_TOKEN") {
                if !val.is_empty() {
                    return Ok(Some(SecretString::from(val)));
                }
            }
        }

        // 3. Fall back to credentials.toml
        if self.credentials_path.exists() {
            let contents = std::fs::read_to_string(&self.credentials_path).map_err(|e| {
                PromoteError::Other(anyhow::anyhow!(
                    "failed to read credentials at {}: {e}",
                    self.credentials_path.display()
                ))
            })?;
            let doc: toml::Value = contents.parse().map_err(|e| {
                PromoteError::Other(anyhow::anyhow!(
                    "invalid TOML in {}: {e}",
                    self.credentials_path.display()
                ))
            })?;

            if let Some(token) = doc
                .get("registries")
                .and_then(|r| r.get(registry_name))
                .and_then(|r| r.get("token"))
                .and_then(|t| t.as_str())
            {
                return Ok(Some(SecretString::from(token.to_owned())));
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn resolve_from_env_var() {
        let key = "CARGO_REGISTRIES_CRATEBOX_TOKEN";
        // Safety: test is single-threaded for this env var
        unsafe { std::env::set_var(key, "test-token-123") };

        let resolver = CargoTokenResolver::with_credentials_path(PathBuf::from("/nonexistent"));
        let result = resolver.resolve("cratebox").unwrap();
        assert_eq!(
            result.as_ref().map(|s| s.expose_secret().as_ref()),
            Some("test-token-123")
        );

        unsafe { std::env::remove_var(key) };
    }

    #[test]
    fn resolve_crates_io_fallback_env() {
        let key = "CARGO_REGISTRY_TOKEN";
        unsafe { std::env::set_var(key, "crates-io-token") };

        let resolver = CargoTokenResolver::with_credentials_path(PathBuf::from("/nonexistent"));
        let result = resolver.resolve("crates-io").unwrap();
        assert_eq!(
            result.as_ref().map(|s| s.expose_secret().as_ref()),
            Some("crates-io-token")
        );

        unsafe { std::env::remove_var(key) };
    }

    #[test]
    fn resolve_none_when_no_token() {
        // Ensure env var is not set
        let key = "CARGO_REGISTRIES_NONEXISTENT_TOKEN";
        unsafe { std::env::remove_var(key) };

        let resolver = CargoTokenResolver::with_credentials_path(PathBuf::from("/nonexistent"));
        let result = resolver.resolve("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn resolve_from_credentials_file() {
        let dir = tempfile::tempdir().unwrap();
        let cred_path = dir.path().join("credentials.toml");
        std::fs::write(
            &cred_path,
            "[registries.myrepo]\ntoken = \"file-token-456\"\n",
        )
        .unwrap();

        // Ensure env var is not set
        unsafe { std::env::remove_var("CARGO_REGISTRIES_MYREPO_TOKEN") };

        let resolver = CargoTokenResolver::with_credentials_path(cred_path);
        let result = resolver.resolve("myrepo").unwrap();
        assert_eq!(
            result.as_ref().map(|s| s.expose_secret().as_ref()),
            Some("file-token-456")
        );
    }

    #[test]
    fn env_var_takes_precedence_over_file() {
        let dir = tempfile::tempdir().unwrap();
        let cred_path = dir.path().join("credentials.toml");
        std::fs::write(
            &cred_path,
            "[registries.precedence]\ntoken = \"file-token\"\n",
        )
        .unwrap();

        let key = "CARGO_REGISTRIES_PRECEDENCE_TOKEN";
        unsafe { std::env::set_var(key, "env-token") };

        let resolver = CargoTokenResolver::with_credentials_path(cred_path);
        let result = resolver.resolve("precedence").unwrap();
        assert_eq!(
            result.as_ref().map(|s| s.expose_secret().as_ref()),
            Some("env-token")
        );

        unsafe { std::env::remove_var(key) };
    }
}
