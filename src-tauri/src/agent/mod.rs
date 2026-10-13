//! The agent: converts a transcript into a validated, executed command and a
//! spoken response. This is the single choke point where interpretation
//! results turn into actions — nothing bypasses the registry and validator.

pub mod context;
pub mod llm;

use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, Runtime};

use crate::commands::{executor, registry, validator, CommandRequest, RiskLevel};
use crate::database::{self, NewActivity};
use crate::error::NovaError;
use crate::events::{
    self, CommandPayload, ConfirmPayload, NovaState, ResultPayload, TranscriptSource,
};
use crate::settings::Settings;
use crate::state::AppState;
use crate::util::{normalize_text, truncate_for_speech};

use context::{ConversationContext, Exchange};

/// Confirmation prompts expire after this long.
pub const CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(60);
/// Clarification questions expire after this long.
pub const CLARIFICATION_TIMEOUT: Duration = Duration::from_secs(45);
/// A transcript identical to the previous one inside this window is treated as
/// a repeated submission and is not executed twice.
const DUPLICATE_WINDOW: Duration = Duration::from_millis(2500);

#[derive(Debug, Clone)]
pub struct PendingConfirmation {
    pub command: CommandRequest,
    pub prompt: String,
    /// The original user utterance that triggered the confirmation.
    pub transcript: String,
    pub created_at: Instant,
    /// Label for the affirmative overlay button ("Delete task", "Open app").
    pub confirm_label: String,
}

/// A question the assistant asked that is still waiting for an answer,
/// e.g. "I found several matching tasks: … Which one did you mean?".
#[derive(Debug, Clone)]
pub struct PendingClarification {
    /// Intent to continue with once the user disambiguates.
    pub intent: String,
    /// The partially-known parameters (everything except the target).
    pub params: Value,
    pub created_at: Instant,
}

#[derive(Debug, Clone)]
struct LastTurn {
    normalized: String,
    response: String,
    at: Instant,
}

#[derive(Default)]
pub struct AgentSession {
    pub context: ConversationContext,
    pub pending: Option<PendingConfirmation>,
    pub clarification: Option<PendingClarification>,
    last: Option<LastTurn>,
}

/// The outcome of one agent turn.
#[derive(Debug, Clone, Serialize)]
pub struct AgentTurnResult {
    pub command: Option<CommandRequest>,
    /// Human label of the intent ("Add task").
    pub label: Option<String>,
    pub success: bool,
    /// `success` | `failed` | `cancelled` | `awaiting_confirmation`
    /// | `awaiting_clarification` | `unsupported` | `timeout`
    pub status: String,
    pub response: String,
    /// True when the turn ended by asking the user for confirmation.
    pub awaiting_confirmation: bool,
    /// True when the turn ended by asking the user to disambiguate.
    pub awaiting_clarification: bool,
    /// True when the Activity screen can offer a retry for this entry.
    pub retryable: bool,
    /// Row id in the activity log, when one was written.
    pub activity_id: Option<i64>,
}

impl AgentTurnResult {
    fn plain(response: impl Into<String>, success: bool) -> Self {
        let response = response.into();
        AgentTurnResult {
            command: None,
            label: None,
            success,
            status: if success { "success" } else { "failed" }.to_string(),
            response,
            awaiting_confirmation: false,
            awaiting_clarification: false,
            retryable: false,
            activity_id: None,
        }
    }

    fn new(
        command: Option<CommandRequest>,
        label: Option<String>,
        outcome: &Outcome,
        response: String,
        activity_id: Option<i64>,
    ) -> Self {
        AgentTurnResult {
            command,
            label,
            success: outcome.success,
            status: outcome.status.to_string(),
            response,
            awaiting_confirmation: outcome.status == database::STATUS_AWAITING_CONFIRMATION,
            awaiting_clarification: outcome.status == database::STATUS_AWAITING_CLARIFICATION,
            retryable: outcome.retryable,
            activity_id,
        }
    }
}

