//! First-run onboarding state.
//!
//! Persisted in SQLite (`settings` table, key `onboarding`) so the wizard is
//! shown once and can be re-run deliberately from Settings → General.

use serde::{Deserialize, Serialize};

pub const ONBOARDING_KEY: &str = "onboarding";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Onboarding {
    /// True once the user finished (or explicitly skipped) the wizard.
    pub completed: bool,
    pub completed_at: Option<String>,
    /// The microphone test step heard something above the noise floor.
    pub microphone_verified: bool,
    /// The sample command step actually created a task.
    pub first_command_done: bool,
    /// The user skipped the optional provider step.
    pub providers_skipped: bool,
}

impl Default for Onboarding {
    fn default() -> Self {
        Onboarding {
            completed: false,
            completed_at: None,
            microphone_verified: false,
            first_command_done: false,
            providers_skipped: false,
        }
    }
}

impl Onboarding {
    /// Mark the wizard as finished, keeping the per-step facts for diagnostics.
    pub fn finish(mut self) -> Onboarding {
        self.completed = true;
        self.completed_at = Some(crate::database::now_rfc3339());
        self
    }

    /// Reset so the wizard runs again ("Run setup again" in Settings).
    pub fn reset(self) -> Onboarding {
        Onboarding::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_install_has_not_completed_onboarding() {
        let state = Onboarding::default();
        assert!(!state.completed);
        assert!(state.completed_at.is_none());
    }

    #[test]
    fn finishing_records_a_timestamp_and_reset_clears_it() {
        let finished = Onboarding {
            microphone_verified: true,
            first_command_done: true,
            ..Default::default()
        }
        .finish();
        assert!(finished.completed);
        assert!(finished.completed_at.is_some());
        assert!(finished.microphone_verified);

        let reset = finished.reset();
        assert!(!reset.completed);
        assert!(reset.completed_at.is_none());
        assert!(!reset.microphone_verified);
    }
}
