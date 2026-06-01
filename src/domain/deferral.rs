use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const DEFERRALS_DIR: &str = ".promote/deferrals";

/// Status of a deferred promotion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeferralStatus {
    Pending,
    Confirmed,
    Rejected,
}


/// What kind of promotion is being deferred.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeferralKind {
    /// Publish to a registry stage.
    #[default]
    Registry,
    /// Merge a branch forward in the branch pipeline.
    Branch,
}

/// A deferred promotion ticket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deferral {
    pub ticket: String,
    pub crate_name: String,
    pub version: String,
    pub from_stage: String,
    pub to_stage: String,
    pub status: DeferralStatus,
    #[serde(default)]
    pub kind: DeferralKind,
    pub deferred_at: String,
    pub source_hash: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

impl Deferral {
    /// Generate a ticket ID from the current timestamp and crate name.
    pub fn ticket_id(crate_name: &str) -> String {
        let now = chrono::Local::now();
        format!("d-{}-{}", now.format("%Y%m%d.%H%M%S"), crate_name)
    }

    /// Return the directory where deferral files are stored.
    pub fn deferrals_dir(repo_root: &Path) -> PathBuf {
        repo_root.join(DEFERRALS_DIR)
    }

    /// Write this deferral to its ticket file.
    pub fn write(&self, repo_root: &Path) -> Result<()> {
        let dir = Self::deferrals_dir(repo_root);
        fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;

        let path = dir.join(format!("{}.toml", self.ticket));
        let content = toml::to_string_pretty(self).context("cannot serialize deferral")?;
        fs::write(&path, content).with_context(|| format!("cannot write {}", path.display()))?;
        Ok(())
    }

    /// Read a deferral by ticket ID.
    pub fn read(repo_root: &Path, ticket: &str) -> Result<Self> {
        let path = Self::deferrals_dir(repo_root).join(format!("{}.toml", ticket));
        let content =
            fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
        toml::from_str(&content).context("cannot parse deferral")
    }