/// Handle a finished transcript (voice capture or typed text). This is the
/// main entry point used by the runtime loop and the `process_text` command.
pub async fn handle_transcript<R: Runtime>(
    app: &AppHandle<R>,
    text: &str,
    source: TranscriptSource,
) -> AgentTurnResult {
    let started = Instant::now();
    let text = text.trim().to_string();

    let state = app.state::<AppState>();
    let settings = state.settings_snapshot();
    let mut session = state.agent.lock().await;

    if text.is_empty() {
        // A capture that produced no recognisable words is not a failed
        // command: it gets its own status so the overlay can offer
        // "Try again" / "Type instead" instead of a dead end.
        let outcome = Outcome {
            status: database::STATUS_NOT_HEARD,
            success: false,
            response: "I didn't catch that. Try again, or type the command instead.".into(),
            understood: Some("No speech was recognised".into()),
            action: Some("Nothing changed.".into()),
            retryable: true,
        };
        let activity_id = finalize_turn(
            app,
            &mut session,
            &text,
            None,
            None,
            None,
            &outcome,
            started,
            None,
        );
        return AgentTurnResult::new(None, None, &outcome, outcome.response.clone(), activity_id);
    }

    // Guard against the same submission being executed twice in a row.
    let normalized = normalize_text(&text);
    let duplicate_of = session
        .last
        .as_ref()
        .filter(|last| {
            !normalized.is_empty()
                && last.normalized == normalized
                && last.at.elapsed() <= DUPLICATE_WINDOW
        })
        .map(|last| last.response.clone());
    if let Some(previous) = duplicate_of {
        let last_intent = session.context.last_intent.clone();
        let response = format!("I already handled that — {previous}");
        let outcome = Outcome {
            status: database::STATUS_SUCCESS,
            success: true,
            response,
            understood: Some("Repeated command · skipped so it isn't done twice".to_string()),
            action: Some("Recognized the command was just handled and did not repeat it.".into()),
            retryable: false,
        };
        let activity_id = log_activity(
            app,
            ActivityRecord {
                transcript: &text,
                intent: last_intent.as_deref(),
                parameters: None,
                outcome: &outcome,
                duration_ms: Some(started.elapsed().as_millis() as i64),
                data: &json!({ "duplicate": true }),
                provider: None,
            },
        );
        return AgentTurnResult::new(None, None, &outcome, outcome.response.clone(), activity_id);
    }

    // Resolve a pending confirmation first.
    if let Some(pending) = session.pending.take() {
        if pending.created_at.elapsed() <= CONFIRMATION_TIMEOUT {
            match parse_confirmation(&text) {
                Some(true) => {
                    set_awaiting(app, false);
                    emit_confirm(app, false, None, None, None);
                    return run_command(
                        app,
                        &mut session,
                        &settings,
                        pending.command,
                        &pending.transcript,
                        started,
                        source,
                        Some("confirmed"),
                        true,
                    )
                    .await;
                }
                Some(false) => {
                    set_awaiting(app, false);
                    emit_confirm(app, false, None, None, None);
                    let outcome = Outcome {
                        status: database::STATUS_CANCELLED,
                        success: true,
                        response: "Okay, cancelled. Nothing changed.".into(),
                        understood: Some("Asked for confirmation — the user declined".into()),
                        action: Some("Nothing changed.".into()),
                        retryable: false,
                    };
                    let activity_id = finalize_turn(
                        app,
                        &mut session,
                        &text,
                        Some(&pending.command.intent),
                        None,
                        None,
                        &outcome,
                        started,
                        Some("cancelled"),
                    );
                    return AgentTurnResult::new(
                        None,
                        None,
                        &outcome,
                        outcome.response.clone(),
                        activity_id,
                    );
                }
                None => {
                    // Unrelated utterance: cancel the pending request and
                    // process this as a fresh command.
                    set_awaiting(app, false);
                    emit_confirm(app, false, None, None, None);
                }
            }
        } else {
            set_awaiting(app, false);
            emit_confirm(app, false, None, None, None);
            let outcome = Outcome {
                status: database::STATUS_TIMEOUT,
                success: false,
                response:
                    "That request expired, so I did nothing. Ask me again and I'll take care of it."
                        .into(),
                understood: Some("Confirmation timed out".into()),
                action: Some("Nothing changed.".into()),
                retryable: true,
            };
            let activity_id = finalize_turn(
                app,
                &mut session,
                &text,
                Some(&pending.command.intent),
                None,
                None,
                &outcome,
                started,
                Some("expired"),
            );
            return AgentTurnResult::new(None, None, &outcome, outcome.response.clone(), activity_id);
        }
    }

    // A clarification answer: continue the interrupted intent with the answer
    // as the task reference, unless the answer is itself a different command.
    if let Some(pending) = session.clarification.clone() {
        if pending.created_at.elapsed() > CLARIFICATION_TIMEOUT {
            session.clarification = None;
        } else {
            let answer_intent = match llm::interpret(&settings, &text, &session.context).await {
                Ok(Some(interpretation)) => Some(interpretation.command.intent),
                _ => None,
            };
            match answer_intent {
                // The user asked for something else — drop the question.
                Some(intent) if intent != pending.intent => {
                    session.clarification = None;
                }
                // Either the same intent or no parseable intent: use the answer
                // as the reference for the question we asked.
                _ => {
                    session.clarification = None;
                    let mut params = pending.params.as_object().cloned().unwrap_or_default();
                    params.remove("todoId");
                    // The answer fills whichever slot the interrupted intent is
                    // missing: a new task needs a title, an app request needs an
                    // application name, everything else a task reference.
                    let slot = match pending.intent.as_str() {
                        "ADD_TODO" => "title",
                        "OPEN_APPLICATION" => "application",
                        _ => "searchQuery",
                    };
                    params.remove("searchQuery");
                    params.insert(slot.into(), json!(text));
                    let request = CommandRequest {
                        intent: pending.intent,
                        parameters: Value::Object(params),
                    };
                    return run_command(
                        app,
                        &mut session,
                        &settings,
                        request,
                        &text,
                        started,
                        source,
                        Some("clarified"),
                        false,
                    )
                    .await;
                }
            }
        }
    }

    // A request to add something that never said what: ask instead of
    // answering "I don't know how to do that". Only reached when no
    // confirmation or question is already pending.
    if asks_for_a_task_without_one(&text) {
        let prompt = "What would you like me to add?".to_string();
        let outcome = Outcome {
            status: database::STATUS_AWAITING_CLARIFICATION,
            success: false,
            response: prompt.clone(),
            understood: Some("Add task · waiting for the task text".into()),
            action: Some("Asked what to add — nothing created yet.".into()),
            retryable: false,
        };
        session.clarification = Some(PendingClarification {
            intent: "ADD_TODO".into(),
            params: json!({}),
            created_at: Instant::now(),
        });
        set_awaiting_clarification(app, true);
        emit_clarification(app, &prompt, &[]);
        crate::runtime::set_state(app, NovaState::AwaitingClarification, Some(prompt.clone()));
        let activity_id = finalize_turn(
            app,
            &mut session,
            &text,
            Some("ADD_TODO"),
            None,
            None,
            &outcome,
            started,
            None,
        );
        return AgentTurnResult::new(None, Some("Add task".into()), &outcome, prompt, activity_id);
    }

    // Interpret the utterance.
    let interpretation = llm::interpret(&settings, &text, &session.context).await;
    let (request, provider) = match interpretation {
        Ok(Some(interpretation)) => (interpretation.command, interpretation.provider),
        Ok(None) => {
            let outcome = Outcome {
                status: database::STATUS_UNSUPPORTED,
                success: false,
                response: "I don't know how to do that yet. I can manage your tasks and open \
                           apps, files and folders."
                    .into(),
                understood: Some("Nothing in the command registry matched this request".into()),
                action: Some("Nothing changed.".into()),
                retryable: false,
            };
            let activity_id = finalize_turn(
                app, &mut session, &text, None, None, None, &outcome, started, None,
            );
            return AgentTurnResult::new(None, None, &outcome, outcome.response.clone(), activity_id);
        }
        Err(NovaError::MissingApiKey) => {
            let outcome = Outcome {
                status: database::STATUS_FAILED,
                success: false,
                response: "I don't have an AI service configured, and that request doesn't \
                           match anything I can do offline. You can add an API key in Settings."
                    .into(),
                understood: Some("No AI provider configured".into()),
                action: Some("Nothing changed.".into()),
                retryable: false,
            };
            let activity_id = finalize_turn(
                app, &mut session, &text, None, None, None, &outcome, started, None,
            );
            return AgentTurnResult::new(None, None, &outcome, outcome.response.clone(), activity_id);
        }
        Err(err) => {
            eprintln!("[nova] interpretation failed: {err}");
            let outcome = Outcome {
                status: database::STATUS_FAILED,
                success: false,
                response: "I couldn't reach the language model, so I did nothing. You can retry, \
                           or keep using the built-in commands."
                    .into(),
                understood: Some("Provider request failed".into()),
                action: Some("Nothing changed.".into()),
                retryable: true,
            };
            let activity_id = finalize_turn(
                app, &mut session, &text, None, None, None, &outcome, started, None,
            );
            return AgentTurnResult::new(None, None, &outcome, outcome.response.clone(), activity_id);
        }
    };

    run_command(
        app,
        &mut session,
        &settings,
        request,
        &text,
        started,
        source,
        Some(provider),
        false,
    )
    .await
}

