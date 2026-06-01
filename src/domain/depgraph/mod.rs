use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// A crate node in the dependency graph.
#[derive(Debug, Clone)]
pub struct CrateNode {
    pub name: String,
    pub version: String,
    pub manifest_path: PathBuf,
    /// Internal deps (names of other crates in the graph).
    pub internal_deps: Vec<String>,
    /// Whether this crate has `publish = false`.
    pub unpublishable: bool,
    /// Deps that use path-only (no version) — blocks publishing.
    pub path_only_deps: Vec<String>,
}

// TODO(cargo-utils, #5): replace manual workspace member resolution with
// cargo_metadata::MetadataCommand to handle glob patterns in
// [workspace.members] and path canonicalization correctly.
// Ref: release-plz/cargo_utils/src/workspace_members.rs

/// Scan a directory tree for all Cargo.toml files and build a dep graph.
pub fn scan_workspace_tree(root: &Path, skip: &[&str]) -> Result<Vec<CrateNode>> {
    let dirs = scannable_dirs(root, skip)?;
    let all_manifests = resolve_all_manifests(&dirs);
    let known_names = collect_crate_names(&all_manifests);
    let nodes = all_manifests
        .iter()
        .filter_map(|m| build_node(m, &known_names))
        .collect();
    Ok(nodes)
}

/// Return repo directories under `root` that are not skipped/hidden and contain a Cargo.toml.
fn scannable_dirs(root: &Path, skip: &[&str]) -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(root).context("cannot read root dir")? {
        let dir = entry?.path();
        if !dir.is_dir() {
            continue;
        }
        let name = dir
            .file_name()
            .context("dir has no name")?
            .to_string_lossy();
        if name.starts_with('.') || skip.iter().any(|s| *s == &*name) {
            continue;
        }
        if dir.join("Cargo.toml").exists() {
            dirs.push(dir);
        }
    }
    Ok(dirs)
}

/// Expand workspace members into individual manifest paths.
fn resolve_all_manifests(dirs: &[PathBuf]) -> Vec<PathBuf> {
    dirs.iter()
        .flat_map(|dir| {
            let manifest = dir.join("Cargo.toml");
            parse_manifest(&manifest)
                .map(|doc| manifest_paths(&doc, dir))
                .unwrap_or_default()
        })
        .collect()
}

fn collect_crate_names(manifests: &[PathBuf]) -> HashSet<String> {
    manifests
        .iter()
        .filter_map(|m| read_crate_name(m))
        .collect()
}

/// Return manifest paths for workspace members, or the root manifest itself.
fn manifest_paths(doc: &toml::Value, dir: &Path) -> Vec<PathBuf> {
    if let Some(members) = doc
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
    {
        members
            .iter()
            .filter_map(|v| v.as_str())
            .map(|m| dir.join(m).join("Cargo.toml"))
            .collect()
    } else {
        vec![dir.join("Cargo.toml")]
    }
}

fn parse_manifest(path: &Path) -> Option<toml::Value> {
    let content = std::fs::read_to_string(path).ok()?;
    content.parse().ok()
}

/// Topological sort of crate nodes. Returns names in publish order.
pub fn topo_sort(nodes: &[CrateNode]) -> Result<Vec<String>> {
    let name_to_idx: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.name.as_str(), i))
        .collect();

    // Build in-degree map
    let mut in_degree: Vec<usize> = vec![0; nodes.len()];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];

    for (i, node) in nodes.iter().enumerate() {
        for dep in &node.internal_deps {
            if let Some(&dep_idx) = name_to_idx.get(dep.as_str()) {
                in_degree[i] += 1;
                dependents[dep_idx].push(i);
            }
        }
    }

    // Kahn's algorithm
    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, &deg) in in_degree.iter().enumerate() {
        if deg == 0 {
            queue.push_back(i);
        }
    }

    let mut order = Vec::new();
    while let Some(idx) = queue.pop_front() {
        order.push(nodes[idx].name.clone());
        for &dep_idx in &dependents[idx] {
            in_degree[dep_idx] -= 1;
            if in_degree[dep_idx] == 0 {
                queue.push_back(dep_idx);
            }
        }
    }

    if order.len() != nodes.len() {
        let missing: Vec<_> = nodes
            .iter()
            .filter(|n| !order.contains(&n.name))
            .map(|n| n.name.as_str())
            .collect();
        anyhow::bail!(
            "circular dependency detected involving: {}",
            missing.join(", ")
        );
    }

    Ok(order)
}

fn read_crate_name(manifest: &Path) -> Option<String> {
    let content = std::fs::read_to_string(manifest).ok()?;
    let doc: toml::Value = content.parse().ok()?;
    doc.get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
}

fn build_node(manifest: &Path, known_names: &HashSet<String>) -> Option<CrateNode> {
    let doc = parse_manifest(manifest)?;

    let pkg = doc.get("package")?;
    let name = pkg.get("name").and_then(|n| n.as_str())?;
    let version = pkg
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0");

    let is_xtask = name == "xtask" || name.ends_with("-xtask");
    let unpublishable = is_xtask
        || pkg
            .get("publish")
            .and_then(|p| p.as_bool())
            .is_some_and(|b| !b);

    let (internal_deps, path_only_deps) = collect_internal_deps(&doc, known_names);

    Some(CrateNode {
        name: name.to_string(),
        version: version.to_string(),
        manifest_path: manifest.to_path_buf(),
        internal_deps,
        unpublishable,
        path_only_deps,
    })
}

/// Extract internal deps and path-only deps from [dependencies] and [build-dependencies].
fn collect_internal_deps(
    doc: &toml::Value,
    known_names: &HashSet<String>,
) -> (Vec<String>, Vec<String>) {
    let mut internal = Vec::new();
    let mut path_only = Vec::new();

    for section in ["dependencies", "build-dependencies"] {
        let Some(deps) = doc.get(section).and_then(|d| d.as_table()) else {
            continue;
        };
        for (dep_name, dep_val) in deps {
            if !known_names.contains(dep_name) {
                continue;
            }
            internal.push(dep_name.clone());
            let is_path_only = dep_val
                .as_table()
                .is_some_and(|t| t.contains_key("path") && !t.contains_key("version"));
            if is_path_only {
                path_only.push(dep_name.clone());
            }
        }
    }

    (internal, path_only)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topo_sort_empty() {
        let order = topo_sort(&[]).unwrap();
        assert!(order.is_empty());
    }

    #[test]
    fn topo_sort_linear() {
        let nodes = vec![
            CrateNode {
                name: "a".into(),
                version: "0.1.0".into(),
                manifest_path: PathBuf::from("a/Cargo.toml"),
                internal_deps: vec!["b".into()],
                unpublishable: false,
                path_only_deps: vec![],
            },
            CrateNode {
                name: "b".into(),
                version: "0.1.0".into(),
                manifest_path: PathBuf::from("b/Cargo.toml"),
                internal_deps: vec![],
                unpublishable: false,
                path_only_deps: vec![],
            },
        ];
        let order = topo_sort(&nodes).unwrap();
        assert_eq!(order, vec!["b", "a"]);
    }
}
