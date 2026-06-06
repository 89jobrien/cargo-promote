use anyhow::Context;
use std::fs;
use std::path::PathBuf;

use crate::domain::deferral::{Deferral, DeferralStatus};
use crate::domain::traits::DeferralStore;
use crate::domain::PromoteError;

const DEFERRALS_DIR: &str = ".promote/deferrals";

/// Filesystem-backed deferral store. Persists tickets as TOML files
/// under `<repo_root>/.promote/deferrals/`.
pub struct FsDeferralStore {
    repo_root: PathBuf,
}

impl FsDeferralStore {
    pub fn new(repo_root: PathBuf) -> Self {
        Self { repo_root }
    }

    fn deferrals_dir(&self) -> PathBuf {
        self.repo_root.join(DEFERRALS_DIR)
    }

    fn ticket_path(&self, ticket: &str) -> PathBuf {
        self.deferrals_dir().join(format!("{ticket}.toml"))
    }
}

impl DeferralStore for FsDeferralStore {
    fn save(&self, deferral: &Deferral) -> Result<(), PromoteError> {
        let dir = self.deferrals_dir();
        fs::create_dir_all(&dir)
            .with_context(|| format!("cannot create {}", dir.display()))
            .map_err(PromoteError::Other)?;

        let path = self.ticket_path(&deferral.ticket);
        let content = toml::to_string_pretty(deferral)
            .context("cannot serialize deferral")
            .map_err(PromoteError::Other)?;
        fs::write(&path, content)
            .with_context(|| format!("cannot write {}", path.display()))
            .map_err(PromoteError::Other)?;
        Ok(())
    }

