use crate::domain::PromoteError;
use crate::domain::traits::{BranchMerger, RemotePusher, Tagger};
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

    /// Perform a fast-forward merge.
    pub fn merge_ff(path: &Path, source: &str, target: &str) -> Result<(), PromoteError> {
        // Fetch to ensure we have latest refs
        let _fetch = Command::new("git")
            .args(["fetch", "origin"])
            .current_dir(path)
            .output();

        // Checkout target branch
        let checkout_status = Command::new("git")
            .args(["checkout", target])
            .current_dir(path)
            .status()
            .map_err(|e| PromoteError::Other(e.into()))?;

        if !checkout_status.success() {
            return Err(PromoteError::Other(anyhow::anyhow!(
                "failed to checkout branch '{target}'"
            )));
        }

        // Merge with --ff-only
        let status = Command::new("git")
            .args(["merge", "--ff-only", source])
            .current_dir(path)
            .status()
            .map_err(|e| PromoteError::Other(e.into()))?;

        if !status.success() {
            return Err(PromoteError::Other(anyhow::anyhow!(
                "fast-forward merge from '{source}' to '{target}' failed"
            )));
        }
        Ok(())
    }

    /// Push a branch to the remote.
    pub fn push_branch(path: &Path, branch: &str) -> Result<(), PromoteError> {
        let status = Command::new("git")
            .args(["push", "origin", branch])
            .current_dir(path)
            .status()
            .map_err(|e| PromoteError::Other(e.into()))?;

        if !status.success() {
            return Err(PromoteError::Other(anyhow::anyhow!(
                "failed to push branch '{branch}'"
            )));
        }
        Ok(())
    }

    /// Push a tag to the remote.
    pub fn push_tag(path: &Path, tag: &str) -> Result<(), PromoteError> {
        let status = Command::new("git")
            .args(["push", "origin", tag])
            .current_dir(path)
            .status()
            .map_err(|e| PromoteError::Other(e.into()))?;

        if !status.success() {
            return Err(PromoteError::Other(anyhow::anyhow!(
                "failed to push tag '{tag}'"
            )));
        }
        Ok(())
    }
}

// Adapters for branch-based pipeline

/// Git CLI implementation of BranchMerger.
pub struct GitCliMerger {
    pub repo_root: std::path::PathBuf,
}

impl BranchMerger for GitCliMerger {
    fn fast_forward(&self, source: &str, target: &str) -> Result<(), PromoteError> {
        LocalGit::merge_ff(&self.repo_root, source, target)
    }
}

/// Git CLI implementation of RemotePusher.
pub struct GitCliPusher {
    pub repo_root: std::path::PathBuf,
}

impl RemotePusher for GitCliPusher {
    fn push_branch(&self, branch: &str) -> Result<(), PromoteError> {
        LocalGit::push_branch(&self.repo_root, branch)
    }

    fn push_tag(&self, tag: &str) -> Result<(), PromoteError> {
        LocalGit::push_tag(&self.repo_root, tag)
    }
}

/// Git CLI implementation of Tagger.
pub struct GitCliTagger {
    pub repo_root: std::path::PathBuf,
}

impl Tagger for GitCliTagger {
    fn create_tag(&self, name: &str, message: &str) -> Result<(), PromoteError> {
        LocalGit::tag(&self.repo_root, name, message)
    }
}
