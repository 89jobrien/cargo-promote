use crate::domain::version::BumpLevel;
use crate::domain::{Pipeline, Registry, Stage};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

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

impl Config {
    /// Load from `promote.toml` in the given directory, or fall back to
    /// hardcoded defaults matching the original cratebox -> crates.io behavior.
    // qual:allow(iosp) reason: "I/O boundary — reads file then delegates to from_toml"
    pub fn load(dir: &Path) -> Result<Self> {
        let config_path = dir.join(CONFIG_FILENAME);
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .with_context(|| format!("cannot read {}", config_path.display()))?;
            Self::from_toml(&content)
        } else {
            Ok(Self::default_config())
        }
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
}