    fn load(&self, ticket: &str) -> Result<Deferral, PromoteError> {
        let path = self.ticket_path(ticket);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))
            .map_err(PromoteError::Other)?;
        toml::from_str(&content)
            .context("cannot parse deferral")
            .map_err(PromoteError::Other)
    }

    fn list_all(&self) -> Result<Vec<Deferral>, PromoteError> {
        let dir = self.deferrals_dir();
        if !dir.exists() {
            return Ok(vec![]);
        }

        let mut deferrals = Vec::new();
        for entry in fs::read_dir(&dir).map_err(|e| PromoteError::Other(e.into()))? {
            let entry = entry.map_err(|e| PromoteError::Other(e.into()))?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "toml") {
                let content = fs::read_to_string(&path)
                    .with_context(|| format!("cannot read {}", path.display()))
                    .map_err(PromoteError::Other)?;
                let d: Deferral = toml::from_str(&content)
                    .with_context(|| format!("cannot parse {}", path.display()))
                    .map_err(PromoteError::Other)?;
                deferrals.push(d);
            }
        }
        deferrals.sort_by(|a, b| a.deferred_at.cmp(&b.deferred_at));
        Ok(deferrals)
    }

    fn list_pending(&self) -> Result<Vec<Deferral>, PromoteError> {
        Ok(self
            .list_all()?
            .into_iter()
            .filter(|d| d.status == DeferralStatus::Pending)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::deferral::{DeferralKind, DeferralStatus};
    use std::path::Path;
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
            pr_number: None,
        }
    }

    fn store(dir: &Path) -> FsDeferralStore {
        FsDeferralStore::new(dir.to_path_buf())
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = TempDir::new().unwrap();
        let s = store(dir.path());
        let d = sample_deferral();
        s.save(&d).unwrap();

        let loaded = s.load(&d.ticket).unwrap();
        assert_eq!(loaded.ticket, d.ticket);
        assert_eq!(loaded.crate_name, "mycrate");
        assert_eq!(loaded.status, DeferralStatus::Pending);
    }

    #[test]
    fn pr_number_round_trips() {
        let dir = TempDir::new().unwrap();
        let s = store(dir.path());
        let mut d = sample_deferral();
        d.pr_number = Some(42);
        s.save(&d).unwrap();

        let loaded = s.load(&d.ticket).unwrap();
        assert_eq!(loaded.pr_number, Some(42));
    }

    #[test]
    fn pr_number_defaults_to_none_when_missing() {
        let dir = TempDir::new().unwrap();
        let deferrals_dir = dir.path().join(".promote/deferrals");
        fs::create_dir_all(&deferrals_dir).unwrap();

        let content = r#"
ticket = "d-20260531.185400-noprt"
crate_name = "noprt"
version = "0.1.0"
from_stage = "cratebox"
to_stage = "crates-io"
status = "pending"
deferred_at = "20260531.185400"
source_hash = "sha256:abc123"
"#;
        fs::write(
            deferrals_dir.join("d-20260531.185400-noprt.toml"),
            content,
        )
        .unwrap();

        let s = store(dir.path());
        let d = s.load("d-20260531.185400-noprt").unwrap();
        assert_eq!(d.pr_number, None);
    }

    #[test]
    fn list_all_returns_sorted_deferrals() {
        let dir = TempDir::new().unwrap();
        let s = store(dir.path());

        let mut d1 = sample_deferral();
        d1.ticket = "d-20260531.100000-alpha".to_string();
        d1.deferred_at = "20260531.100000".to_string();
        s.save(&d1).unwrap();

        let mut d2 = sample_deferral();
        d2.ticket = "d-20260531.110000-beta".to_string();
        d2.deferred_at = "20260531.110000".to_string();
        s.save(&d2).unwrap();

        let all = s.list_all().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].ticket, "d-20260531.100000-alpha");
        assert_eq!(all[1].ticket, "d-20260531.110000-beta");
    }

    #[test]
    fn list_all_empty_dir_returns_empty() {
        let dir = TempDir::new().unwrap();
        let s = store(dir.path());
        let all = s.list_all().unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn list_pending_filters_confirmed() {
        let dir = TempDir::new().unwrap();
        let s = store(dir.path());

        let mut d1 = sample_deferral();
        d1.ticket = "d-20260531.100000-alpha".to_string();
        s.save(&d1).unwrap();

        let mut d2 = sample_deferral();
        d2.ticket = "d-20260531.110000-beta".to_string();
        d2.status = DeferralStatus::Confirmed;
        s.save(&d2).unwrap();

        let pending = s.list_pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].ticket, "d-20260531.100000-alpha");
    }

    #[test]
    fn command_field_round_trips() {
        let dir = TempDir::new().unwrap();
        let s = store(dir.path());
        let mut d = sample_deferral();
        d.command = vec![
            "curl".to_string(),
            "-X".to_string(),
            "POST".to_string(),
            "https://ci.example.com/hook".to_string(),
        ];
        s.save(&d).unwrap();

        let loaded = s.load(&d.ticket).unwrap();
        assert_eq!(loaded.command.len(), 4);
        assert_eq!(loaded.command[0], "curl");
    }

    #[test]
    fn kind_defaults_to_registry_when_missing() {
        let dir = TempDir::new().unwrap();
        let deferrals_dir = dir.path().join(".promote/deferrals");
        fs::create_dir_all(&deferrals_dir).unwrap();

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
        fs::write(
            deferrals_dir.join("d-20260531.185400-legacy.toml"),
            content,
        )
        .unwrap();

        let s = store(dir.path());
        let d = s.load("d-20260531.185400-legacy").unwrap();
        assert_eq!(
            d.kind,
            DeferralKind::Registry,
            "missing kind field should default to Registry"
        );
    }

    #[test]
    fn branch_kind_round_trips() {
        let dir = TempDir::new().unwrap();
        let s = store(dir.path());
        let mut d = sample_deferral();
        d.kind = DeferralKind::Branch;
        s.save(&d).unwrap();

        let loaded = s.load(&d.ticket).unwrap();
        assert_eq!(loaded.kind, DeferralKind::Branch);
    }

    #[test]
    fn confirm_via_store_is_idempotent_on_disk() {
        let dir = TempDir::new().unwrap();
        let s = store(dir.path());
        let d = sample_deferral();
        s.save(&d).unwrap();

        // First confirm succeeds.
        let loaded = s.load(&d.ticket).unwrap();
        let confirmed = loaded.into_confirmed("ok").unwrap();
        s.save(&confirmed).unwrap();

        // Second confirm on same ticket errors (already confirmed).
        let loaded = s.load(&d.ticket).unwrap();
        assert!(loaded.into_confirmed("again").is_err());

        // Status on disk is still Confirmed (not corrupted).
        let loaded = s.load(&d.ticket).unwrap();
        assert_eq!(loaded.status, DeferralStatus::Confirmed);
        assert_eq!(loaded.reason, "ok");
    }
}
