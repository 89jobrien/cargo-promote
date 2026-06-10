use crate::domain::PromoteError;
use crate::domain::traits::{BranchMerger, CiBranchPromoter, FfStatus, GitCommitter, RemotePusher, Tagger};
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

    /// Run a git command, capture stdout, return trimmed output or error.
    fn run_output(&self, args: &[&str], err_msg: &str) -> Result<String, PromoteError> {
        let output = Command::new("git")
            .args(args)
            .current_dir(self.path())
            .output()
            .map_err(|e| PromoteError::Other(e.into()))?;
        if !output.status.success() {
            return Err(PromoteError::Other(anyhow::anyhow!("{err_msg}")));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Run a git command and return an error with `err_msg` on failure.
    // TODO: capture stderr in the error message — currently only returns the static err_msg,
    // making it hard to diagnose why a git operation failed (e.g., merge conflict details)
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

        // TODO: restore original branch after merge — currently leaves the working tree
        // on the target branch, which is surprising if the user was on a different branch
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

impl CiBranchPromoter for LocalGit {
    fn fetch(&self, remote: &str, branches: &[&str]) -> Result<(), PromoteError> {
        let mut args = vec!["fetch", remote];
        args.extend_from_slice(branches);
        self.run(&args, &format!("git fetch {remote} failed"))
    }

    fn remote_sha(&self, remote: &str, branch: &str) -> Result<String, PromoteError> {
        let r = format!("{remote}/{branch}");
        self.run_output(&["rev-parse", "--short", &r], &format!("failed to resolve {r}"))
    }

    fn ff_status(&self, remote: &str, from: &str, to: &str) -> Result<FfStatus, PromoteError> {
        let from_ref = format!("{remote}/{from}");
        let to_ref = format!("{remote}/{to}");

        // Use full SHAs for merge-base comparison.
        let from_sha =
            self.run_output(&["rev-parse", &from_ref], &format!("failed to resolve {from_ref}"))?;
        let to_sha =
            self.run_output(&["rev-parse", &to_ref], &format!("failed to resolve {to_ref}"))?;

        if from_sha == to_sha {
            return Ok(FfStatus::InSync);
        }

        let base_sha =
            self.run_output(&["merge-base", &from_sha, &to_sha], "git merge-base failed")?;

        if base_sha == to_sha {
            Ok(FfStatus::Promotable)
        } else {
            Ok(FfStatus::Diverged)
        }
    }

    fn checkout_and_ff_merge(
        &self,
        remote: &str,
        from: &str,
        to: &str,
    ) -> Result<(), PromoteError> {
        self.run(&["checkout", to], &format!("failed to checkout '{to}'"))?;
        let from_ref = format!("{remote}/{from}");
        self.run(
            &["merge", "--ff-only", &from_ref],
            &format!("fast-forward merge {from_ref} -> {to} failed"),
        )
    }

    fn push_branch_to(&self, remote: &str, branch: &str) -> Result<(), PromoteError> {
        self.run(
            &["push", remote, branch],
            &format!("failed to push '{branch}' to '{remote}'"),
        )
    }

    fn push_all_tags_to(&self, remote: &str) -> Result<(), PromoteError> {
        self.run(
            &["push", remote, "--tags"],
            &format!("failed to push tags to '{remote}'"),
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