    /// List all deferrals in the repo.
    // qual:allow(iosp) reason: "I/O boundary — reads dir then parses files"
    pub fn list(repo_root: &Path) -> Result<Vec<Self>> {
        let dir = Self::deferrals_dir(repo_root);
        if !dir.exists() {
            return Ok(vec![]);
        }

        let mut deferrals = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "toml") {
                let content = fs::read_to_string(&path)
                    .with_context(|| format!("cannot read {}", path.display()))?;
                let d: Deferral = toml::from_str(&content)
                    .with_context(|| format!("cannot parse {}", path.display()))?;
                deferrals.push(d);
            }
        }
        deferrals.sort_by(|a, b| a.deferred_at.cmp(&b.deferred_at));
        Ok(deferrals)
    }

    /// List only pending deferrals.
    pub fn list_pending(repo_root: &Path) -> Result<Vec<Self>> {
        Ok(Self::list(repo_root)?
            .into_iter()
            .filter(|d| d.status == DeferralStatus::Pending)
            .collect())
    }

    /// Transition a pending deferral to the given status.
    // qual:allow(iosp) reason: "I/O boundary — read, validate, write"
    fn update_status(
        repo_root: &Path,
        ticket: &str,
        new_status: DeferralStatus,
        reason: &str,
    ) -> Result<Self> {
        let mut d = Self::read(repo_root, ticket)?;
        if d.status != DeferralStatus::Pending {
            anyhow::bail!("deferral '{}' is already {:?}", ticket, d.status);
        }
        d.status = new_status;
        d.reason = reason.to_string();
        d.write(repo_root)?;
        Ok(d)
    }

    /// Confirm a deferral with an optional reason.
    pub fn confirm(repo_root: &Path, ticket: &str, reason: &str) -> Result<Self> {
        Self::update_status(repo_root, ticket, DeferralStatus::Confirmed, reason)
    }

    /// Reject a deferral with an optional reason.
    pub fn reject(repo_root: &Path, ticket: &str, reason: &str) -> Result<Self> {
        Self::update_status(repo_root, ticket, DeferralStatus::Rejected, reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_deferral() -> Deferral {
        Deferral {
            ticket: "d-20260531.185400-mycrate".to_string(),
            crate_name: "mycrate".to_string(),
            version: "0.2.1".to_string(),
            from_stage: "cratebox".to_string(),
            to_stage: "crates-io".to_string(),
            status: DeferralStatus::Pending,
            kind: DeferralKind::Registry,
            deferred_at: "20260531.185400".to_string(),
            source_hash: "sha256:abc123".to_string(),
            command: vec![],
            reason: String::new(),
        }
    }

    #[test]
    fn write_and_read_round_trip() {
        let dir = TempDir::new().unwrap();
        let d = sample_deferral();
        d.write(dir.path()).unwrap();

        let loaded = Deferral::read(dir.path(), &d.ticket).unwrap();
        assert_eq!(loaded.ticket, d.ticket);
        assert_eq!(loaded.crate_name, "mycrate");
        assert_eq!(loaded.status, DeferralStatus::Pending);
    }

    #[test]
    fn list_returns_all_deferrals() {
        let dir = TempDir::new().unwrap();
        let mut d1 = sample_deferral();
        d1.ticket = "d-20260531.100000-alpha".to_string();
        d1.deferred_at = "20260531.100000".to_string();
        d1.write(dir.path()).unwrap();

        let mut d2 = sample_deferral();
        d2.ticket = "d-20260531.110000-beta".to_string();
        d2.deferred_at = "20260531.110000".to_string();
        d2.write(dir.path()).unwrap();

        let all = Deferral::list(dir.path()).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].ticket, "d-20260531.100000-alpha");
        assert_eq!(all[1].ticket, "d-20260531.110000-beta");
    }

    #[test]
    fn list_empty_dir_returns_empty() {
        let dir = TempDir::new().unwrap();
        let all = Deferral::list(dir.path()).unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn list_pending_filters_confirmed() {
        let dir = TempDir::new().unwrap();
        let mut d1 = sample_deferral();
        d1.ticket = "d-20260531.100000-alpha".to_string();
        d1.write(dir.path()).unwrap();

        let mut d2 = sample_deferral();
        d2.ticket = "d-20260531.110000-beta".to_string();
        d2.status = DeferralStatus::Confirmed;
        d2.write(dir.path()).unwrap();

        let pending = Deferral::list_pending(dir.path()).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].ticket, "d-20260531.100000-alpha");
    }

    #[test]
    fn confirm_updates_status() {
        let dir = TempDir::new().unwrap();
        let d = sample_deferral();
        d.write(dir.path()).unwrap();

        let confirmed = Deferral::confirm(dir.path(), &d.ticket, "CI passed").unwrap();
        assert_eq!(confirmed.status, DeferralStatus::Confirmed);
        assert_eq!(confirmed.reason, "CI passed");

        let loaded = Deferral::read(dir.path(), &d.ticket).unwrap();
        assert_eq!(loaded.status, DeferralStatus::Confirmed);
    }

    #[test]
    fn reject_updates_status() {
        let dir = TempDir::new().unwrap();
        let d = sample_deferral();
        d.write(dir.path()).unwrap();

        let rejected = Deferral::reject(dir.path(), &d.ticket, "tests failed").unwrap();
        assert_eq!(rejected.status, DeferralStatus::Rejected);
        assert_eq!(rejected.reason, "tests failed");
    }

    #[test]
    fn confirm_already_confirmed_errors() {
        let dir = TempDir::new().unwrap();
        let mut d = sample_deferral();
        d.status = DeferralStatus::Confirmed;
        d.write(dir.path()).unwrap();

        let result = Deferral::confirm(dir.path(), &d.ticket, "");
        assert!(result.is_err());
    }

    #[test]
    fn reject_already_rejected_errors() {
        let dir = TempDir::new().unwrap();
        let mut d = sample_deferral();
        d.status = DeferralStatus::Rejected;
        d.write(dir.path()).unwrap();

        let result = Deferral::reject(dir.path(), &d.ticket, "");
        assert!(result.is_err());
    }

    #[test]
    fn ticket_id_contains_crate_name() {
        let id = Deferral::ticket_id("mycrate");
        assert!(id.starts_with("d-"));
        assert!(id.ends_with("-mycrate"));
    }

    #[test]
    fn command_field_round_trips() {
        let dir = TempDir::new().unwrap();
        let mut d = sample_deferral();
        d.command = vec![
            "curl".to_string(),
            "-X".to_string(),
            "POST".to_string(),
            "https://ci.example.com/hook".to_string(),
        ];
        d.write(dir.path()).unwrap();

        let loaded = Deferral::read(dir.path(), &d.ticket).unwrap();
        assert_eq!(loaded.command.len(), 4);
        assert_eq!(loaded.command[0], "curl");
    }

    #[test]
    fn kind_defaults_to_registry_when_missing() {
        let dir = TempDir::new().unwrap();
        let deferrals_dir = dir.path().join(".promote/deferrals");
        fs::create_dir_all(&deferrals_dir).unwrap();

        // Write a TOML file without the `kind` field (pre-existing ticket).
        let content = r#"
ticket = "d-20260531.185400-legacy"
crate_name = "legacy"
version = "0.1.0"
from_stage = "cratebox"
to_stage = "crates-io"
status = "pending"
deferred_at = "20260531.185400"
source_hash = "sha256:abc123"
"#;
        fs::write(deferrals_dir.join("d-20260531.185400-legacy.toml"), content).unwrap();

        let d = Deferral::read(dir.path(), "d-20260531.185400-legacy").unwrap();
        assert_eq!(
            d.kind,
            DeferralKind::Registry,
            "missing kind field should default to Registry"
        );
    }

    #[test]
    fn branch_kind_round_trips() {
        let dir = TempDir::new().unwrap();
        let mut d = sample_deferral();
        d.kind = DeferralKind::Branch;
        d.write(dir.path()).unwrap();

        let loaded = Deferral::read(dir.path(), &d.ticket).unwrap();
        assert_eq!(loaded.kind, DeferralKind::Branch);
    }

    // Regression: confirm must not change status if caller aborts
    // between confirm and side-effect. This test verifies the
    // low-level Deferral::confirm still works atomically at the
    // file level.
    #[test]
    fn confirm_is_idempotent_on_disk() {
        let dir = TempDir::new().unwrap();
        let d = sample_deferral();
        d.write(dir.path()).unwrap();

        // First confirm succeeds.
        Deferral::confirm(dir.path(), &d.ticket, "ok").unwrap();

        // Second confirm on same ticket errors (already confirmed).
        let result = Deferral::confirm(dir.path(), &d.ticket, "again");
        assert!(
            result.is_err(),
            "confirming an already-confirmed ticket must error"
        );

        // Status on disk is still Confirmed (not corrupted).
        let loaded = Deferral::read(dir.path(), &d.ticket).unwrap();
        assert_eq!(loaded.status, DeferralStatus::Confirmed);
        assert_eq!(loaded.reason, "ok");
    }
}
