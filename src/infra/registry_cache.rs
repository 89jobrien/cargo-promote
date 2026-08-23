use std::cell::RefCell;
use std::collections::HashMap;

use crate::domain::traits::RegistryQuery;
use crate::domain::{CrateInfo, PromoteError, Registry};

/// Decorator: caches `list_crates` results per registry name so repeated
/// listing queries within a single run hit memory instead of the network.
/// `crate_exists` is delegated to the inner query unchanged — `list_crates`
/// returns only `max_version` per crate, so a cache scan would silently
/// return false for any version that is not the latest.
pub struct CachingRegistryQuery<Q> {
    inner: Q,
    cache: RefCell<HashMap<String, Vec<CrateInfo>>>,
}

impl<Q: RegistryQuery> CachingRegistryQuery<Q> {
    pub fn new(inner: Q) -> Self {
        Self {
            inner,
            cache: RefCell::new(HashMap::new()),
        }
    }

    fn get_or_fetch(&self, registry: &Registry) -> Result<Vec<CrateInfo>, PromoteError> {
        if let Some(cached) = self.cache.borrow().get(&registry.name) {
            return Ok(cached.clone());
        }
        let result = self.inner.list_crates(registry)?;
        self.cache
            .borrow_mut()
            .insert(registry.name.clone(), result.clone());
        Ok(result)
    }
}

impl<Q: RegistryQuery> RegistryQuery for CachingRegistryQuery<Q> {
    fn list_crates(&self, registry: &Registry) -> Result<Vec<CrateInfo>, PromoteError> {
        self.get_or_fetch(registry)
    }

    fn crate_exists(
        &self,
        registry: &Registry,
        name: &str,
        version: &str,
    ) -> Result<bool, PromoteError> {
        self.inner.crate_exists(registry, name, version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Whether a specific crate version is known to exist on the registry.
    #[derive(Clone, Copy)]
    enum Existence {
        Present,
        Absent,
    }

    /// Test double that records call counts and answers from explicit fixtures.
    ///
    /// `list_crates` returns `crates` (one entry per crate, max_version only).
    /// `crate_exists` looks up `(name, version)` in `published` — the full set
    /// of published versions, independent of what `list_crates` advertises.
    /// This models the real registry state: list returns only the latest, but
    /// any historical version can still exist.
    ///
    /// Versions not present in `published` panic — unregistered queries are a
    /// test bug, not a legitimate "absent" answer.
    struct StubQuery {
        list_calls: Cell<usize>,
        exists_calls: Cell<usize>,
        /// What list_crates returns: one entry per crate, max_version only.
        crates: Vec<CrateInfo>,
        /// Explicit existence fixture for each (name, version) under test.
        published: HashMap<(String, String), Existence>,
    }

    impl StubQuery {
        fn new(crates: Vec<CrateInfo>) -> Self {
            Self {
                list_calls: Cell::new(0),
                exists_calls: Cell::new(0),
                crates,
                published: HashMap::new(),
            }
        }

        fn with_published(mut self, name: &str, version: &str) -> Self {
            self.published
                .insert((name.to_string(), version.to_string()), Existence::Present);
            self
        }

        fn with_absent(mut self, name: &str, version: &str) -> Self {
            self.published
                .insert((name.to_string(), version.to_string()), Existence::Absent);
            self
        }
    }

    impl RegistryQuery for StubQuery {
        fn list_crates(&self, _registry: &Registry) -> Result<Vec<CrateInfo>, PromoteError> {
            self.list_calls.set(self.list_calls.get() + 1);
            Ok(self.crates.clone())
        }

        fn crate_exists(
            &self,
            _registry: &Registry,
            name: &str,
            version: &str,
        ) -> Result<bool, PromoteError> {
            self.exists_calls.set(self.exists_calls.get() + 1);
            match self.published.get(&(name.to_string(), version.to_string())) {
                Some(Existence::Present) => Ok(true),
                Some(Existence::Absent) => Ok(false),
                None => Err(PromoteError::Other(anyhow::anyhow!(
                    "StubQuery: no fixture registered for {name} {version}"
                ))),
            }
        }
    }

    fn test_registry() -> Registry {
        Registry {
            name: "test".to_string(),
            cargo_name: Some("test".to_string()),
            api_url: None,
            confirm: false,
        }
    }

    #[test]
    fn caches_list_crates() {
        let inner = StubQuery::new(vec![CrateInfo {
            name: "foo".to_string(),
            max_version: "0.1.0".to_string(),
        }]);
        let cached = CachingRegistryQuery::new(inner);
        let reg = test_registry();

        let r1 = cached.list_crates(&reg).unwrap();
        let r2 = cached.list_crates(&reg).unwrap();
        assert_eq!(r1.len(), 1);
        assert_eq!(r2.len(), 1);
        assert_eq!(cached.inner.list_calls.get(), 1);
    }

    /// Regression: the old implementation scanned `max_version` from the
    /// listing cache to answer `crate_exists`. This broke for older versions:
    /// if "bar 1.0.0" was published and "bar 2.0.0" was later published,
    /// `crate_exists("bar", "1.0.0")` returned false (2.0.0 != 1.0.0) and
    /// the pipeline would re-publish 1.0.0, potentially overwriting it.
    #[test]
    fn crate_exists_finds_older_version_not_listed_as_max() {
        // Registry state: bar has two published versions; listing shows max only.
        let inner = StubQuery::new(vec![CrateInfo {
            name: "bar".to_string(),
            max_version: "2.0.0".to_string(),
        }])
        .with_published("bar", "1.0.0")
        .with_published("bar", "2.0.0")
        .with_absent("bar", "3.0.0");
        let cached = CachingRegistryQuery::new(inner);
        let reg = test_registry();

        // Older version exists — would be missed by a max_version scan.
        assert!(cached.crate_exists(&reg, "bar", "1.0.0").unwrap());
        // Max version also found.
        assert!(cached.crate_exists(&reg, "bar", "2.0.0").unwrap());
        // Never published version correctly absent.
        assert!(!cached.crate_exists(&reg, "bar", "3.0.0").unwrap());
        // All three answers came from the inner point query, not the listing.
        assert_eq!(cached.inner.exists_calls.get(), 3);
        assert_eq!(cached.inner.list_calls.get(), 0);
    }

    #[test]
    fn crate_exists_does_not_trigger_list_crates() {
        // list_crates must not be called as a side effect of crate_exists.
        let inner = StubQuery::new(vec![]).with_published("foo", "0.1.0");
        let cached = CachingRegistryQuery::new(inner);
        let reg = test_registry();

        cached.crate_exists(&reg, "foo", "0.1.0").unwrap();
        assert_eq!(cached.inner.list_calls.get(), 0);
    }
}