/// Resolve a pending confirmation outside a transcript (overlay buttons).
pub async fn handle_confirmation<R: Runtime>(
    app: &AppHandle<R>,
    confirmed: bool,
) -> AgentTurnResult {
    let started = Instant::now();
    let state = app.state::<AppState>();
    let settings = state.settings_snapshot();
    let mut session = state.agent.lock().await;

    let Some(pending) = session.pending.take() else {
        return AgentTurnResult::plain("There is nothing waiting for confirmation.", true);
    };
    set_awaiting(app, false);
    emit_confirm(app, false, None, None, None);

    if pending.created_at.elapsed() > CONFIRMATION_TIMEOUT {
        let outcome = Outcome {
            status: database::STATUS_TIMEOUT,
            success: false,
            response: "That request timed out. Ask me again and I'll take care of it.".into(),
            understood: Some("Confirmation timed out".into()),
            action: Some("Nothing changed.".into()),
            retryable: true,
        };
        let activity_id = finalize_turn(
            app,
            &mut session,
            &pending.transcript,
            Some(&pending.command.intent),
            None,
            None,
            &outcome,
            started,
            Some("expired"),
        );
        return AgentTurnResult::new(None, None, &outcome, outcome.response.clone(), activity_id);
    }

    if confirmed {
        run_command(
            app,
            &mut session,
            &settings,
            pending.command,
            &pending.transcript,
            started,
            TranscriptSource::Voice,
            Some("confirmed"),
            true,
        )
        .await
    } else {
        let outcome = Outcome {
            status: database::STATUS_CANCELLED,
            success: true,
            response: "Okay, cancelled. Nothing changed.".into(),
            understood: Some("Confirmation declined in the overlay".into()),
            action: Some("Nothing changed.".into()),
            retryable: false,
        };
        let activity_id = finalize_turn(
            app,
            &mut session,
            &pending.transcript,
            Some(&pending.command.intent),
            None,
            None,
            &outcome,
            started,
            Some("cancelled"),
        );
        AgentTurnResult::new(None, None, &outcome, outcome.response.clone(), activity_id)
    }
}

/// Everything a turn needs to log and report.
struct Outcome {
    status: &'static str,
    success: bool,
    response: String,
    understood: Option<String>,
    action: Option<String>,
    retryable: bool,
}

