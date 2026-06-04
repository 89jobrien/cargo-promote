use crate::domain::PromoteError;
use crate::domain::traits::{BranchMerger, GitCommitter, RemotePusher, Tagger};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Adapter: local git operations via the git CLI.
// qual:allow(srp) reason: "single-field struct implementing multiple git trait facets by design"
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

    /// Run a git command and return an error with `err_msg` on failure.
    fn run(&self, args: &[&str], err_msg: &str) -> Result<(), PromoteError> {
        let status = Command::new("git")
            .args(args)
            .current_dir(self.path())
            .status()
            .map_err(|e| PromoteError::Other(e.into()))?;
        if !status.success() {
            return Err(PromoteError::Other(anyhow::anyhow!("{err_msg}")));
        }
        Ok(())
    }

    /// Stage files for commit.
    pub fn stage(&self, files: &[&str]) -> Result<(), PromoteError> {
        let mut args = vec!["add"];
        args.extend_from_slice(files);
        self.run(&args, &format!("git add failed for: {}", files.join(", ")))
    }

    /// Create a commit with the given message.
    pub fn commit(&self, message: &str) -> Result<(), PromoteError> {
        self.run(&["commit", "-m", message], "git commit failed")
    }

    /// Push the current HEAD to origin.
    pub fn push_head(&self) -> Result<(), PromoteError> {
        self.run(&["push", "origin", "HEAD"], "git push HEAD failed")
    }
}

impl GitCommitter for LocalGit {
    fn stage(&self, files: &[&str]) -> Result<(), PromoteError> {
        LocalGit::stage(self, files)
    }

    fn commit(&self, message: &str) -> Result<(), PromoteError> {
        LocalGit::commit(self, message)
    }

    fn push_head(&self) -> Result<(), PromoteError> {
        LocalGit::push_head(self)
    }
}

impl BranchMerger for LocalGit {
    fn fast_forward(&self, source: &str, target: &str) -> Result<(), PromoteError> {
        // Best-effort fetch; ignore errors (may be offline).
        let _ = Command::new("git")
            .args(["fetch", "origin"])
            .current_dir(self.path())
            .output();

        self.run(
            &["checkout", target],
            &format!("failed to checkout branch '{target}'"),
        )?;
        self.run(
            &["merge", "--ff-only", source],
            &format!("fast-forward merge from '{source}' to '{target}' failed"),
        )
    }
}

impl RemotePusher for LocalGit {
    fn push_branch(&self, branch: &str) -> Result<(), PromoteError> {
        self.run(
            &["push", "origin", branch],
            &format!("failed to push branch '{branch}'"),
        )
    }

    fn push_tag(&self, tag: &str) -> Result<(), PromoteError> {
        self.run(
            &["push", "origin", tag],
            &format!("failed to push tag '{tag}'"),
        )
    }
}

impl Tagger for LocalGit {
    fn create_tag(&self, name: &str, message: &str) -> Result<(), PromoteError> {
        self.run(
            &["tag", "-a", name, "-m", message],
            &format!("git tag '{name}' failed"),
        )
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
