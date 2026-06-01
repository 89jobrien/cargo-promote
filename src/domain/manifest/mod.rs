use super::CrateRef;
use super::local_manifest::LocalManifest;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Resolve a CrateRef from a manifest path and optional package name.
pub fn resolve_crate(path: Option<&Path>, package: Option<&str>) -> Result<CrateRef> {
    let manifest_path = manifest_for(path);
    let manifest = LocalManifest::try_new(&manifest_path)?;

    let (name, version) = if let Some(pkg_name) = package {
        let ver = manifest.package_version().unwrap_or("0.0.0").to_string();
        (pkg_name.to_string(), ver)
    } else {
        let n = manifest
            .package_name()
            .context("missing package.name")?
            .to_string();
        let v = manifest.package_version().unwrap_or("0.0.0").to_string();
        (n, v)
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
    let manifest = LocalManifest::try_new(&manifest_path)?;

    if let Some(members) = manifest.workspace_members() {
        let base = path.unwrap_or(Path::new("."));
        let infos = members
            .iter()
            .filter_map(|m| read_member_info(base, m))
            .collect();
        Ok(ManifestDescription::Workspace(infos))
    } else if manifest.package_name().is_some() {
        let name = manifest.package_name().unwrap_or("?").to_string();
        let version = manifest.package_version().unwrap_or("?").to_string();
        Ok(ManifestDescription::Single(MemberInfo { name, version }))
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

fn read_member_info(base: &Path, member: &str) -> Option<MemberInfo> {
    let manifest_path = base.join(member).join("Cargo.toml");
    let manifest = LocalManifest::try_new(&manifest_path).ok()?;
    let name = manifest.package_name().unwrap_or("?").to_string();
    let version = manifest.package_version().unwrap_or("?").to_string();
    Some(MemberInfo { name, version })
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
}
