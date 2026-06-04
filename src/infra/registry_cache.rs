use std::cell::RefCell;
use std::collections::HashMap;

use crate::domain::traits::RegistryQuery;
use crate::domain::{CrateInfo, PromoteError, Registry};

/// Decorator: caches `list_crates` and `crate_exists` results per
/// registry name so repeated queries within a single run hit memory
/// instead of the network.
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
        let crates = self.get_or_fetch(registry)?;
        Ok(crates
            .iter()
            .any(|c| c.name == name && c.max_version == version))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct CountingQuery {
        calls: Cell<usize>,
        crates: Vec<CrateInfo>,
    }

    impl CountingQuery {
        fn new(crates: Vec<CrateInfo>) -> Self {
            Self {
                calls: Cell::new(0),
                crates,
            }
        }
    }

    impl RegistryQuery for CountingQuery {
        fn list_crates(&self, _registry: &Registry) -> Result<Vec<CrateInfo>, PromoteError> {
            self.calls.set(self.calls.get() + 1);
            Ok(self.crates.clone())
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
        let inner = CountingQuery::new(vec![CrateInfo {
            name: "foo".to_string(),
            max_version: "0.1.0".to_string(),
        }]);
        let cached = CachingRegistryQuery::new(inner);
        let reg = test_registry();

        let r1 = cached.list_crates(&reg).unwrap();
        let r2 = cached.list_crates(&reg).unwrap();
        assert_eq!(r1.len(), 1);
        assert_eq!(r2.len(), 1);
        assert_eq!(cached.inner.calls.get(), 1);
    }

    #[test]
    fn crate_exists_uses_cache() {
        let inner = CountingQuery::new(vec![CrateInfo {
            name: "bar".to_string(),
            max_version: "1.0.0".to_string(),
        }]);
        let cached = CachingRegistryQuery::new(inner);
        let reg = test_registry();

        assert!(cached.crate_exists(&reg, "bar", "1.0.0").unwrap());
        assert!(!cached.crate_exists(&reg, "bar", "2.0.0").unwrap());
        assert_eq!(cached.inner.calls.get(), 1);
    }
}
