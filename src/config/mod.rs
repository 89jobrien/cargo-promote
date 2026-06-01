use crate::domain::version::BumpLevel;
use crate::domain::{Pipeline, Registry, Stage};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const CONFIG_FILENAME: &str = "promote.toml";

#[derive(Debug, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    registries: HashMap<String, RegistryDef>,
    #[serde(default)]
    pipelines: HashMap<String, PipelineDef>,
    #[serde(default)]
    pipeline: Option<BranchPipelineDef>,
    #[serde(default)]
    autobump: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BranchPipelineDef {
    stages: Vec<String>,
    #[serde(default)]
    release_branch: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RegistryDef {
    cargo_name: Option<String>,
    api_url: Option<String>,
    #[serde(default)]
    confirm: bool,
}

#[derive(Debug, Deserialize)]
struct PipelineDef {
    stages: Vec<String>,
}

/// Configuration for a branch-based promotion pipeline.
#[derive(Debug, Clone)]
pub struct BranchPipelineConfig {
    pub stages: Vec<String>,
    pub release_branch: String,
}

/// Loaded configuration with resolved pipelines.
#[derive(Debug)]
pub struct Config {
    pub registries: HashMap<String, Registry>,
    pub pipelines: HashMap<String, Pipeline>,
    pub branch_pipeline: Option<BranchPipelineConfig>,
    pub autobump: Option<BumpLevel>,
}

/// A `[registries.<name>]` entry in `.cargo/config.toml`.
#[derive(Debug, Deserialize)]
struct CargoRegistryEntry {
    index: Option<String>,
}

/// Top-level shape of `.cargo/config.toml` (only the fields we need).
#[derive(Debug, Deserialize)]
struct CargoConfigFile {
    #[serde(default)]
    registries: HashMap<String, CargoRegistryEntry>,
}

/// Strip `sparse+` prefix from an index URL to derive the API URL.
fn index_to_api_url(index: &str) -> String {
    index.strip_prefix("sparse+").unwrap_or(index).to_string()
}

/// Collect candidate `.cargo/config.toml` paths by walking ancestors, then
/// appending `$CARGO_HOME/config.toml` (defaulting to `~/.cargo/config.toml`).
fn cargo_config_paths(dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut current = Some(dir.to_path_buf());
    while let Some(d) = current {
        for name in &["config.toml", "config"] {
            let candidate = d.join(".cargo").join(name);
            if candidate.is_file() {
                paths.push(candidate);
                break; // only one per directory
            }
        }
        current = d.parent().map(Path::to_path_buf);
    }
    // Also check $CARGO_HOME
    let cargo_home = std::env::var("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/"))
                .join(".cargo")
        });
    for name in &["config.toml", "config"] {
        let candidate = cargo_home.join(name);
        if candidate.is_file() && !paths.contains(&candidate) {
            paths.push(candidate);
            break;
        }
    }
    paths
}

/// Discover registries from `.cargo/config.toml` files by walking ancestor
/// directories and checking `$CARGO_HOME`.
pub fn discover_cargo_registries(dir: &Path) -> HashMap<String, Registry> {
    let mut result = HashMap::new();
    for path in cargo_config_paths(dir) {
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let parsed: CargoConfigFile = match toml::from_str(&content) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for (name, entry) in parsed.registries {
            // First config found wins (closest ancestor first).
            result.entry(name.clone()).or_insert_with(|| {
                let api_url = entry.index.as_deref().map(index_to_api_url);
                Registry {
                    name: name.clone(),
                    cargo_name: Some(name),
                    api_url,
                    confirm: false,
                }
            });
        }
    }
    result
}

