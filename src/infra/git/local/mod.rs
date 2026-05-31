use crate::domain::PromoteError;
use std::path::Path;
use std::process::Command;

/// Adapter: local git operations (dirty check, tagging, current branch).
pub struct LocalGit;

impl LocalGit {
    /// Check if the working tree is dirty.
    pub fn is_dirty(path: &Path) -> Result<bool, PromoteError> {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(path)
            .output()
            .map_err(|e| PromoteError::Other(e.into()))?;

        Ok(!output.stdout.is_empty())
    }

    /// Get the current branch name.
    pub fn current_branch(path: &Path) -> Result<String, PromoteError> {
        let output = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(path)
            .output()
            .map_err(|e| PromoteError::Other(e.into()))?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Create a git tag.
    pub fn tag(path: &Path, tag_name: &str, message: &str) -> Result<(), PromoteError> {
        let status = Command::new("git")
            .args(["tag", "-a", tag_name, "-m", message])
            .current_dir(path)
            .status()
            .map_err(|e| PromoteError::Other(e.into()))?;

        if !status.success() {
            return Err(PromoteError::Other(anyhow::anyhow!(
                "git tag '{tag_name}' failed"
            )));
        }
        Ok(())
    }
}