#[allow(clippy::too_many_arguments)]
async fn run_command<R: Runtime>(
    app: &AppHandle<R>,
    session: &mut AgentSession,
    settings: &Settings,
    request: CommandRequest,
    transcript: &str,
    started: Instant,
    source: TranscriptSource,
    provider: Option<&str>,
    already_confirmed: bool,
) -> AgentTurnResult {
    // Strict validation: unknown intents / malformed parameters never execute.
    let validated = match validator::validate(&request) {
        Ok(validated) => validated,
        Err(NovaError::Unsupported(message)) => {
            eprintln!("[nova] rejected command: {message}");
            let outcome = Outcome {
                status: database::STATUS_UNSUPPORTED,
                success: false,
                response: "I don't know how to do that yet.".into(),
                understood: Some("The intent is not in Nova's command registry".into()),
                action: Some("Nothing changed.".into()),
                retryable: false,
            };
            let activity_id = finalize_turn(
                app, session, transcript, None, None, None, &outcome, started, provider,
            );
            return AgentTurnResult::new(None, None, &outcome, outcome.response.clone(), activity_id);
        }
        Err(NovaError::InvalidCommand(message)) => {
            eprintln!("[nova] invalid command parameters: {message}");
            let outcome = Outcome {
                status: database::STATUS_FAILED,
                success: false,
                response: "I didn't understand how to do that. Please try again.".into(),
                understood: Some("The command parameters did not validate".into()),
                action: Some("Nothing changed.".into()),
                retryable: false,
            };
            let activity_id = finalize_turn(
                app,
                session,
                transcript,
                Some(&request.intent),
                Some(&request.parameters),
                None,
                &outcome,
                started,
                provider,
            );
            return AgentTurnResult::new(None, None, &outcome, outcome.response.clone(), activity_id);
        }
        Err(err) => {
            eprintln!("[nova] command validation error: {err}");
            let outcome = Outcome {
                status: database::STATUS_FAILED,
                success: false,
                response: "Something went wrong while preparing that command.".into(),
                understood: Some("Command validation failed".into()),
                action: Some("Nothing changed.".into()),
                retryable: true,
            };
            let activity_id = finalize_turn(
                app,
                session,
                transcript,
                Some(&request.intent),
                Some(&request.parameters),
                None,
                &outcome,
                started,
                provider,
            );
            return AgentTurnResult::new(None, None, &outcome, outcome.response.clone(), activity_id);
        }
    };

    emit_command(app, &validated.intent, &validated.label, &validated.params);
    let understood = registry::summarize(&validated.intent, &validated.params);
    let label = validated.label.clone();

    if validated.risk == RiskLevel::Blocked {
        let outcome = Outcome {
            status: database::STATUS_UNSUPPORTED,
            success: false,
            response: "I can't do that — Nova never runs scripts or shell commands.".into(),
            understood: Some("Blocked by Nova's safety policy".into()),
            action: Some("Refused — this capability is disabled.".into()),
            retryable: false,
        };
        let activity_id = finalize_turn(
            app,
            session,
            transcript,
            Some(&validated.intent),
            Some(&validated.params),
            Some(&json!({ "blocked": true })),
            &outcome,
            started,
            provider,
        );
        return AgentTurnResult::new(
            Some(request),
            Some(label),
            &outcome,
            outcome.response.clone(),
            activity_id,
        );
    }

    // Confirmation gate for destructive actions, and for "opening things" when
    // the user asked to be asked first.
    let needs_confirmation = (validated.risk == RiskLevel::ConfirmationRequired
        && settings.confirm_destructive)
        || (is_opening_intent(&validated.intent)
            && app.state::<AppState>().behaviour_snapshot().confirm_open);

    if needs_confirmation && !already_confirmed {
        if validated.intent == "DELETE_TODO" {
            match preview_delete(app, &validated.params) {
                DeletePreview::Found(title) => {
                    let prompt = format!(
                        "This will delete '{}' from your list. Do you want me to continue?",
                        truncate_for_speech(&title, 60)
                    );
                    let outcome = Outcome {
                        status: database::STATUS_AWAITING_CONFIRMATION,
                        success: true,
                        response: prompt.clone(),
                        understood: Some("Delete task · waiting for confirmation".into()),
                        action: Some("Asked for confirmation before deleting.".into()),
                        retryable: false,
                    };
                    session.pending = Some(PendingConfirmation {
                        command: request.clone(),
                        prompt: prompt.clone(),
                        transcript: transcript.to_string(),
                        created_at: Instant::now(),
                        confirm_label: "Delete task".into(),
                    });
                    set_awaiting(app, true);
                    emit_confirm(app, true, Some(&prompt), Some("Delete task"), Some("confirmation"));
                    crate::runtime::set_state(
                        app,
                        NovaState::AwaitingConfirmation,
                        Some(prompt.clone()),
                    );
                    let activity_id = finalize_turn(
                        app,
                        session,
                        transcript,
                        Some(&validated.intent),
                        Some(&validated.params),
                        Some(&json!({ "awaitingConfirmation": true })),
                        &outcome,
                        started,
                        provider,
                    );
                    return AgentTurnResult::new(
                        Some(request),
                        Some(label),
                        &outcome,
                        prompt,
                        activity_id,
                    );
                }
                DeletePreview::Ambiguous(message, candidates) => {
                    let outcome = Outcome {
                        status: database::STATUS_AWAITING_CLARIFICATION,
                        success: false,
                        response: message.clone(),
                        understood: Some("Delete task · several matches".into()),
                        action: Some("Asked which task was meant — nothing deleted.".into()),
                        retryable: false,
                    };
                    session.clarification = Some(PendingClarification {
                        intent: validated.intent.clone(),
                        params: validated.params.clone(),
                        created_at: Instant::now(),
                    });
                    set_awaiting_clarification(app, true);
                    emit_clarification(app, &message, &candidates);
                    crate::runtime::set_state(
                        app,
                        NovaState::AwaitingClarification,
                        Some(message.clone()),
                    );
                    let activity_id = finalize_turn(
                        app,
                        session,
                        transcript,
                        Some(&validated.intent),
                        Some(&validated.params),
                        Some(&json!({ "ambiguous": true, "candidates": candidates })),
                        &outcome,
                        started,
                        provider,
                    );
                    return AgentTurnResult::new(
                        Some(request),
                        Some(label),
                        &outcome,
                        message,
                        activity_id,
                    );
                }
                DeletePreview::NotFound => {
                    let outcome = Outcome {
                        status: database::STATUS_FAILED,
                        success: false,
                        response: "I couldn't find that task on your list.".into(),
                        understood: Some("Delete task · no match".into()),
                        action: Some("Nothing changed.".into()),
                        retryable: false,
                    };
                    let activity_id = finalize_turn(
                        app,
                        session,
                        transcript,
                        Some(&validated.intent),
                        Some(&validated.params),
                        Some(&json!({ "found": false })),
                        &outcome,
                        started,
                        provider,
                    );
                    return AgentTurnResult::new(
                        Some(request),
                        Some(label),
                        &outcome,
                        outcome.response.clone(),
                        activity_id,
                    );
                }
            }
        }

        // Generic confirmation for the remaining sensitive commands.
        let prompt = describe_confirmation(&validated.intent, &validated.params);
        let confirm_label = confirm_label_for(&validated.intent);
        let outcome = Outcome {
            status: database::STATUS_AWAITING_CONFIRMATION,
            success: true,
            response: prompt.clone(),
            understood: Some(format!("{understood} · waiting for confirmation")),
            action: Some("Asked for confirmation before acting.".into()),
            retryable: false,
        };
        session.pending = Some(PendingConfirmation {
            command: request.clone(),
            prompt: prompt.clone(),
            transcript: transcript.to_string(),
            created_at: Instant::now(),
            confirm_label: confirm_label.clone(),
        });
        set_awaiting(app, true);
        emit_confirm(
            app,
            true,
            Some(&prompt),
            Some(&confirm_label),
            Some("confirmation"),
        );
        crate::runtime::set_state(app, NovaState::AwaitingConfirmation, Some(prompt.clone()));
        let activity_id = finalize_turn(
            app,
            session,
            transcript,
            Some(&validated.intent),
            Some(&validated.params),
            Some(&json!({ "awaitingConfirmation": true })),
            &outcome,
            started,
            provider,
        );
        return AgentTurnResult::new(Some(request), Some(label), &outcome, prompt, activity_id);
    }

    // Execute.
    crate::runtime::set_state(app, NovaState::Executing, None);
    match executor::execute(app, &validated.intent, &validated.params, source.as_str()) {
        Ok(skill) => {
            if let Some(id) = skill.data.get("todoId").and_then(|v| v.as_i64()) {
                session.context.last_todo_id = Some(id);
            }
            if validated.intent == "OPEN_APPLICATION" {
                if let Some(name) = validated.params.get("application").and_then(|v| v.as_str()) {
                    session.context.last_app = Some(name.to_string());
                }
            }

            // The skill asked the user to disambiguate — keep capture armed.
            if skill
                .data
                .get("requiresClarification")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                let outcome = Outcome {
                    status: database::STATUS_AWAITING_CLARIFICATION,
                    success: false,
                    response: skill.response.clone(),
                    understood: Some(format!("{understood} · several tasks matched")),
                    action: Some("Asked which task was meant — nothing changed yet.".into()),
                    retryable: false,
                };
                session.clarification = Some(PendingClarification {
                    intent: validated.intent.clone(),
                    params: validated.params.clone(),
                    created_at: Instant::now(),
                });
                set_awaiting_clarification(app, true);
                let options: Vec<String> = skill
                    .data
                    .get("candidates")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str())
                            .map(|item| item.to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                emit_clarification(app, &skill.response, &options);
                crate::runtime::set_state(
                    app,
                    NovaState::AwaitingClarification,
                    Some(skill.response.clone()),
                );
                let activity_id = finalize_turn(
                    app,
                    session,
                    transcript,
                    Some(&validated.intent),
                    Some(&validated.params),
                    Some(&skill.data),
                    &outcome,
                    started,
                    provider,
                );
                return AgentTurnResult::new(
                    Some(request),
                    Some(label),
                    &outcome,
                    skill.response,
                    activity_id,
                );
            }

            let retryable = !skill.success
                && skill.data.get("found").is_none()
                && skill.data.get("ambiguous").is_none();
            let status = if skill.success {
                database::STATUS_SUCCESS
            } else {
                database::STATUS_FAILED
            };
            let outcome = Outcome {
                status,
                success: skill.success,
                response: skill.response.clone(),
                understood: Some(understood),
                action: Some(describe_action(
                    &validated.intent,
                    &validated.params,
                    skill.success,
                )),
                retryable,
            };
            let activity_id = finalize_turn(
                app,
                session,
                transcript,
                Some(&validated.intent),
                Some(&validated.params),
                Some(&skill.data),
                &outcome,
                started,
                provider,
            );
            return AgentTurnResult::new(
                Some(request),
                Some(label),
                &outcome,
                skill.response,
                activity_id,
            );
        }
        Err(err) => {
            eprintln!("[nova] command execution failed: {err}");
            let outcome = Outcome {
                status: database::STATUS_FAILED,
                success: false,
                response: format!("I couldn't do that. ({err})"),
                understood: Some(understood),
                action: Some("Nothing changed.".into()),
                retryable: true,
            };
            let activity_id = finalize_turn(
                app,
                session,
                transcript,
                Some(&validated.intent),
                Some(&validated.params),
                Some(&json!({ "error": err.to_string() })),
                &outcome,
                started,
                provider,
            );
            return AgentTurnResult::new(
                Some(request),
                Some(label),
                &outcome,
                outcome.response.clone(),
                activity_id,
            );
        }
    }
}

