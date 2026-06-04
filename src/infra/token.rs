use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use secrecy::{ExposeSecret, SecretString};

use crate::domain::PromoteError;
use crate::domain::traits::TokenResolver;

/// Looks up an environment variable by name. Abstracted for testability
/// (avoids `unsafe set_var` in edition 2024 tests).
type EnvLookup = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Adapter: resolves registry auth tokens from env vars, then
/// `~/.cargo/credentials.toml`. Caches results per registry name
/// to avoid repeated env lookups and file reads.
pub struct CargoTokenResolver {
    credentials_path: PathBuf,
    env_lookup: EnvLookup,
    cache: RefCell<HashMap<String, Option<String>>>,
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
            env_lookup: Box::new(|key| std::env::var(key).ok()),
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// Build with a custom credentials path (for testing).
    pub fn with_credentials_path(path: PathBuf) -> Self {
        Self {
            credentials_path: path,
            env_lookup: Box::new(|key| std::env::var(key).ok()),
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// Build with a custom credentials path and env lookup (for testing).
    #[cfg(test)]
    fn with_env(path: PathBuf, env_lookup: impl Fn(&str) -> Option<String> + Send + Sync + 'static) -> Self {
        Self {
            credentials_path: path,
            env_lookup: Box::new(env_lookup),
            cache: RefCell::new(HashMap::new()),
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
        if let Some(cached) = self.cache.borrow().get(registry_name) {
            return Ok(cached.as_ref().map(|s| SecretString::from(s.clone())));
        }
        let result = self.resolve_uncached(registry_name)?;
        self.cache.borrow_mut().insert(
            registry_name.to_string(),
            result.as_ref().map(|s| s.expose_secret().to_string()),
        );
        Ok(result)
    }
}

impl CargoTokenResolver {
    fn resolve_uncached(&self, registry_name: &str) -> Result<Option<SecretString>, PromoteError> {
        // 1. Check CARGO_REGISTRIES_{NAME}_TOKEN
        let env_key = Self::env_var_name(registry_name);
        if let Some(val) = (self.env_lookup)(&env_key) {
            if !val.is_empty() {
                return Ok(Some(SecretString::from(val)));
            }
        }

        // 2. For crates-io, also check CARGO_REGISTRY_TOKEN
        if registry_name == "crates-io" {
            if let Some(val) = (self.env_lookup)("CARGO_REGISTRY_TOKEN") {
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
    use std::collections::HashMap;

    fn mock_env(vars: Vec<(&str, &str)>) -> impl Fn(&str) -> Option<String> + Send + Sync + 'static {
        let map: HashMap<String, String> = vars
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    fn empty_env() -> impl Fn(&str) -> Option<String> + Send + Sync + 'static {
        |_| None
    }

    #[test]
    fn resolve_from_env_var() {
        let resolver = CargoTokenResolver::with_env(
            PathBuf::from("/nonexistent"),
            mock_env(vec![("CARGO_REGISTRIES_CRATEBOX_TOKEN", "test-token-123")]),
        );
        let result = resolver.resolve("cratebox").unwrap();
        assert_eq!(
            result.as_ref().map(|s| s.expose_secret().as_ref()),
            Some("test-token-123")
        );
    }

    #[test]
    fn resolve_crates_io_fallback_env() {
        let resolver = CargoTokenResolver::with_env(
            PathBuf::from("/nonexistent"),
            mock_env(vec![("CARGO_REGISTRY_TOKEN", "crates-io-token")]),
        );
        let result = resolver.resolve("crates-io").unwrap();
        assert_eq!(
            result.as_ref().map(|s| s.expose_secret().as_ref()),
            Some("crates-io-token")
        );
    }

    #[test]
    fn resolve_none_when_no_token() {
        let resolver = CargoTokenResolver::with_env(
            PathBuf::from("/nonexistent"),
            empty_env(),
        );
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

        let resolver = CargoTokenResolver::with_env(cred_path, empty_env());
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

        let resolver = CargoTokenResolver::with_env(
            cred_path,
            mock_env(vec![("CARGO_REGISTRIES_PRECEDENCE_TOKEN", "env-token")]),
        );
        let result = resolver.resolve("precedence").unwrap();
        assert_eq!(
            result.as_ref().map(|s| s.expose_secret().as_ref()),
            Some("env-token")
        );
    }
}
