use serde::{Deserialize, Serialize};

use super::PromoteError;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u64>,
}

impl Deferral {
    /// Generate a ticket ID from the current timestamp and crate name.
    pub fn ticket_id(crate_name: &str) -> String {
        let now = chrono::Local::now();
        format!("d-{}-{}", now.format("%Y%m%d.%H%M%S"), crate_name)
    }

    /// Transition a pending deferral to confirmed. Returns an error
    /// if the deferral is not in `Pending` status.
    pub fn into_confirmed(mut self, reason: &str) -> Result<Self, PromoteError> {
        if self.status != DeferralStatus::Pending {
            return Err(PromoteError::Other(anyhow::anyhow!(
                "deferral '{}' is already {:?}",
                self.ticket,
                self.status,
            )));
        }
        self.status = DeferralStatus::Confirmed;
        self.reason = reason.to_string();
        Ok(self)
    }

    /// Transition a pending deferral to rejected. Returns an error
    /// if the deferral is not in `Pending` status.
    pub fn into_rejected(mut self, reason: &str) -> Result<Self, PromoteError> {
        if self.status != DeferralStatus::Pending {
            return Err(PromoteError::Other(anyhow::anyhow!(
                "deferral '{}' is already {:?}",
                self.ticket,
                self.status,
            )));
        }
        self.status = DeferralStatus::Rejected;
        self.reason = reason.to_string();
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn ticket_id_contains_crate_name() {
        let id = Deferral::ticket_id("mycrate");
        assert!(id.starts_with("d-"));
        assert!(id.ends_with("-mycrate"));
    }

    #[test]
    fn into_confirmed_transitions_pending() {
        let d = sample_deferral();
        let confirmed = d.into_confirmed("CI passed").unwrap();
        assert_eq!(confirmed.status, DeferralStatus::Confirmed);
        assert_eq!(confirmed.reason, "CI passed");
    }

    #[test]
    fn into_rejected_transitions_pending() {
        let d = sample_deferral();
        let rejected = d.into_rejected("tests failed").unwrap();
        assert_eq!(rejected.status, DeferralStatus::Rejected);
        assert_eq!(rejected.reason, "tests failed");
    }

    #[test]
    fn into_confirmed_errors_when_already_confirmed() {
        let mut d = sample_deferral();
        d.status = DeferralStatus::Confirmed;
        assert!(d.into_confirmed("").is_err());
    }

    #[test]
    fn into_rejected_errors_when_already_rejected() {
        let mut d = sample_deferral();
        d.status = DeferralStatus::Rejected;
        assert!(d.into_rejected("").is_err());
    }

    #[test]
    fn into_confirmed_errors_when_rejected() {
        let mut d = sample_deferral();
        d.status = DeferralStatus::Rejected;
        assert!(d.into_confirmed("nope").is_err());
    }

    #[test]
    fn into_rejected_errors_when_confirmed() {
        let mut d = sample_deferral();
        d.status = DeferralStatus::Confirmed;
        assert!(d.into_rejected("nope").is_err());
    }
}