fn is_opening_intent(intent: &str) -> bool {
    matches!(intent, "OPEN_APPLICATION" | "OPEN_FILE" | "OPEN_FOLDER")
}

/// True when the user asked Nova to add something but never said what
/// ("can you add a…", "remind me", "add to my list"). Such a request is
/// answered with a question, not with a refusal.
fn asks_for_a_task_without_one(text: &str) -> bool {
    // Lead-ins are stripped with the parser's own helper so both agree on what
    // "can you add a" means.
    let cleaned = crate::agent::llm::offline::strip_leadins(text);
    let normalized = normalize_text(&cleaned);
    if normalized.is_empty() {
        return false;
    }
    const VERBS: &[&str] = &[
        "add",
        "add a",
        "add an",
        "add the",
        "add to my list",
        "add to my todo list",
        "add to my to do list",
        "put on my list",
        "put it on my list",
        "remind me",
        "remind me to",
        "note down",
        "write down",
        "jot down",
        "make a note",
        "new task",
        "create a task",
        "create a todo",
    ];
    for verb in VERBS {
        if let Some(rest) = normalized.strip_prefix(verb) {
            let rest = rest.trim();
            // "add a" alone, or only filler left over → nothing to add yet.
            const FILLER: &[&str] = &[
                "", "a", "an", "the", "it", "that", "this", "one", "something", "anything",
                "to my list", "on my list", "for me", "please", "task", "todo", "item",
            ];
            if FILLER.contains(&rest) {
                return true;
            }
        }
    }
    false
}

