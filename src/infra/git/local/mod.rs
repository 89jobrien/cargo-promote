use crate::domain::PromoteError;
use crate::domain::traits::{
    BranchMerger, CiBranchPromoter, FfStatus, GitCommitter, RemotePusher, Tagger,
};
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
        self.run_output(
            &["rev-parse", "--short", &r],
            &format!("failed to resolve {r}"),
        )
    }

    fn ff_status(&self, remote: &str, from: &str, to: &str) -> Result<FfStatus, PromoteError> {
        let from_ref = format!("{remote}/{from}");
        let to_ref = format!("{remote}/{to}");

        // Use full SHAs for merge-base comparison.
        let from_sha = self.run_output(
            &["rev-parse", &from_ref],
            &format!("failed to resolve {from_ref}"),
        )?;
        let to_sha = self.run_output(
            &["rev-parse", &to_ref],
            &format!("failed to resolve {to_ref}"),
        )?;

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
    use crate::domain::traits::CiBranchPromoter;
    use tempfile::TempDir;

    #[test]
    fn local_git_has_repo_root_field() {
        let git = LocalGit::new(PathBuf::from("/tmp/test"));
        assert_eq!(git.repo_root, PathBuf::from("/tmp/test"));
    }

    // --- test helpers: real subprocess git repos ---

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .expect("failed to run git");
        assert!(status.success(), "git {args:?} failed in {dir:?}");
    }

    fn git_out(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("failed to run git");
        assert!(output.status.success(), "git {args:?} failed in {dir:?}");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Initialize a git repo in `dir` with a configured identity, ready to commit.
    fn init_repo(dir: &Path) {
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "Test User"]);
        git(dir, &["config", "commit.gpgsign", "false"]);
    }

    /// Create a commit with a unique file so each commit has a distinct SHA.
    fn commit_file(dir: &Path, name: &str, contents: &str) {
        std::fs::write(dir.join(name), contents).expect("failed to write file");
        git(dir, &["add", name]);
        git(dir, &["commit", "-q", "-m", &format!("add {name}")]);
    }

    /// Create a bare "remote" repo and a working clone with `origin` pointing at it.
    /// Returns (bare_dir, work_dir).
    fn init_remote_and_clone() -> (TempDir, TempDir) {
        let bare = TempDir::new().unwrap();
        git(bare.path(), &["init", "-q", "--bare", "-b", "main"]);

        let seed = TempDir::new().unwrap();
        init_repo(seed.path());
        commit_file(seed.path(), "seed.txt", "seed");
        git(
            seed.path(),
            &["remote", "add", "origin", bare.path().to_str().unwrap()],
        );
        git(seed.path(), &["push", "-q", "origin", "main"]);

        let work = TempDir::new().unwrap();
        git(
            work.path(),
            &[
                "clone",
                "-q",
                bare.path().to_str().unwrap(),
                work.path().to_str().unwrap(),
            ],
        );
        // git clone into an existing (empty) dir needs `.` target from within it in
        // some git versions; fall back if the above didn't populate the dir.
        if !work.path().join(".git").exists() {
            git(
                work.path(),
                &["clone", "-q", bare.path().to_str().unwrap(), "."],
            );
        }
        git(work.path(), &["config", "user.email", "test@example.com"]);
        git(work.path(), &["config", "user.name", "Test User"]);
        git(work.path(), &["config", "commit.gpgsign", "false"]);

        (bare, work)
    }

    #[test]
    fn ff_status_returns_in_sync_when_branches_point_at_same_commit() {
        let (_bare, work) = init_remote_and_clone();
        // develop == main on the remote right after clone.
        git(work.path(), &["branch", "develop", "origin/main"]);
        git(work.path(), &["push", "-q", "origin", "develop"]);
        git(work.path(), &["fetch", "-q", "origin"]);

        let repo = LocalGit::new(work.path().to_path_buf());
        let status = repo.ff_status("origin", "develop", "main").unwrap();
        assert_eq!(status, FfStatus::InSync);
    }

    #[test]
    fn ff_status_returns_promotable_when_target_is_ancestor_of_source() {
        let (_bare, work) = init_remote_and_clone();
        git(work.path(), &["branch", "develop", "origin/main"]);
        git(work.path(), &["checkout", "-q", "develop"]);
        commit_file(work.path(), "feature.txt", "feature work");
        git(work.path(), &["push", "-q", "origin", "develop"]);
        git(work.path(), &["push", "-q", "origin", "main"]);
        git(work.path(), &["fetch", "-q", "origin"]);

        let repo = LocalGit::new(work.path().to_path_buf());
        let status = repo.ff_status("origin", "develop", "main").unwrap();
        assert_eq!(status, FfStatus::Promotable);
    }

    #[test]
    fn ff_status_returns_diverged_when_both_branches_have_unique_commits() {
        let (_bare, work) = init_remote_and_clone();
        git(work.path(), &["branch", "develop", "origin/main"]);

        git(work.path(), &["checkout", "-q", "develop"]);
        commit_file(work.path(), "develop-only.txt", "develop work");
        git(work.path(), &["push", "-q", "origin", "develop"]);

        git(work.path(), &["checkout", "-q", "main"]);
        commit_file(work.path(), "main-only.txt", "main work");
        git(work.path(), &["push", "-q", "origin", "main"]);

        git(work.path(), &["fetch", "-q", "origin"]);

        let repo = LocalGit::new(work.path().to_path_buf());
        let status = repo.ff_status("origin", "develop", "main").unwrap();
        assert_eq!(status, FfStatus::Diverged);
    }

    #[test]
    fn checkout_and_ff_merge_performs_merge_and_leaves_tree_at_merged_commit() {
        let (_bare, work) = init_remote_and_clone();
        git(work.path(), &["branch", "develop", "origin/main"]);
        git(work.path(), &["checkout", "-q", "develop"]);
        commit_file(work.path(), "feature.txt", "feature work");
        git(work.path(), &["push", "-q", "origin", "develop"]);
        git(work.path(), &["checkout", "-q", "main"]);
        git(work.path(), &["fetch", "-q", "origin"]);

        let expected_sha = git_out(work.path(), &["rev-parse", "origin/develop"]);

        let repo = LocalGit::new(work.path().to_path_buf());
        repo.checkout_and_ff_merge("origin", "develop", "main")
            .unwrap();

        let head_sha = git_out(work.path(), &["rev-parse", "HEAD"]);
        assert_eq!(head_sha, expected_sha);
        assert!(work.path().join("feature.txt").exists());

        let current_branch = git_out(work.path(), &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert_eq!(current_branch, "main");
    }

    #[test]
    fn remote_sha_resolves_remote_branch_ref_to_short_sha() {
        let (_bare, work) = init_remote_and_clone();
        git(work.path(), &["fetch", "-q", "origin"]);

        let expected = git_out(work.path(), &["rev-parse", "--short", "origin/main"]);

        let repo = LocalGit::new(work.path().to_path_buf());
        let sha = repo.remote_sha("origin", "main").unwrap();
        assert_eq!(sha, expected);
        assert!(!sha.is_empty());
    }

    #[test]
    fn fetch_smoke_test_against_local_remote_does_not_error() {
        let (bare, work) = init_remote_and_clone();

        // Add a new commit directly to the bare remote via a second clone, so
        // there's something new for `fetch` to pull down.
        let other = TempDir::new().unwrap();
        git(
            other.path(),
            &[
                "clone",
                "-q",
                bare.path().to_str().unwrap(),
                other.path().to_str().unwrap(),
            ],
        );
        if !other.path().join(".git").exists() {
            git(
                other.path(),
                &["clone", "-q", bare.path().to_str().unwrap(), "."],
            );
        }
        git(other.path(), &["config", "user.email", "test@example.com"]);
        git(other.path(), &["config", "user.name", "Test User"]);
        git(other.path(), &["config", "commit.gpgsign", "false"]);
        commit_file(other.path(), "elsewhere.txt", "from another clone");
        git(other.path(), &["push", "-q", "origin", "main"]);

        let repo = LocalGit::new(work.path().to_path_buf());
        repo.fetch("origin", &["main"]).unwrap();

        let sha = git_out(work.path(), &["rev-parse", "origin/main"]);
        assert!(!sha.is_empty());
    }

    #[test]
    fn push_branch_to_lands_branch_on_bare_remote() {
        let (bare, work) = init_remote_and_clone();
        git(work.path(), &["checkout", "-q", "-b", "feature-x"]);
        commit_file(work.path(), "feature-x.txt", "feature x work");

        let repo = LocalGit::new(work.path().to_path_buf());
        repo.push_branch_to("origin", "feature-x").unwrap();

        let branches = git_out(bare.path(), &["branch", "--list", "feature-x"]);
        assert!(branches.contains("feature-x"));
    }

    #[test]
    fn push_all_tags_to_lands_tags_on_bare_remote() {
        let (bare, work) = init_remote_and_clone();
        git(work.path(), &["tag", "-a", "v0.1.0", "-m", "release"]);

        let repo = LocalGit::new(work.path().to_path_buf());
        repo.push_all_tags_to("origin").unwrap();

        let tags = git_out(bare.path(), &["tag", "--list", "v0.1.0"]);
        assert_eq!(tags, "v0.1.0");
    }
}
