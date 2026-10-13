//! The structured command layer: the registry of commands Nova can execute,
//! their parameter schemas, risk levels, validation, and the executor.

pub mod executor;
pub mod registry;
pub mod validator;

use serde::{Deserialize, Serialize};

/// A structured command produced by an intent engine (LLM tool call or the
/// offline parser). This is the ONLY shape the agent will execute.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommandRequest {
    pub intent: String,
    #[serde(default)]
    pub parameters: serde_json::Value,
}

/// Risk classification for commands (spec §13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RiskLevel {
    Safe,
    ConfirmationRequired,
    Blocked,
}

/// Result of a validated command execution.
#[derive(Debug, Clone, Serialize)]
pub struct SkillOutcome {
    pub success: bool,
    /// Ready-to-speak user-facing response.
    pub response: String,
    /// Structured data for the activity log and UI.
    pub data: serde_json::Value,
}
