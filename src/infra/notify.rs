use crate::domain::PromoteError;
use crate::domain::deferral::Deferral;
use crate::domain::traits::Notifier;

/// Adapter: fire a shell command on deferral (best-effort).
pub struct SpawnNotifier {
    pub command: Vec<String>,
}

impl Notifier for SpawnNotifier {
    fn on_deferred(&self, _deferral: &Deferral) -> Result<(), PromoteError> {
        if self.command.is_empty() {
            return Ok(());
        }
        match std::process::Command::new(&self.command[0])
            .args(&self.command[1..])
            .spawn()
        {
            Ok(_child) => {
                eprintln!("=> notification command spawned");
            }
            Err(e) => {
                eprintln!("=> notification command failed to start: {e}");
            }
        }
        Ok(())
    }
}

/// Adapter: no-op notifier for tests and library usage.
pub struct NoopNotifier;

impl Notifier for NoopNotifier {
    fn on_deferred(&self, _deferral: &Deferral) -> Result<(), PromoteError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::deferral::{DeferralKind, DeferralStatus};

    fn sample_deferral() -> Deferral {
        Deferral {
            ticket: "d-test".to_string(),
            crate_name: "mycrate".to_string(),
            version: "0.1.0".to_string(),
            from_stage: "staging".to_string(),
            to_stage: "production".to_string(),
            status: DeferralStatus::Pending,
            kind: DeferralKind::Registry,
            deferred_at: "20260531.120000".to_string(),
            source_hash: "sha256:abc".to_string(),
            command: vec![],
            reason: String::new(),
        }
    }

    #[test]
    fn noop_notifier_returns_ok() {
        let n = NoopNotifier;
        let result = n.on_deferred(&sample_deferral());
        assert!(result.is_ok());
    }

    #[test]
    fn spawn_notifier_with_no_command_returns_ok() {
        let n = SpawnNotifier { command: vec![] };
        let result = n.on_deferred(&sample_deferral());
        assert!(result.is_ok());
    }
}
