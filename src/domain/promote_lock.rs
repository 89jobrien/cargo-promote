use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

const PROMOTE_LOCK_FILENAME: &str = "promote.lock";

/// The promote.lock file — prevents code from changing mid-pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromoteLock {
    /// Version that was bumped.
    pub version: String,
    /// SHA-256 of concatenated publishable source files.
    pub source_hash: String,
    /// ISO 8601 timestamp when bump occurred.
    pub bumped_at: String,
    /// First branch in the pipeline where bump happened.
    pub entered_pipeline: String,
}

impl PromoteLock {
    /// Compute the SHA-256 hash of all publishable source files.
    ///
    /// Includes:
    ///   - src/**/*.rs
    ///   - Cargo.toml
    ///   - Cargo.lock
    ///
    /// Excludes:
    ///   - promote.lock, promote.toml, tests/, docs/, .github/, .ctx/, .gitignore
    pub fn compute_source_hash(repo_root: &Path) -> Result<String> {
        let mut hasher = Sha256::new();
        let mut files = Vec::new();

        // Collect Cargo.toml and Cargo.lock
        let cargo_toml = repo_root.join("Cargo.toml");
        let cargo_lock = repo_root.join("Cargo.lock");

        if cargo_toml.exists() {
            files.push(cargo_toml);
        }
        if cargo_lock.exists() {
            files.push(cargo_lock);
        }

        // Collect src/**/*.rs
        if let Ok(entries) = fs::read_dir(repo_root.join("src")) {
            Self::collect_rust_files(entries, &mut files)?;
        }

        // Sort for deterministic ordering
        files.sort();

        // Hash each file's content in order
        for file_path in files {
            let content = fs::read(&file_path)
                .with_context(|| format!("cannot read {}", file_path.display()))?;
            hasher.update(&content);
        }

        let hash = hasher.finalize();
        Ok(format!("sha256:{:x}", hash))
    }

    fn collect_rust_files(dir: fs::ReadDir, files: &mut Vec<PathBuf>) -> Result<()> {
        for entry in dir {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            } else if path.is_dir() {
                Self::collect_rust_files(fs::read_dir(&path)?, files)?;
            }
        }
        Ok(())
    }

    /// Write promote.lock to the repository.
    pub fn write(&self, repo_root: &Path) -> Result<()> {
        let lock_path = repo_root.join(PROMOTE_LOCK_FILENAME);
        let yaml = serde_yaml::to_string(self).context("cannot serialize promote.lock")?;
        fs::write(&lock_path, yaml)
            .with_context(|| format!("cannot write {}", lock_path.display()))?;
        Ok(())
    }

    /// Read promote.lock from the repository.
    pub fn read(repo_root: &Path) -> Result<Self> {
        let lock_path = repo_root.join(PROMOTE_LOCK_FILENAME);
        let content = fs::read_to_string(&lock_path)
            .with_context(|| format!("cannot read {}", lock_path.display()))?;
        serde_yaml::from_str(&content).context("cannot parse promote.lock")
    }

    /// Verify that the source hash matches the current state.
    pub fn verify_hash(&self, repo_root: &Path) -> Result<()> {
        let current_hash = Self::compute_source_hash(repo_root)?;
        if current_hash != self.source_hash {
            anyhow::bail!(
                "source hash mismatch: lock says '{}' but current is '{}'",
                self.source_hash,
                current_hash
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn compute_source_hash_includes_cargo_files() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Create Cargo.toml
        fs::write(root.join("Cargo.toml"), "name = \"test\"").unwrap();

        // Create src directory with a Rust file
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "fn foo() {}").unwrap();

        let hash = PromoteLock::compute_source_hash(root).unwrap();
        assert!(hash.starts_with("sha256:"));
    }

    #[test]
    fn source_hash_is_deterministic() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        fs::write(root.join("Cargo.toml"), "name = \"test\"").unwrap();
        fs::create_dir(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "fn foo() {}").unwrap();

        let hash1 = PromoteLock::compute_source_hash(root).unwrap();
        let hash2 = PromoteLock::compute_source_hash(root).unwrap();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn write_and_read_lock() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        let lock = PromoteLock {
            version: "0.2.0".to_string(),
            source_hash: "sha256:abc123".to_string(),
            bumped_at: "20260531::180000".to_string(),
            entered_pipeline: "develop".to_string(),
        };

        lock.write(root).unwrap();
        let loaded = PromoteLock::read(root).unwrap();

        assert_eq!(loaded.version, lock.version);
        assert_eq!(loaded.source_hash, lock.source_hash);
    }
}