fn confirm_label_for(intent: &str) -> String {
    match intent {
        "DELETE_TODO" => "Delete task".to_string(),
        "CLEAR_COMPLETED_TODOS" => "Clear tasks".to_string(),
        "OPEN_APPLICATION" => "Open app".to_string(),
        "OPEN_FILE" => "Open file".to_string(),
        "OPEN_FOLDER" => "Open folder".to_string(),
        "SHUTDOWN_PC" => "Shut down".to_string(),
        _ => "Confirm".to_string(),
    }
}

/// A confirmation prompt that names what is about to happen.
fn describe_confirmation(intent: &str, params: &Value) -> String {
    let s = |key: &str| params.get(key).and_then(|v| v.as_str()).unwrap_or("");
    match intent {
        "OPEN_APPLICATION" => format!(
            "Nova will open {}. Do you want me to continue?",
            s("application")
        ),
        "OPEN_FILE" => format!(
            "Nova will open the file {}. Do you want me to continue?",
            s("path")
        ),
        "OPEN_FOLDER" => format!(
            "Nova will open the folder {}. Do you want me to continue?",
            s("folder")
        ),
        "CLEAR_COMPLETED_TODOS" => {
            "This will remove every completed task from your list. Do you want me to continue?"
                .to_string()
        }
        "SHUTDOWN_PC" => {
            let delay = params
                .get("delaySeconds")
                .and_then(|v| v.as_i64())
                .unwrap_or(10);
            format!(
                "Nova will shut down the computer in {delay} seconds. Open apps may lose unsaved work. \
                 Do you want me to continue?"
            )
        }
        _ => "This action needs your confirmation. Do you want me to continue?".to_string(),
    }
}

