//! State machine, runtime events and the event names shared with the frontend.

use serde::Serialize;
use serde::Deserialize;
use tauri::{AppHandle, Emitter, Runtime};

use crate::appearance::Appearance;
use crate::preferences::BehaviourPrefs;
use crate::settings::Settings;

/// High-level agent state, surfaced to the UI, tray tooltip and overlay.
/// Serialized as SCREAMING_SNAKE_CASE per the Nova spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NovaState {
    /// Nothing is being captured: wake listening is off or the assistant is stopped.
    Idle,
    /// The microphone is armed for the wake phrase.
    ListeningForWakeWord,
    /// The wake phrase (or manual activation) just fired.
    WakeDetected,
    /// Capturing the user's spoken command.
    ListeningForCommand,
    /// Running speech recognition over the captured command audio.
    Transcribing,
    /// Interpreting the command and deciding the action.
    Processing,
    /// Waiting for the user to disambiguate which task/thing they meant.
    AwaitingClarification,
    /// Waiting for a yes/no confirmation before a sensitive action.
    AwaitingConfirmation,
    /// Running the approved command.
    Executing,
    /// The command finished successfully (shown briefly, then Idle).
    Success,
    /// Reading the response out loud.
    Speaking,
    /// The user or a timeout cancelled the interaction; nothing changed.
    Cancelled,
    /// Something failed (recognition, provider, database, skill).
    Error,
    /// Windows/microphone permission denied.
    MicrophoneDenied,
    /// No usable microphone (unplugged, in use, or driver error).
    MicrophoneUnavailable,
}

impl NovaState {
    pub fn as_str(self) -> &'static str {
        match self {
            NovaState::Idle => "IDLE",
            NovaState::ListeningForWakeWord => "LISTENING_FOR_WAKE_WORD",
            NovaState::WakeDetected => "WAKE_DETECTED",
            NovaState::ListeningForCommand => "LISTENING_FOR_COMMAND",
            NovaState::Transcribing => "TRANSCRIBING",
            NovaState::Processing => "PROCESSING",
            NovaState::AwaitingClarification => "AWAITING_CLARIFICATION",
            NovaState::AwaitingConfirmation => "AWAITING_CONFIRMATION",
            NovaState::Executing => "EXECUTING",
            NovaState::Success => "SUCCESS",
            NovaState::Speaking => "SPEAKING",
            NovaState::Cancelled => "CANCELLED",
            NovaState::Error => "ERROR",
            NovaState::MicrophoneDenied => "MICROPHONE_DENIED",
            NovaState::MicrophoneUnavailable => "MICROPHONE_UNAVAILABLE",
        }
    }

    /// Human-readable tooltip shown on the tray icon and dashboard.
    pub fn label(self) -> &'static str {
        match self {
            NovaState::Idle => "Idle",
            NovaState::ListeningForWakeWord => "Listening for wake word",
            NovaState::WakeDetected => "Activated",
            NovaState::ListeningForCommand => "Listening for your command",
            NovaState::Transcribing => "Transcribing…",
            NovaState::Processing => "Working on it…",
            NovaState::AwaitingClarification => "Waiting for an answer",
            NovaState::AwaitingConfirmation => "Waiting for confirmation",
            NovaState::Executing => "Executing…",
            NovaState::Success => "Done",
            NovaState::Speaking => "Speaking…",
            NovaState::Cancelled => "Cancelled",
            NovaState::Error => "Error",
            NovaState::MicrophoneDenied => "Microphone permission denied",
            NovaState::MicrophoneUnavailable => "Microphone unavailable",
        }
    }

    /// A state Nova regularly settles back into.
    pub fn is_resting(self) -> bool {
        matches!(self, NovaState::Idle | NovaState::ListeningForWakeWord)
    }

    /// True while an interaction is in flight (used to reject new activations).
    pub fn is_busy(self) -> bool {
        matches!(
            self,
            NovaState::ListeningForCommand
                | NovaState::Transcribing
                | NovaState::Processing
                | NovaState::AwaitingClarification
                | NovaState::AwaitingConfirmation
                | NovaState::Executing
                | NovaState::Speaking
        )
    }

    /// Whether an interaction may end here without leaving the assistant stuck.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            NovaState::Success | NovaState::Cancelled | NovaState::Error
        )
    }
}

/// States any state may fall back to. Errors, cancellation and recovery must
/// never be blocked by the transition guard, so they are always allowed.
const RECOVERY_STATES: &[NovaState] = &[
    NovaState::Idle,
    NovaState::ListeningForWakeWord,
    NovaState::Cancelled,
    NovaState::Error,
    NovaState::MicrophoneDenied,
    NovaState::MicrophoneUnavailable,
];