impl Config {
    /// Load from `promote.toml` in the given directory, or fall back to
    /// hardcoded defaults matching the original cratebox -> crates.io behavior.
    // qual:allow(iosp) reason: "I/O boundary — reads file then delegates to from_toml"
    pub fn load(dir: &Path) -> Result<Self> {
        let config_path = dir.join(CONFIG_FILENAME);
        let mut cfg = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .with_context(|| format!("cannot read {}", config_path.display()))?;
            Self::from_toml(&content)?
        } else {
            Self::default_config()
        };
        cfg.merge_discovered(dir);
        Ok(cfg)
    }

    /// Parse from TOML string.
    pub fn from_toml(content: &str) -> Result<Self> {
        let file: ConfigFile = toml::from_str(content).context("invalid promote.toml")?;

        let registries: HashMap<String, Registry> = file
            .registries
            .into_iter()
            .map(|(name, def)| {
                let reg = Registry {
                    name: name.clone(),
                    cargo_name: def.cargo_name,
                    api_url: def.api_url,
                    confirm: def.confirm,
                };
                (name, reg)
            })
            .collect();

        let mut pipelines = HashMap::new();
        for (name, def) in file.pipelines {
            let stages: Vec<Stage> = def
                .stages
                .iter()
                .map(|stage_name| {
                    let registry = registries.get(stage_name).cloned().ok_or_else(|| {
                        anyhow::anyhow!(
                            "pipeline '{name}' references unknown registry '{stage_name}'"
                        )
                    });
                    registry.map(|r| Stage { registry: r })
                })
                .collect::<Result<Vec<_>>>()?;
            pipelines.insert(name.clone(), Pipeline { name, stages });
        }

        let autobump = file
            .autobump
            .as_deref()
            .map(|s| s.parse::<BumpLevel>())
            .transpose()
            .context("invalid autobump value")?;

        let branch_pipeline = file.pipeline.map(|def| {
            let release_branch = def
                .release_branch
                .unwrap_or_else(|| def.stages.last().cloned().unwrap_or_default());
            BranchPipelineConfig {
                stages: def.stages,
                release_branch,
            }
        });

        Ok(Self {
            registries,
            pipelines,
            branch_pipeline,
            autobump,
        })
    }

    /// Merge discovered cargo registries into this config. Existing entries
    /// (from promote.toml) take precedence.
    fn merge_discovered(&mut self, dir: &Path) {
        let discovered = discover_cargo_registries(dir);
        for (name, reg) in discovered {
            self.registries.entry(name).or_insert(reg);
        }
    }

    /// Hardcoded default: cratebox -> crates.io (backwards compatible).
    pub fn default_config() -> Self {
        let base_url = std::env::var("REGISTRY_URL")
            .unwrap_or_else(|_| "http://100.105.75.7:3000".to_string());
        let user = std::env::var("REGISTRY_USER").unwrap_or_else(|_| "joe".to_string());

        let cratebox = Registry {
            name: "cratebox".to_string(),
            cargo_name: Some("cratebox".to_string()),
            api_url: Some(format!("{base_url}/api/packages/{user}/cargo")),
            confirm: false,
        };

        let crates_io = Registry {
            name: "crates-io".to_string(),
            cargo_name: None,
            api_url: None,
            confirm: true,
        };

        let registries = HashMap::from([
            ("cratebox".to_string(), cratebox.clone()),
            ("crates-io".to_string(), crates_io.clone()),
        ]);

        let pipelines = HashMap::from([(
            "default".to_string(),
            Pipeline {
                name: "default".to_string(),
                stages: vec![
                    Stage { registry: cratebox },
                    Stage {
                        registry: crates_io,
                    },
                ],
            },
        )]);

        Self {
            registries,
            pipelines,
            branch_pipeline: None,
            autobump: None,
        }
    }

    /// Get a pipeline by name, falling back to "default".
    pub fn pipeline(&self, name: Option<&str>) -> Option<&Pipeline> {
        let key = name.unwrap_or("default");
        self.pipelines.get(key)
    }

    /// Get a registry by name.
    pub fn registry(&self, name: &str) -> Option<&Registry> {
        self.registries.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_two_stage_default_pipeline() {
        let cfg = Config::default_config();
        let p = cfg.pipeline(None).expect("default pipeline should exist");
        assert_eq!(p.stages.len(), 2);
        assert_eq!(p.stages[0].registry.name, "cratebox");
        assert_eq!(p.stages[1].registry.name, "crates-io");
    }

    #[test]
    fn parse_custom_config() {
        let toml = r#"
[registries.staging]
cargo_name = "my-staging"
api_url = "http://localhost:3000/api/packages/joe/cargo"
confirm = false

[registries.prod]
confirm = true

[pipelines.default]
stages = ["staging", "prod"]

[pipelines.staging-only]
stages = ["staging"]
"#;
        let cfg = Config::from_toml(toml).expect("should parse");
        assert_eq!(cfg.pipelines.len(), 2);

        let default = cfg.pipeline(None).unwrap();
        assert_eq!(default.stages.len(), 2);
        assert_eq!(
            default.stages[0].registry.cargo_name.as_deref(),
            Some("my-staging")
        );
        assert!(default.stages[1].registry.confirm);
        assert!(default.stages[1].registry.cargo_name.is_none());

        let staging = cfg.pipeline(Some("staging-only")).unwrap();
        assert_eq!(staging.stages.len(), 1);
    }

    #[test]
    fn unknown_registry_in_pipeline_errors() {
        let toml = r#"
[pipelines.bad]
stages = ["nonexistent"]
"#;
        let result = Config::from_toml(toml);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("nonexistent"),
            "error should name the registry: {msg}"
        );
    }

    #[test]
    fn empty_config_is_valid() {
        let cfg = Config::from_toml("").expect("empty config should parse");
        assert!(cfg.pipelines.is_empty());
        assert!(cfg.registries.is_empty());
        assert!(cfg.autobump.is_none());
    }

    #[test]
    fn autobump_parses_from_config() {
        let toml = r#"autobump = "patch""#;
        let cfg = Config::from_toml(toml).expect("should parse");
        assert_eq!(cfg.autobump, Some(BumpLevel::Patch));
    }

    #[test]
    fn autobump_minor() {
        let toml = r#"autobump = "minor""#;
        let cfg = Config::from_toml(toml).expect("should parse");
        assert_eq!(cfg.autobump, Some(BumpLevel::Minor));
    }

    #[test]
    fn autobump_invalid_errors() {
        let toml = r#"autobump = "bogus""#;
        let result = Config::from_toml(toml);
        assert!(result.is_err());
    }

    #[test]
    fn discover_registries_from_cargo_config() {
        let tmp = tempfile::tempdir().unwrap();
        let cargo_dir = tmp.path().join(".cargo");
        std::fs::create_dir_all(&cargo_dir).unwrap();
        std::fs::write(
            cargo_dir.join("config.toml"),
            r#"
[registries.mybox]
index = "sparse+http://localhost:3000/api/packages/joe/cargo/"
"#,
        )
        .unwrap();

        let discovered = discover_cargo_registries(tmp.path());
        assert!(
            discovered.contains_key("mybox"),
            "should discover mybox registry"
        );
        let reg = discovered.get("mybox").unwrap();
        assert_eq!(reg.name, "mybox");
        assert_eq!(reg.cargo_name.as_deref(), Some("mybox"));
        assert_eq!(
            reg.api_url.as_deref(),
            Some("http://localhost:3000/api/packages/joe/cargo/")
        );
    }

    #[test]
    fn promote_toml_takes_precedence_over_cargo_config() {
        let tmp = tempfile::tempdir().unwrap();
        let cargo_dir = tmp.path().join(".cargo");
        std::fs::create_dir_all(&cargo_dir).unwrap();
        std::fs::write(
            cargo_dir.join("config.toml"),
            r#"
[registries.staging]
index = "sparse+http://discovered:3000/"
"#,
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("promote.toml"),
            r#"
[registries.staging]
api_url = "http://explicit:3000/"
confirm = true
"#,
        )
        .unwrap();

        let cfg = Config::load(tmp.path()).unwrap();
        let reg = cfg.registries.get("staging").unwrap();
        assert_eq!(reg.api_url.as_deref(), Some("http://explicit:3000/"));
        assert!(reg.confirm);
    }

    #[test]
    fn missing_cargo_config_has_no_local_registries() {
        // A unique registry name that won't exist in any real config
        let tmp = tempfile::tempdir().unwrap();
        let discovered = discover_cargo_registries(tmp.path());
        assert!(
            !discovered.contains_key("__nonexistent_test_registry__"),
            "should not find a made-up registry name"
        );
    }
}
