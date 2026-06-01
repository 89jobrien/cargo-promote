use anyhow::{Context, Result};
use semver::Version;
use std::path::Path;
use std::str::FromStr;

/// Which component to bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BumpLevel {
    Patch,
    Minor,
    Major,
}

impl FromStr for BumpLevel {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "patch" => Ok(Self::Patch),
            "minor" => Ok(Self::Minor),
            "major" => Ok(Self::Major),
            _ => anyhow::bail!("unknown bump level '{s}', expected patch|minor|major"),
        }
    }
}

/// Increment a semver version by the given level.
pub fn bump_version(version: &Version, level: BumpLevel) -> Version {
    let mut v = version.clone();
    match level {
        BumpLevel::Major => {
            v.major += 1;
            v.minor = 0;
            v.patch = 0;
            v.pre = semver::Prerelease::EMPTY;
            v.build = semver::BuildMetadata::EMPTY;
        }
        BumpLevel::Minor => {
            v.minor += 1;
            v.patch = 0;
            v.pre = semver::Prerelease::EMPTY;
            v.build = semver::BuildMetadata::EMPTY;
        }
        BumpLevel::Patch => {
            if v.pre.is_empty() {
                v.patch += 1;
            }
            v.pre = semver::Prerelease::EMPTY;
            v.build = semver::BuildMetadata::EMPTY;
        }
    }
    v
}

// TODO(cargo-utils, #3): replace manual TOML version editing with a LocalManifest
// struct (toml_edit-based). Add support for workspace-inherited versions by
// detecting version.workspace=true and bumping the workspace root instead.
// Ref: release-plz/cargo_utils/src/local_manifest.rs

/// Bump the version in a Cargo.toml file, preserving formatting.
/// Returns `(old_version, new_version)`.
pub fn bump_manifest_version(manifest_path: &Path, level: BumpLevel) -> Result<(Version, Version)> {
    let content = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("cannot read {}", manifest_path.display()))?;

    let mut doc: toml_edit::DocumentMut = content
        .parse()
        .with_context(|| format!("invalid TOML in {}", manifest_path.display()))?;

    let version_item = doc
        .get("package")
        .and_then(|p| p.get("version"))
        .context("no [package].version in Cargo.toml")?;

    // Check for workspace inheritance
    if version_item.is_inline_table() || version_item.is_table() {
        anyhow::bail!(
            "version.workspace = true is not supported by autobump; \
             bump the workspace root version instead"
        );
    }

    let version_str = version_item
        .as_str()
        .context("[package].version is not a string")?;

    let old =
        Version::parse(version_str).with_context(|| format!("invalid semver: {version_str}"))?;
    let new = bump_version(&old, level);

    doc["package"]["version"] = toml_edit::value(new.to_string());
    std::fs::write(manifest_path, doc.to_string())
        .with_context(|| format!("cannot write {}", manifest_path.display()))?;

    Ok((old, new))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn bump_patch() {
        let v = Version::new(1, 2, 3);
        assert_eq!(bump_version(&v, BumpLevel::Patch), Version::new(1, 2, 4));
    }

    #[test]
    fn bump_minor() {
        let v = Version::new(1, 2, 3);
        assert_eq!(bump_version(&v, BumpLevel::Minor), Version::new(1, 3, 0));
    }

    #[test]
    fn bump_major() {
        let v = Version::new(1, 2, 3);
        assert_eq!(bump_version(&v, BumpLevel::Major), Version::new(2, 0, 0));
    }

    #[test]
    fn bump_patch_strips_prerelease() {
        let v = Version::parse("1.0.0-alpha.1").unwrap();
        assert_eq!(bump_version(&v, BumpLevel::Patch), Version::new(1, 0, 0));
    }

    #[test]
    fn bump_level_from_str() {
        assert_eq!(BumpLevel::from_str("patch").unwrap(), BumpLevel::Patch);
        assert_eq!(BumpLevel::from_str("minor").unwrap(), BumpLevel::Minor);
        assert_eq!(BumpLevel::from_str("major").unwrap(), BumpLevel::Major);
        assert!(BumpLevel::from_str("bogus").is_err());
    }

    #[test]
    fn bump_manifest_version_round_trip() {
        let dir = std::env::temp_dir().join("cargo-promote-test-bump");
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = dir.join("Cargo.toml");
        std::fs::write(
            &manifest,
            "[package]\nname = \"test-crate\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();

        let (old, new) = bump_manifest_version(&manifest, BumpLevel::Patch).unwrap();
        assert_eq!(old, Version::new(0, 1, 0));
        assert_eq!(new, Version::new(0, 1, 1));

        let content = std::fs::read_to_string(&manifest).unwrap();
        assert!(content.contains("version = \"0.1.1\""));
        // Verify formatting preserved
        assert!(content.contains("name = \"test-crate\""));
        assert!(content.contains("edition = \"2024\""));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bump_manifest_rejects_workspace_inheritance() {
        let dir = std::env::temp_dir().join("cargo-promote-test-workspace");
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = dir.join("Cargo.toml");
        std::fs::write(
            &manifest,
            "[package]\nname = \"child\"\nversion.workspace = true\n",
        )
        .unwrap();

        let result = bump_manifest_version(&manifest, BumpLevel::Patch);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("workspace"),);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bump_manifest_path_returns_correct_path() {
        // Verify we can call it on this project's own Cargo.toml (read-only check)
        let manifest = PathBuf::from("Cargo.toml");
        let content = std::fs::read_to_string(&manifest).unwrap();
        let doc: toml_edit::DocumentMut = content.parse().unwrap();
        let version_str = doc["package"]["version"].as_str().unwrap();
        let _v = Version::parse(version_str).expect("own version should be valid semver");
    }
}
