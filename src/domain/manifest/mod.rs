use super::CrateRef;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

// TODO(cargo-utils): replace manual TOML parsing with LocalManifest struct
// for consistent format-preserving manifest access across resolve + bump.
// Ref: release-plz/cargo_utils/src/local_manifest.rs

/// Resolve a CrateRef from a manifest path and optional package name.
pub fn resolve_crate(path: Option<&Path>, package: Option<&str>) -> Result<CrateRef> {
    let manifest_path = manifest_for(path);
    let doc = read_manifest(&manifest_path)?;

    let (name, version) = if let Some(pkg_name) = package {
        let ver = extract_version(&doc).unwrap_or_else(|| "0.0.0".to_string());
        (pkg_name.to_string(), ver)
    } else {
        let pkg = doc.get("package").context("no [package] in Cargo.toml")?;
        let n = pkg
            .get("name")
            .and_then(|n| n.as_str())
            .context("missing package.name")?;
        let v = pkg
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0");
        (n.to_string(), v.to_string())
    };

    Ok(CrateRef {
        name,
        version,
        manifest_path,
    })
}

/// Info about a single crate member in a workspace.
pub struct MemberInfo {
    pub name: String,
    pub version: String,
}

/// Describe the workspace or single-crate at the given path.
pub fn describe_manifest(path: Option<&Path>) -> Result<ManifestDescription> {
    let manifest_path = manifest_for(path);
    let doc = read_manifest(&manifest_path)?;

    if let Some(members) = workspace_members(&doc) {
        let base = path.unwrap_or(Path::new("."));
        let infos = members
            .iter()
            .filter_map(|m| m.as_str())
            .filter_map(|m| read_member_info(base, m))
            .collect();
        Ok(ManifestDescription::Workspace(infos))
    } else if let Some(pkg) = doc.get("package") {
        let name = pkg.get("name").and_then(|n| n.as_str()).unwrap_or("?");
        let version = pkg.get("version").and_then(|v| v.as_str()).unwrap_or("?");
        Ok(ManifestDescription::Single(MemberInfo {
            name: name.to_string(),
            version: version.to_string(),
        }))
    } else {
        Ok(ManifestDescription::Workspace(vec![]))
    }
}

pub enum ManifestDescription {
    Single(MemberInfo),
    Workspace(Vec<MemberInfo>),
}

fn manifest_for(path: Option<&Path>) -> PathBuf {
    path.map(|p| p.join("Cargo.toml"))
        .unwrap_or_else(|| PathBuf::from("Cargo.toml"))
}

fn read_manifest(manifest_path: &Path) -> Result<toml::Value> {
    let content = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("cannot read {}", manifest_path.display()))?;
    content.parse().context("invalid Cargo.toml")
}

fn extract_version(doc: &toml::Value) -> Option<String> {
    doc.get("package")
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn workspace_members(doc: &toml::Value) -> Option<&Vec<toml::Value>> {
    doc.get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
}

fn read_member_info(base: &Path, member: &str) -> Option<MemberInfo> {
    let manifest = base.join(member).join("Cargo.toml");
    let content = std::fs::read_to_string(&manifest).ok()?;
    let doc: toml::Value = content.parse().ok()?;
    let name = doc
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("?");
    let version = doc
        .get("package")
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    Some(MemberInfo {
        name: name.to_string(),
        version: version.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_crate_reads_own_manifest() {
        let krate = resolve_crate(None, None).expect("should read own Cargo.toml");
        assert_eq!(krate.name, "cargo-promote");
        assert!(!krate.version.is_empty());
    }

    #[test]
    fn resolve_crate_with_package_override() {
        let krate =
            resolve_crate(None, Some("custom-pkg")).expect("should succeed with package name");
        assert_eq!(krate.name, "custom-pkg");
    }

    #[test]
    fn describe_manifest_single_crate() {
        let desc = describe_manifest(None).expect("should describe own manifest");
        match desc {
            ManifestDescription::Single(info) => {
                assert_eq!(info.name, "cargo-promote");
            }
            ManifestDescription::Workspace(_) => {
                panic!("expected Single, got Workspace")
            }
        }
    }

    #[test]
    fn manifest_for_defaults_to_cwd() {
        let p = manifest_for(None);
        assert_eq!(p, PathBuf::from("Cargo.toml"));
    }

    #[test]
    fn extract_version_from_own_manifest() {
        let doc = read_manifest(Path::new("Cargo.toml")).expect("should read");
        let version = extract_version(&doc);
        assert!(version.is_some());
        assert!(!version.unwrap().is_empty());
    }
}
