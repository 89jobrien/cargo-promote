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

/// Scan a directory tree for all Cargo.toml files and build a dep graph.
pub fn scan_workspace_tree(root: &Path, skip: &[&str]) -> Result<Vec<CrateNode>> {
    let mut nodes = Vec::new();
    let mut known_names: HashSet<String> = HashSet::new();

    // First pass: collect all crate names
    for entry in std::fs::read_dir(root).context("cannot read root dir")? {
        let entry = entry?;
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let repo_name = dir.file_name().unwrap().to_string_lossy().to_string();
        if skip.iter().any(|s| *s == repo_name) {
            continue;
        }
        // Skip hidden dirs (.maestro, .claude, etc.)
        if repo_name.starts_with('.') {
            continue;
        }

        let manifest = dir.join("Cargo.toml");
        if !manifest.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&manifest).unwrap_or_default();
        let doc: toml::Value = match content.parse() {
            Ok(d) => d,
            Err(_) => continue,
        };

        if let Some(members) = doc
            .get("workspace")
            .and_then(|w| w.get("members"))
            .and_then(|m| m.as_array())
        {
            for m in members.iter().filter_map(|v| v.as_str()) {
                let mpath = dir.join(m).join("Cargo.toml");
                if let Some(name) = read_crate_name(&mpath) {
                    known_names.insert(name);
                }
            }
        } else if let Some(name) = read_crate_name(&manifest) {
            known_names.insert(name);
        }
    }

    // Second pass: build nodes with internal deps
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let repo_name = dir.file_name().unwrap().to_string_lossy().to_string();
        if skip.iter().any(|s| *s == repo_name) || repo_name.starts_with('.') {
            continue;
        }

        let manifest = dir.join("Cargo.toml");
        if !manifest.exists() {
            continue;
        }

        let content = std::fs::read_to_string(&manifest).unwrap_or_default();
        let doc: toml::Value = match content.parse() {
            Ok(d) => d,
            Err(_) => continue,
        };

        if let Some(members) = doc
            .get("workspace")
            .and_then(|w| w.get("members"))
            .and_then(|m| m.as_array())
        {
            for m in members.iter().filter_map(|v| v.as_str()) {
                let mpath = dir.join(m).join("Cargo.toml");
                if let Some(node) = build_node(&mpath, &known_names) {
                    nodes.push(node);
                }
            }
        } else if let Some(node) = build_node(&manifest, &known_names) {
            nodes.push(node);
        }
    }

    Ok(nodes)
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
    let content = std::fs::read_to_string(manifest).ok()?;
    let doc: toml::Value = content.parse().ok()?;

    let pkg = doc.get("package")?;
    let name = pkg.get("name").and_then(|n| n.as_str())?;
    let version = pkg
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0");

    // xtask crates are never publishable (workspace automation)
    let is_xtask = name == "xtask" || name.ends_with("-xtask");

    let unpublishable = is_xtask
        || pkg
            .get("publish")
            .and_then(|p| p.as_bool())
            .map(|b| !b)
            .unwrap_or(false);

    let mut internal_deps = Vec::new();
    let mut path_only_deps = Vec::new();

    // Check [dependencies] and [build-dependencies] only.
    // dev-dependencies don't block publishing.
    for section in ["dependencies", "build-dependencies"] {
        if let Some(deps) = doc.get(section).and_then(|d| d.as_table()) {
            for (dep_name, dep_val) in deps {
                if !known_names.contains(dep_name) {
                    continue;
                }
                internal_deps.push(dep_name.clone());

                // Check if path-only (no version field)
                if let Some(tbl) = dep_val.as_table() {
                    if tbl.contains_key("path") && !tbl.contains_key("version") {
                        path_only_deps.push(dep_name.clone());
                    }
                }
            }
        }
    }

    Some(CrateNode {
        name: name.to_string(),
        version: version.to_string(),
        manifest_path: manifest.to_path_buf(),
        internal_deps,
        unpublishable,
        path_only_deps,
    })
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