/// What Nova actually did, in one line, for the activity log.
fn describe_action(intent: &str, params: &Value, success: bool) -> String {
    if !success {
        return "Nothing changed.".to_string();
    }
    let s = |key: &str| params.get(key).and_then(|v| v.as_str()).unwrap_or("");
    match intent {
        "ADD_TODO" => "Saved the task to your list.".to_string(),
        "LIST_TODOS" => "Read out your open tasks.".to_string(),
        "SUMMARIZE_TODOS" => "Summarized the tasks due in that period.".to_string(),
        "COMPLETE_TODO" => "Marked the task as completed.".to_string(),
        "EDIT_TODO" => "Updated the task.".to_string(),
        "DELETE_TODO" => "Removed the task from your list.".to_string(),
        "CLEAR_COMPLETED_TODOS" => "Removed the completed tasks.".to_string(),
        "SEARCH_TODOS" => "Searched your task list.".to_string(),
        "OPEN_APPLICATION" => format!("Launched {}.", s("application")),
        "OPEN_FILE" => format!("Opened {} with its default app.", s("path")),
        "OPEN_FOLDER" => format!("Opened File Explorer at {}.", s("folder")),
        "TAKE_SCREENSHOT" => "Saved a screenshot to Pictures.".to_string(),
        "SHUTDOWN_PC" => "Started the shutdown countdown.".to_string(),
        "CANCEL_SHUTDOWN" => "Cancelled the shutdown countdown.".to_string(),
        _ => "Completed the command.".to_string(),
    }
}

enum DeletePreview {
    Found(String),
    Ambiguous(String, Vec<String>),
    NotFound,
}

fn preview_delete<R: Runtime>(app: &AppHandle<R>, params: &Value) -> DeletePreview {
    use crate::skills::todo::{resolve_todo, ResolvedTodo};

    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock poisoned");
    let search = params.get("searchQuery").and_then(|v| v.as_str());
    let id = params.get("todoId").and_then(|v| v.as_i64());

    match resolve_todo(&conn, search, id) {
        Ok(ResolvedTodo::Unique(todo)) => DeletePreview::Found(todo.title),
        Ok(ResolvedTodo::Ambiguous(candidates)) => {
            let titles: Vec<String> = candidates
                .iter()
                .map(|t| format!("'{}'", truncate_for_speech(&t.title, 60)))
                .collect();
            DeletePreview::Ambiguous(
                format!(
                    "I found several matching tasks: {}. Which one did you mean?",
                    titles.join(", ")
                ),
                titles,
            )
        }
        Ok(ResolvedTodo::None) => DeletePreview::NotFound,
        Err(_) => DeletePreview::NotFound,
    }
}

// ---------------------------------------------------------------------------
// Confirmation parsing
// ---------------------------------------------------------------------------

/// Parse a yes/no answer for a pending confirmation.
pub fn parse_confirmation(text: &str) -> Option<bool> {
    const YES: &[&str] = &[
        "yes", "yeah", "yep", "yup", "ya", "sure", "ok", "okay", "confirm", "confirmed", "do it",
        "do that", "go ahead", "go for it", "proceed", "continue", "absolutely", "please do",
        "yes please", "affirmative", "correct", "sounds good",
    ];
    const NO: &[&str] = &[
        "no", "nope", "nah", "negative", "cancel", "stop", "abort", "don't", "dont",
        "never mind", "nevermind", "no thanks", "no thank you", "dont do it", "don't do it",
        "do not", "leave it",
    ];

    let normalized = normalize_text(text);
    if normalized.is_empty() {
        return None;
    }
    let matches = |phrases: &[&str]| {
        phrases.iter().any(|phrase| {
            normalized == *phrase
                || normalized
                    .strip_prefix(phrase)
                    .map(|rest| rest.starts_with(' ') && rest.trim().len() <= 12)
                    .unwrap_or(false)
        })
    };

    if matches(YES) {
        return Some(true);
    }
    if matches(NO) {
        return Some(false);
    }
    None
}

// ---------------------------------------------------------------------------
// Turn finalization: conversation context, activity log, UI events
// ---------------------------------------------------------------------------

/// Row written for one interaction.
struct ActivityRecord<'a> {
    transcript: &'a str,
    intent: Option<&'a str>,
    parameters: Option<&'a str>,
    outcome: &'a Outcome,
    duration_ms: Option<i64>,
    data: &'a Value,
    provider: Option<&'a str>,
}

/// Persist one interaction to the activity log; returns the new row id.
fn log_activity<R: Runtime>(app: &AppHandle<R>, record: ActivityRecord<'_>) -> Option<i64> {
    let state = app.state::<AppState>();
    let conn = state.db.lock().ok()?;
    let mut result_obj = record.data.clone();
    if let Some(provider) = record.provider {
        if let Some(obj) = result_obj.as_object_mut() {
            obj.insert("provider".to_string(), json!(provider));
        }
    }
    let result_json = result_obj.to_string();
    let label = record.intent.map(registry::label_for);
    database::insert_activity(
        &conn,
        NewActivity {
            transcript: record.transcript,
            intent: record.intent,
            parameters: record.parameters,
            result: Some(&result_json),
            success: record.outcome.success,
            response: &record.outcome.response,
            duration_ms: record.duration_ms,
            status: record.outcome.status,
            intent_label: label.as_deref(),
            understood: record.outcome.understood.as_deref(),
            action: record.outcome.action.as_deref(),
            retryable: record.outcome.retryable,
        },
    )
    .ok()
}