/// The legal transitions of the voice lifecycle.
///
/// Anything not listed here is rejected by [`nova_can_transition`] and logged;
/// the previous state is kept, so the UI never shows an impossible state.
pub fn nova_can_transition(from: NovaState, to: NovaState) -> bool {
    use NovaState::*;
    if from == to {
        return true;
    }
    if RECOVERY_STATES.contains(&to) {
        return true;
    }
    match from {
        Idle => matches!(
            to,
            WakeDetected
                | ListeningForCommand
                | Transcribing
                | Processing
                | AwaitingConfirmation
                | Executing
        ),
        ListeningForWakeWord => matches!(
            to,
            WakeDetected | ListeningForCommand | Transcribing | Processing
        ),
        WakeDetected => matches!(
            to,
            ListeningForCommand | Transcribing | Processing | AwaitingClarification
        ),
        ListeningForCommand => matches!(
            to,
            Transcribing
                | Processing
                | AwaitingClarification
                | AwaitingConfirmation
                | Executing
                | Speaking
        ),
        Transcribing => matches!(
            to,
            Processing | AwaitingClarification | AwaitingConfirmation | Success
        ),
        Processing => matches!(
            to,
            AwaitingClarification | AwaitingConfirmation | Executing | Success | Speaking
        ),
        AwaitingClarification => matches!(to, ListeningForCommand | Transcribing | Processing),
        AwaitingConfirmation => matches!(
            to,
            ListeningForCommand | Transcribing | Processing | Executing | Success | Speaking
        ),
        Executing => matches!(
            to,
            Success | Speaking | Processing | AwaitingConfirmation
        ),
        Success => matches!(to, Speaking),
        Speaking => matches!(to, ListeningForCommand | Success),
        Cancelled => matches!(to, ListeningForCommand | Transcribing),
        Error => matches!(to, ListeningForCommand | Processing | Transcribing),
        MicrophoneDenied => matches!(to, ListeningForWakeWord | ListeningForCommand),
        MicrophoneUnavailable => matches!(to, ListeningForWakeWord | ListeningForCommand),
    }
}

/// Where a transcript came from (voice capture vs typed text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptSource {
    Voice,
    Text,
}

impl TranscriptSource {
    pub fn as_str(self) -> &'static str {
        match self {
            TranscriptSource::Voice => "voice",
            TranscriptSource::Text => "text",
        }
    }
}

/// Running state of the assistant itself (Settings → General, tray, Home).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceState {
    /// Wake-word detection is on.
    Running,
    /// Listening paused — Nova stays in the tray with its data intact.
    Paused,
    /// Wake-word detection off and the microphone released.
    Stopped,
}

impl ServiceState {
    pub fn as_str(self) -> &'static str {
        match self {
            ServiceState::Running => "running",
            ServiceState::Paused => "paused",
            ServiceState::Stopped => "stopped",
        }
    }
}

/// Microphone availability as reported to the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicrophoneStatus {
    pub available: bool,
    /// `granted` | `denied` | `unknown`
    pub permission: String,
    pub device: Option<String>,
    pub message: Option<String>,
}

impl Default for MicrophoneStatus {
    fn default() -> Self {
        MicrophoneStatus {
            available: true,
            permission: "unknown".into(),
            device: None,
            message: None,
        }
    }
}

/// Events flowing into the background runtime loop.
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    /// The wake word was detected. May carry a command spoken in the same breath.
    WakeDetected { residual_command: Option<String> },
    /// Push-to-talk hotkey pressed.
    PushToTalkPressed,
    /// Manual activation from tray or UI.
    Activate,
    /// Speech recognition started for a captured segment (or a typed command).
    TranscribeStart { source: TranscriptSource },
    /// A finished transcript is ready to be processed.
    TranscriptReady {
        text: String,
        source: TranscriptSource,
    },
    /// Command capture heard nothing (timeout) — the runtime decides how to nag.
    CommandTimeout,
    /// The user answered a pending confirmation (voice or overlay buttons).
    ConfirmResolved(bool),
    /// The question asked for clarification was never answered.
    ClarificationTimeout,
    /// The user cancelled the current interaction (Stop listening / Esc).
    CancelRequested,
    /// TTS finished speaking the current response.
    SpeechDone,
    /// Pause / resume listening.
    SetPaused(bool),
    /// Start / stop the assistant (General settings + tray).
    SetServiceState(ServiceState),
    /// Microphone availability changed (device removed, permission, engine error).
    MicrophoneStatusChanged(Box<MicrophoneStatus>),
    /// Settings were updated; apply live side effects.
    SettingsChanged(Box<Settings>),
    /// Behaviour preferences were updated; apply live side effects.
    BehaviourChanged(Box<BehaviourPrefs>),
    /// Appearance was updated; mirror it to the overlay window.
    AppearanceChanged(Box<Appearance>),
    /// The capture stream died (device change, exclusive access, driver reset).
    /// The runtime reports it and rebuilds the engine with bounded retries.
    AudioStreamFailed { message: String },
}

