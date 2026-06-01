use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// A parsed Cargo.toml that preserves formatting via `toml_edit`.
pub struct LocalManifest {
    pub path: PathBuf,
    pub data: toml_edit::DocumentMut,
}

impl LocalManifest {
    /// Read and parse a Cargo.toml from disk.
    pub fn try_new(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        let data: toml_edit::DocumentMut = content
            .parse()
            .with_context(|| format!("invalid TOML in {}", path.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
            data,
        })
    }

    /// Write the document back to disk, preserving formatting.
    pub fn write(&self) -> Result<()> {
        std::fs::write(&self.path, self.data.to_string())
            .with_context(|| format!("cannot write {}", self.path.display()))
    }

    /// Return the `[package].name` value.
    pub fn package_name(&self) -> Option<&str> {
        self.data
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
    }

    /// Return the `[package].version` string value (not workspace-inherited).
    pub fn package_version(&self) -> Option<&str> {
        self.data
            .get("package")
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())
    }

    /// Set the `[package].version` to the given semver version.
    pub fn set_package_version(&mut self, version: &semver::Version) {
        self.data["package"]["version"] = toml_edit::value(version.to_string());
    }

    /// Returns `true` if version is inherited from workspace (`version.workspace = true`).
    pub fn version_is_inherited(&self) -> bool {
        let Some(pkg) = self.data.get("package") else {
            return false;
        };
        let Some(version_item) = pkg.get("version") else {
            return false;
        };
        // version.workspace = true shows up as an inline table { workspace = true }
        if let Some(tbl) = version_item.as_inline_table() {
            return tbl
                .get("workspace")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        }
        if let Some(tbl) = version_item.as_table() {
            return tbl
                .get("workspace")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        }
        false
    }

    /// Get `[workspace.package].version` parsed as semver.
    pub fn get_workspace_version(&self) -> Option<semver::Version> {
        let v_str = self
            .data
            .get("workspace")
            .and_then(|w| w.get("package"))
            .and_then(|p| p.get("version"))
            .and_then(|v| v.as_str())?;
        semver::Version::parse(v_str).ok()
    }

    /// Set `[workspace.package].version`.
    pub fn set_workspace_version(&mut self, version: &semver::Version) {
        self.data["workspace"]["package"]["version"] = toml_edit::value(version.to_string());
    }

    /// Get workspace members array as strings.
    pub fn workspace_members(&self) -> Option<Vec<&str>> {
        self.data
            .get("workspace")
            .and_then(|w| w.get("members"))
            .and_then(|m| m.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_new_reads_valid_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("Cargo.toml");
        std::fs::write(
            &manifest,
            "[package]\nname = \"foo\"\nversion = \"1.2.3\"\n",
        )
        .unwrap();

        let lm = LocalManifest::try_new(&manifest).unwrap();
        assert_eq!(lm.package_name(), Some("foo"));
        assert_eq!(lm.package_version(), Some("1.2.3"));
    }

    #[test]
    fn version_is_inherited_detects_workspace_true() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("Cargo.toml");
        std::fs::write(
            &manifest,
            "[package]\nname = \"child\"\nversion.workspace = true\n",
        )
        .unwrap();

        let lm = LocalManifest::try_new(&manifest).unwrap();
        assert!(lm.version_is_inherited());
    }

    #[test]
    fn version_is_inherited_false_for_string() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("Cargo.toml");
        std::fs::write(
            &manifest,
            "[package]\nname = \"foo\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let lm = LocalManifest::try_new(&manifest).unwrap();
        assert!(!lm.version_is_inherited());
    }

    #[test]
    fn set_package_version_and_write_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("Cargo.toml");
        std::fs::write(
            &manifest,
            "[package]\nname = \"foo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();

        let mut lm = LocalManifest::try_new(&manifest).unwrap();
        lm.set_package_version(&semver::Version::new(0, 2, 0));
        lm.write().unwrap();

        let content = std::fs::read_to_string(&manifest).unwrap();
        assert!(content.contains("version = \"0.2.0\""));
        assert!(content.contains("name = \"foo\""));
        assert!(content.contains("edition = \"2024\""));
    }

    #[test]
    fn package_name_returns_correct_name() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("Cargo.toml");
        std::fs::write(
            &manifest,
            "[package]\nname = \"my-crate\"\nversion = \"0.0.1\"\n",
        )
        .unwrap();

        let lm = LocalManifest::try_new(&manifest).unwrap();
        assert_eq!(lm.package_name(), Some("my-crate"));
    }

    #[test]
    fn workspace_version_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("Cargo.toml");
        std::fs::write(
            &manifest,
            "[workspace.package]\nversion = \"1.0.0\"\n\n[workspace]\nmembers = [\"a\"]\n",
        )
        .unwrap();

        let mut lm = LocalManifest::try_new(&manifest).unwrap();
        assert_eq!(
            lm.get_workspace_version(),
            Some(semver::Version::new(1, 0, 0))
        );
        lm.set_workspace_version(&semver::Version::new(2, 0, 0));
        lm.write().unwrap();

        let content = std::fs::read_to_string(&manifest).unwrap();
        assert!(content.contains("version = \"2.0.0\""));
    }
}