#[allow(clippy::too_many_arguments)]
fn finalize_turn<R: Runtime>(
    app: &AppHandle<R>,
    session: &mut AgentSession,
    transcript: &str,
    intent: Option<&str>,
    params: Option<&Value>,
    data: Option<&Value>,
    outcome: &Outcome,
    started: Instant,
    provider: Option<&str>,
) -> Option<i64> {
    let state = app.state::<AppState>();
    let behaviour = state.behaviour_snapshot();

    session.context.push(Exchange {
        transcript: transcript.to_string(),
        intent: intent.map(|s| s.to_string()),
        success: outcome.success,
        summary: truncate_for_speech(&outcome.response, 80),
        response: outcome.response.clone(),
    });
    if let Some(intent) = intent {
        session.context.last_intent = Some(intent.to_string());
    }

    // Verbosity shapes what Nova says back; confirmation prompts and questions
    // are never shortened because the user has to act on them.
    let is_prompt = matches!(
        outcome.status,
        database::STATUS_AWAITING_CONFIRMATION | database::STATUS_AWAITING_CLARIFICATION
    );
    let response = if is_prompt {
        outcome.response.clone()
    } else {
        behaviour
            .response_verbosity
            .render(&outcome.response, outcome.understood.as_deref())
    };

    let empty = json!({});
    let data = data.unwrap_or(&empty);
    let params_json = params.map(|p| p.to_string());
    // The log stores exactly what the user saw/was told (verbosity applied).
    let spoken = Outcome {
        status: outcome.status,
        success: outcome.success,
        response: response.clone(),
        understood: outcome.understood.clone(),
        action: outcome.action.clone(),
        retryable: outcome.retryable,
    };
    let activity_id = log_activity(
        app,
        ActivityRecord {
            transcript,
            intent,
            parameters: params_json.as_deref(),
            outcome: &spoken,
            duration_ms: Some(started.elapsed().as_millis() as i64),
            data,
            provider,
        },
    );

    session.last = Some(LastTurn {
        normalized: normalize_text(transcript),
        response: response.clone(),
        at: Instant::now(),
    });

    if is_prompt {
        // Prompts are reported through the confirm channel; the overlay shows
        // the question rather than a result.
        events::emit_nova(app, events::EVT_ACTIVITY, json!({}));
        return activity_id;
    }

    // Push to the UI.
    events::emit_nova(
        app,
        events::EVT_RESULT,
        ResultPayload {
            intent: intent.map(|s| s.to_string()),
            label: intent.map(registry::label_for),
            success: outcome.success,
            status: outcome.status.to_string(),
            response,
            retryable: outcome.retryable,
            activity_id,
        },
    );
    events::emit_nova(app, events::EVT_ACTIVITY, json!({}));
    activity_id
}

fn emit_command<R: Runtime>(app: &AppHandle<R>, intent: &str, label: &str, params: &Value) {
    events::emit_nova(
        app,
        events::EVT_COMMAND,
        CommandPayload {
            intent: intent.to_string(),
            label: label.to_string(),
            summary: registry::summarize(intent, params),
        },
    );
}

fn emit_confirm<R: Runtime>(
    app: &AppHandle<R>,
    pending: bool,
    prompt: Option<&str>,
    confirm_label: Option<&str>,
    kind: Option<&str>,
) {
    events::emit_nova(
        app,
        events::EVT_CONFIRM,
        ConfirmPayload {
            pending,
            prompt: prompt.map(|s| s.to_string()),
            confirm_label: confirm_label.map(|s| s.to_string()),
            kind: kind.map(|s| s.to_string()),
            options: None,
        },
    );
}

/// Ask the user to pick between several matching tasks.
fn emit_clarification<R: Runtime>(app: &AppHandle<R>, prompt: &str, options: &[String]) {
    events::emit_nova(
        app,
        events::EVT_CONFIRM,
        ConfirmPayload {
            pending: true,
            prompt: Some(prompt.to_string()),
            confirm_label: Some("Use this one".to_string()),
            kind: Some("clarification".to_string()),
            options: Some(options.to_vec()),
        },
    );
}

fn set_awaiting<R: Runtime>(app: &AppHandle<R>, awaiting: bool) {
    crate::runtime::set_awaiting_confirmation(app, awaiting);
}

fn set_awaiting_clarification<R: Runtime>(app: &AppHandle<R>, awaiting: bool) {
    crate::runtime::set_awaiting_clarification(app, awaiting);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_yes_and_no() {
        assert_eq!(parse_confirmation("yes"), Some(true));
        assert_eq!(parse_confirmation("Yeah, do it"), Some(true));
        assert_eq!(parse_confirmation("go ahead please"), Some(true));
        assert_eq!(parse_confirmation("no"), Some(false));
        assert_eq!(parse_confirmation("No thanks"), Some(false));
        assert_eq!(parse_confirmation("cancel that"), Some(false));
        assert_eq!(parse_confirmation("open spotify"), None);
        assert_eq!(parse_confirmation(""), None);
    }

    #[test]
    fn confirmation_prompts_name_the_action() {
        assert!(
            describe_confirmation("OPEN_FOLDER", &json!({"folder": "Downloads"}))
                .contains("Downloads")
        );
        assert!(describe_confirmation("CLEAR_COMPLETED_TODOS", &json!({}))
            .contains("completed task"));
        assert_eq!(confirm_label_for("DELETE_TODO"), "Delete task");
    }

    #[test]
    fn action_descriptions_are_honest_about_failures() {
        assert_eq!(describe_action("ADD_TODO", &json!({}), false), "Nothing changed.");
        assert!(describe_action("OPEN_FOLDER", &json!({"folder": "Downloads"}), true)
            .contains("Downloads"));
    }
}