// Event names shared with the frontend.
pub const EVT_STATE: &str = "nova:state";
pub const EVT_TRANSCRIPT: &str = "nova:transcript";
pub const EVT_COMMAND: &str = "nova:command";
pub const EVT_RESULT: &str = "nova:result";
pub const EVT_ACTIVITY: &str = "nova:activity";
pub const EVT_CONFIRM: &str = "nova:confirm";
pub const EVT_NAVIGATE: &str = "nova:navigate";
/// Microphone level ticks (0.0..1.0) while capturing or testing.
pub const EVT_LEVEL: &str = "nova:level";
/// Microphone availability/permission changes.
pub const EVT_MIC: &str = "nova:microphone";
/// Tray-style quick status changes (service state).
pub const EVT_SERVICE: &str = "nova:service";
/// Applied appearance changed (mirrored into the overlay window).
pub const EVT_APPEARANCE: &str = "nova:appearance";
/// Overlay mode changed (`mini` | `expanded`).
pub const EVT_OVERLAY: &str = "nova:overlay";

/// Payload for `nova:state`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatePayload {
    pub state: String,
    pub detail: Option<String>,
    pub paused: bool,
    /// `running` | `paused` | `stopped`
    pub service: String,
}

/// Payload for `nova:transcript`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptPayload {
    pub text: String,
    pub final_transcript: bool,
    pub source: String,
}

/// Payload for `nova:command`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandPayload {
    pub intent: String,
    pub label: String,
    pub summary: String,
}

/// Payload for `nova:result`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultPayload {
    pub intent: Option<String>,
    pub label: Option<String>,
    pub success: bool,
    pub status: String,
    pub response: String,
    /// 1 when the Activity screen can offer a retry.
    pub retryable: bool,
    pub activity_id: Option<i64>,
}

/// Payload for `nova:confirm`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmPayload {
    pub pending: bool,
    pub prompt: Option<String>,
    /// The button label for the affirmative action ("Delete task", "Confirm").
    pub confirm_label: Option<String>,
    /// Set for clarification questions ("Which one did you mean?").
    pub kind: Option<String>,
    /// Candidate answers offered for a clarification question.
    pub options: Option<Vec<String>>,
}

/// Payload for `nova:navigate`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigatePayload {
    pub page: String,
}

/// Payload for `nova:level`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelPayload {
    /// Smoothed input level 0.0..1.0.
    pub level: f32,
    /// True while the microphone test sweep is running.
    pub testing: bool,
}

/// Payload for `nova:service`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicePayload {
    pub service: String,
    pub label: String,
    pub microphone: MicrophoneStatus,
}

/// Payload for `nova:overlay`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayPayload {
    /// `mini` (collapsed pill) or `expanded`.
    pub mode: String,
}

/// Emit an event to all Nova windows.
pub fn emit_nova<R: Runtime, T: Serialize + Clone>(app: &AppHandle<R>, event: &str, payload: T) {
    let _ = app.emit(event, payload);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_transitions_are_guarded() {
        use NovaState::*;
        // The happy path of Flow A.
        assert!(nova_can_transition(ListeningForWakeWord, WakeDetected));
        assert!(nova_can_transition(WakeDetected, ListeningForCommand));
        assert!(nova_can_transition(ListeningForCommand, Transcribing));
        assert!(nova_can_transition(Transcribing, Processing));
        assert!(nova_can_transition(Processing, Executing));
        assert!(nova_can_transition(Executing, Success));
        assert!(nova_can_transition(Success, Idle));

        // Nonsense transitions are refused.
        assert!(!nova_can_transition(ListeningForWakeWord, Executing));
        assert!(!nova_can_transition(Idle, Success));
        assert!(!nova_can_transition(Success, Executing));
        assert!(!nova_can_transition(Speaking, Executing));
        // Executing straight from rest is legal (a command run without a wake
        // phrase, e.g. from the dashboard or a retry).
        assert!(nova_can_transition(Idle, Executing));
    }

    #[test]
    fn every_state_can_recover() {
        use NovaState::*;
        let all = [
            Idle,
            ListeningForWakeWord,
            WakeDetected,
            ListeningForCommand,
            Transcribing,
            Processing,
            AwaitingClarification,
            AwaitingConfirmation,
            Executing,
            Success,
            Speaking,
            Cancelled,
            Error,
            MicrophoneDenied,
            MicrophoneUnavailable,
        ];
        // No state may trap the assistant: recovery is always possible, and the
        // microphone failure states can always be re-entered from anywhere.
        for from in all {
            for to in RECOVERY_STATES {
                assert!(
                    nova_can_transition(from, *to),
                    "{} must be able to recover to {}",
                    from.as_str(),
                    to.as_str()
                );
            }
            assert!(nova_can_transition(from, from));
        }
    }

    #[test]
    fn state_labels_and_flags_are_consistent() {
        assert_eq!(NovaState::Idle.as_str(), "IDLE");
        assert_eq!(NovaState::AwaitingConfirmation.as_str(), "AWAITING_CONFIRMATION");
        assert!(NovaState::Speaking.is_busy());
        assert!(NovaState::Idle.is_resting());
        assert!(NovaState::Error.is_terminal());
        assert!(NovaState::Success.is_terminal());
    }
}
