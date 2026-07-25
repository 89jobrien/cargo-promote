use crate::domain::PromoteError;
use crate::domain::traits::RailBumper;
use std::path::{Path, PathBuf};
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

/// Reads `Cargo.toml` in `cargo_toml_dir` and extracts the `[package].version`
/// field. Uses proper TOML parsing so it can't be tricked by `version = "..."`
/// lines belonging to `[dependencies.*]` tables, and reports clear errors for
/// missing files, unparsable TOML, or a missing/malformed version field.
fn extract_package_version(cargo_toml_dir: &Path) -> Result<String, PromoteError> {
    let path = cargo_toml_dir.join("Cargo.toml");

    let contents = std::fs::read_to_string(&path).map_err(|e| {
        PromoteError::Other(anyhow::anyhow!(
            "failed to read {}: {e}",
            path.display()
        ))
    })?;

    let parsed: toml::Value = contents.parse().map_err(|e| {
        PromoteError::Other(anyhow::anyhow!("failed to parse {}: {e}", path.display()))
    })?;

    parsed
        .get("package")
        .and_then(|pkg| pkg.get("version"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            PromoteError::Other(anyhow::anyhow!(
                "could not read [package].version from {}",
                path.display()
            ))
        })
}

impl RailBumper for ProcessRailBumper {
    fn patch_bump(&self, package: &str) -> Result<String, PromoteError> {
        let status = Command::new("cargo")
            .args([
                "rail",
                "release",
                "run",
                package,
                "--bump=patch",
                "--skip-publish",
                "--yes",
            ])
            .current_dir(&self.repo_root)
            .status()
            .map_err(|e| {
                PromoteError::Other(anyhow::anyhow!(
                    "failed to invoke `cargo rail` (is it installed and on PATH?): {e}"
                ))
            })?;

        if !status.success() {
            return Err(PromoteError::Other(anyhow::anyhow!(
                "cargo rail release run {package} --bump=patch failed"
            )));
        }

        extract_package_version(&self.repo_root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn missing_cargo_toml_returns_error_not_panic() {
        let dir = TempDir::new().unwrap();

        let result = extract_package_version(dir.path());

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("failed to read"), "unexpected message: {msg}");
    }

    #[test]
    fn cargo_toml_without_version_line_returns_error() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"foo\"\n",
        )
        .unwrap();

        let result = extract_package_version(dir.path());

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("[package].version"),
            "unexpected message: {msg}"
        );
    }

    #[test]
    fn malformed_version_line_no_quotes_is_invalid_toml() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"foo\"\nversion = 1.0.0\n",
        )
        .unwrap();

        // `version = 1.0.0` without quotes is not valid TOML (bare float/ident),
        // so this must fail with a clear parse error rather than silently
        // succeeding with a wrong value.
        let result = extract_package_version(dir.path());

        assert!(result.is_err());
    }

    #[test]
    fn version_line_without_spaces_around_equals_still_parses() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname=\"foo\"\nversion=\"1.0.0\"\n",
        )
        .unwrap();

        let result = extract_package_version(dir.path()).unwrap();

        assert_eq!(result, "1.0.0");
    }

    #[test]
    fn picks_package_version_not_dependency_version() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"
[package]
name = "foo"
version = "2.3.4"

[dependencies.bar]
path = "../bar"
version = "9.9.9"
"#,
        )
        .unwrap();

        let result = extract_package_version(dir.path()).unwrap();

        assert_eq!(
            result, "2.3.4",
            "must pick [package].version, not a nested dependency version"
        );
    }

    #[test]
    fn valid_simple_cargo_toml_parses() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"foo\"\nversion = \"0.1.5\"\n",
        )
        .unwrap();

        let result = extract_package_version(dir.path()).unwrap();

        assert_eq!(result, "0.1.5");
    }
}
