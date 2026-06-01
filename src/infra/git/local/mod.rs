use crate::domain::PromoteError;
use crate::domain::traits::{BranchMerger, RemotePusher, Tagger};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Adapter: local git operations via the git CLI.
pub struct LocalGit {
    pub repo_root: PathBuf,
}

impl LocalGit {
    pub fn new(repo_root: PathBuf) -> Self {
        Self { repo_root }
    }

    fn path(&self) -> &Path {
        &self.repo_root
    }

    /// Check if the working tree is dirty.
    pub fn is_dirty(&self) -> Result<bool, PromoteError> {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(self.path())
            .output()
            .map_err(|e| PromoteError::Other(e.into()))?;
        Ok(!output.stdout.is_empty())
    }

    /// Get the current branch name.
    pub fn current_branch(&self) -> Result<String, PromoteError> {
        let output = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(self.path())
            .output()
            .map_err(|e| PromoteError::Other(e.into()))?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Stage files for commit.
    pub fn stage(&self, files: &[&str]) -> Result<(), PromoteError> {
        let mut cmd = Command::new("git");
        cmd.arg("add").current_dir(self.path());
        for f in files {
            cmd.arg(f);
        }
        let status = cmd.status().map_err(|e| PromoteError::Other(e.into()))?;
        if !status.success() {
            return Err(PromoteError::Other(anyhow::anyhow!(
                "git add failed for: {}",
                files.join(", ")
            )));
        }
        Ok(())
    }

    /// Create a commit with the given message.
    pub fn commit(&self, message: &str) -> Result<(), PromoteError> {
        let status = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(self.path())
            .status()
            .map_err(|e| PromoteError::Other(e.into()))?;
        if !status.success() {
            return Err(PromoteError::Other(anyhow::anyhow!("git commit failed")));
        }
        Ok(())
    }

    /// Push the current HEAD to origin.
    pub fn push_head(&self) -> Result<(), PromoteError> {
        let status = Command::new("git")
            .args(["push", "origin", "HEAD"])
            .current_dir(self.path())
            .status()
            .map_err(|e| PromoteError::Other(e.into()))?;
        if !status.success() {
            return Err(PromoteError::Other(anyhow::anyhow!("git push HEAD failed")));
        }
        Ok(())
    }
}

impl BranchMerger for LocalGit {
    fn fast_forward(&self, source: &str, target: &str) -> Result<(), PromoteError> {
        let _fetch = Command::new("git")
            .args(["fetch", "origin"])
            .current_dir(self.path())
            .output();

        let checkout = Command::new("git")
            .args(["checkout", target])
            .current_dir(self.path())
            .status()
            .map_err(|e| PromoteError::Other(e.into()))?;
        if !checkout.success() {
            return Err(PromoteError::Other(anyhow::anyhow!(
                "failed to checkout branch '{target}'"
            )));
        }

        let status = Command::new("git")
            .args(["merge", "--ff-only", source])
            .current_dir(self.path())
            .status()
            .map_err(|e| PromoteError::Other(e.into()))?;
        if !status.success() {
            return Err(PromoteError::Other(anyhow::anyhow!(
                "fast-forward merge from '{source}' to '{target}' failed"
            )));
        }
        Ok(())
    }
}

impl RemotePusher for LocalGit {
    fn push_branch(&self, branch: &str) -> Result<(), PromoteError> {
        let status = Command::new("git")
            .args(["push", "origin", branch])
            .current_dir(self.path())
            .status()
            .map_err(|e| PromoteError::Other(e.into()))?;
        if !status.success() {
            return Err(PromoteError::Other(anyhow::anyhow!(
                "failed to push branch '{branch}'"
            )));
        }
        Ok(())
    }

    fn push_tag(&self, tag: &str) -> Result<(), PromoteError> {
        let status = Command::new("git")
            .args(["push", "origin", tag])
            .current_dir(self.path())
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

impl Tagger for LocalGit {
    fn create_tag(&self, name: &str, message: &str) -> Result<(), PromoteError> {
        let status = Command::new("git")
            .args(["tag", "-a", name, "-m", message])
            .current_dir(self.path())
            .status()
            .map_err(|e| PromoteError::Other(e.into()))?;
        if !status.success() {
            return Err(PromoteError::Other(anyhow::anyhow!(
                "git tag '{name}' failed"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_git_has_repo_root_field() {
        let git = LocalGit::new(PathBuf::from("/tmp/test"));
        assert_eq!(git.repo_root, PathBuf::from("/tmp/test"));
    }
}
