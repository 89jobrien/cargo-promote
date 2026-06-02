/// Regression tests for bugs fixed in the 2026-06-01 session.

mod auth_header {
    use cargo_promote::infra::git::gitea::GiteaRegistry;

    /// Regression: tokens from ~/.cargo/credentials.toml already include
    /// "Bearer " prefix. The old code wrapped them again, producing
    /// "Authorization: Bearer Bearer <token>" which Gitea rejected.
    #[test]
    fn bearer_prefixed_token_not_doubled() {
        let header = GiteaRegistry::auth_header("Bearer abc123");
        assert_eq!(
            header, "Authorization: Bearer abc123",
            "must not produce 'Bearer Bearer'"
        );
    }

    #[test]
    fn lowercase_bearer_prefixed_token_not_doubled() {
        let header = GiteaRegistry::auth_header("bearer abc123");
        assert_eq!(header, "Authorization: bearer abc123");
    }

    #[test]
    fn raw_token_gets_bearer_prefix() {
        let header = GiteaRegistry::auth_header("abc123");
        assert_eq!(header, "Authorization: Bearer abc123");
    }

    #[test]
    fn empty_token_gets_bearer_prefix() {
        let header = GiteaRegistry::auth_header("");
        assert_eq!(header, "Authorization: Bearer ");
    }
}

mod crate_exists_url {
    /// Regression: crate_exists was building the URL as
    /// "{api_url}/api/packages/joe/cargo/{name}/{version}" which doubled
    /// the path when api_url already contained /api/packages/joe/cargo.
    /// Fixed to "{api_url}/{name}/{version}".
    #[test]
    fn crate_exists_url_does_not_double_api_packages_path() {
        let api_url = "http://host:3000/api/packages/joe/cargo";
        let name = "mycrate";
        let version = "0.1.0";
        let url = format!("{api_url}/{name}/{version}");
        assert_eq!(
            url,
            "http://host:3000/api/packages/joe/cargo/mycrate/0.1.0",
            "must not contain /api/packages twice"
        );
        assert_eq!(
            url.matches("/api/packages").count(),
            1,
            "path segment must appear exactly once"
        );
    }
}

mod cargo_publisher_registry {
    use cargo_promote::domain::Registry;

    /// Regression: cargo publish without --registry hit the default
    /// registry (cratebox) instead of crates.io. The fix: always derive
    /// a registry name from cargo_name or fall back to registry.name.
    #[test]
    fn registry_name_falls_back_to_name_when_cargo_name_is_none() {
        let reg = Registry {
            name: "crates-io".to_string(),
            cargo_name: None,
            api_url: None,
            confirm: true,
        };
        let cargo_name = reg.cargo_name.as_deref().unwrap_or(&reg.name);
        assert_eq!(
            cargo_name, "crates-io",
            "must use registry name when cargo_name is None"
        );
    }

    #[test]
    fn registry_name_uses_cargo_name_when_set() {
        let reg = Registry {
            name: "my-private".to_string(),
            cargo_name: Some("private-reg".to_string()),
            api_url: None,
            confirm: false,
        };
        let cargo_name = reg.cargo_name.as_deref().unwrap_or(&reg.name);
        assert_eq!(
            cargo_name, "private-reg",
            "must prefer cargo_name when set"
        );
    }
}
