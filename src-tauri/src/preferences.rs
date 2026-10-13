//! Nova behaviour preferences: response verbosity, confirmation policy,
//! overlay behaviour, history retention and notifications.
//!
//! These live in their own persisted row (SQLite `settings` table, key
//! `behaviour`) rather than in the core [`crate::settings::Settings`] record,
//! because they are pure UI/behaviour policy: each one has a defined runtime
//! effect and none of them touch voice capture or provider configuration.
//!
//! Every setting here is applied by `runtime::apply_behaviour_change`, so a
//! change in the Settings screen changes real behaviour immediately.

use serde::{Deserialize, Serialize};

pub const BEHAVIOUR_KEY: &str = "behaviour";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResponseVerbosity {
    /// Short confirmations only.
    Brief,
    /// The standard response (default).
    Standard,
    /// Standard response plus what Nova understood.
    Detailed,
}

impl Default for ResponseVerbosity {
    fn default() -> Self {
        ResponseVerbosity::Standard
    }
}

impl ResponseVerbosity {
    /// Render the spoken/displayed response for an outcome.
    ///
    /// * `brief` keeps the first sentence (or the whole short reply).
    /// * `detailed` appends what Nova understood.
    pub fn render(self, response: &str, understood: Option<&str>) -> String {
        let base = response.trim();
        match self {
            ResponseVerbosity::Brief => first_sentence(base),
            ResponseVerbosity::Standard => base.to_string(),
            ResponseVerbosity::Detailed => match understood {
                Some(summary) if !summary.trim().is_empty() => {
                    format!("{base} ({summary})")
                }
                _ => base.to_string(),
            },
        }
    }
}

/// First sentence of `text`, keeping short responses intact.
fn first_sentence(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= 60 {
        return trimmed.to_string();
    }
    let mut out = String::new();
    for ch in trimmed.chars() {
        out.push(ch);
        if matches!(ch, '.' | '!' | '?') {
            break;
        }
    }
    let sentence = out.trim().to_string();
    if sentence.is_empty() {
        trimmed.to_string()
    } else {
        sentence
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BehaviourPrefs {
    /// How much Nova says back after a command.
    pub response_verbosity: ResponseVerbosity,
    /// Also confirm before opening applications, files and folders.
    pub confirm_open: bool,
    /// Pop the floating overlay for commands started from the dashboard.
    /// Voice activation always shows it (the user is already talking to it).
    pub overlay_after_command: bool,
    /// Keep the overlay expanded between commands instead of collapsing to the pill.
    pub overlay_keep_expanded: bool,
    /// How long command history is kept: 0, 30, 90 or 365 days.
    pub history_retention_days: i64,
    /// Notify (in-app toast) when a command finishes while the window is closed.
    pub notifications_enabled: bool,
}

impl Default for BehaviourPrefs {
    fn default() -> Self {
        BehaviourPrefs {
            response_verbosity: ResponseVerbosity::Standard,
            confirm_open: false,
            overlay_after_command: true,
            overlay_keep_expanded: false,
            history_retention_days: 90,
            notifications_enabled: true,
        }
    }
}

impl BehaviourPrefs {
    /// Clamp into the ranges the UI offers, so a hand-edited database can't
    /// put Nova into a state the UI cannot represent.
    pub fn sanitized(mut self) -> BehaviourPrefs {
        self.history_retention_days = match self.history_retention_days {
            days if days <= 0 => 0,
            days if days <= 30 => 30,
            days if days <= 90 => 90,
            _ => 365,
        };
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_designed_settings_screen() {
        let prefs = BehaviourPrefs::default();
        assert_eq!(prefs.response_verbosity, ResponseVerbosity::Standard);
        assert!(prefs.confirm_open == false, "off by default in the design");
        assert!(prefs.overlay_after_command);
        assert!(prefs.overlay_keep_expanded == false);
        assert_eq!(prefs.history_retention_days, 90);
        assert!(prefs.notifications_enabled);
    }

    #[test]
    fn retention_is_clamped_to_supported_windows() {
        let clamp = |days: i64| {
            BehaviourPrefs {
                history_retention_days: days,
                ..Default::default()
            }
            .sanitized()
            .history_retention_days
        };
        assert_eq!(clamp(-5), 0);
        assert_eq!(clamp(7), 30);
        assert_eq!(clamp(45), 90);
        assert_eq!(clamp(1000), 365);
    }

    #[test]
    fn verbosity_changes_the_response() {
        let long = "Marked 'finish DBMS assignment' as completed on your list. Anything else?";
        assert_eq!(
            ResponseVerbosity::Standard.render(long, Some("Complete task · “DBMS”")),
            long
        );
        assert_eq!(
            ResponseVerbosity::Brief.render(long, None),
            "Marked 'finish DBMS assignment' as completed on your list."
        );
        assert_eq!(
            ResponseVerbosity::Detailed.render("Added buy milk.", Some("Add task · “buy milk”")),
            "Added buy milk. (Add task · “buy milk”)"
        );
        // Short replies are never truncated by `brief`.
        assert_eq!(
            ResponseVerbosity::Brief.render("Added buy milk.", None),
            "Added buy milk."
        );
    }

    #[test]
    fn roundtrip_through_json() {
        let prefs = BehaviourPrefs {
            response_verbosity: ResponseVerbosity::Detailed,
            overlay_keep_expanded: true,
            history_retention_days: 30,
            ..Default::default()
        };
        let json = serde_json::to_string(&prefs).unwrap();
        assert_eq!(
            serde_json::from_str::<BehaviourPrefs>(&json).unwrap(),
            prefs.sanitized()
        );
        // Unknown/partial JSON falls back to defaults field by field.
        let partial: BehaviourPrefs = serde_json::from_str("{\"confirm_open\":true}").unwrap();
        assert!(partial.confirm_open);
        assert_eq!(partial.response_verbosity, ResponseVerbosity::Standard);
    }
}
