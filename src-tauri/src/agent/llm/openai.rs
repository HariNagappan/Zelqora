//! OpenAI-compatible chat-completions provider with strict tool calling.
//! The model may ONLY emit tool calls from the command registry; anything
//! else is rejected without side effects. One bounded repair attempt is made
//! for malformed output.

use std::time::Duration;

use serde_json::{json, Value};

use crate::commands::{registry, CommandRequest};
use crate::error::{NovaError, NovaResult};
use crate::settings::Settings;

use super::super::context::ConversationContext;
use super::{sanitize_json_text, Interpretation};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(25);

pub async fn interpret(
    settings: &Settings,
    text: &str,
    context: &ConversationContext,
) -> NovaResult<Option<Interpretation>> {
    let Some(key) = settings.resolved_api_key() else {
        return Err(NovaError::MissingApiKey);
    };
    let base_url = settings.resolved_base_url();
    let model = settings.resolved_model();

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| NovaError::Network(e.to_string()))?;

    let mut messages: Vec<Value> = vec![json!({
        "role": "system",
        "content": system_prompt(context),
    })];
    messages.push(json!({ "role": "user", "content": text }));

    // Up to two attempts: the second is a bounded repair for malformed output.
    for attempt in 0..2 {
        let body = json!({
            "model": model,
            "messages": messages,
            "tools": registry::tool_definitions(),
            "tool_choice": "auto",
            "temperature": 0.1,
            "max_tokens": 400,
        });

        let response = client
            .post(format!("{base_url}/chat/completions"))
            .bearer_auth(&key)
            .json(&body)
            .send()
            .await
            .map_err(|e| NovaError::Network(format!("could not reach the AI service: {e}")))?;

        let status = response.status();
        let payload: Value = response
            .json()
            .await
            .map_err(|e| NovaError::Network(format!("invalid response from the AI service: {e}")))?;

        if !status.is_success() {
            let detail = payload
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            return Err(NovaError::Network(format!(
                "AI service error ({status}): {detail}"
            )));
        }

        match parse_response(&payload) {
            ParseOutcome::Command(command) => {
                return Ok(Some(Interpretation {
                    command,
                    provider: "cloud",
                }))
            }
            ParseOutcome::NoMatch => return Ok(None),
            ParseOutcome::Invalid => {
                if attempt == 0 {
                    messages.push(json!({
                        "role": "assistant",
                        "content": "(unparseable response)"
                    }));
                    messages.push(json!({
                        "role": "user",
                        "content": "Your previous reply could not be used. Respond with exactly ONE tool call from the provided tools. If you cannot help with the request, say so briefly."
                    }));
                    continue;
                }
                return Err(NovaError::InvalidCommand(
                    "the AI returned an unusable response".into(),
                ));
            }
        }
    }

    Err(NovaError::InvalidCommand(
        "the AI returned an unusable response".into(),
    ))
}

enum ParseOutcome {
    Command(CommandRequest),
    /// The model responded with plain text (e.g. "I can't do that") — no tool call.
    NoMatch,
    /// Could not be parsed into a usable command.
    Invalid,
}

fn parse_response(payload: &Value) -> ParseOutcome {
    let Some(message) = payload
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
    else {
        return ParseOutcome::Invalid;
    };

    if let Some(calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
        let Some(call) = calls.first() else {
            return ParseOutcome::Invalid;
        };
        let Some(name) = call
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
        else {
            return ParseOutcome::Invalid;
        };
        // Unknown tools are rejected outright — never executed.
        if registry::get(name).is_none() {
            return ParseOutcome::Invalid;
        }
        let args_raw = call
            .get("function")
            .and_then(|f| f.get("arguments"))
            .and_then(|a| a.as_str())
            .unwrap_or("{}");
        let Some(parameters) = sanitize_json_text(args_raw) else {
            return ParseOutcome::Invalid;
        };
        return ParseOutcome::Command(CommandRequest {
            intent: name.to_string(),
            parameters,
        });
    }

    // Some gateways ignore `tools`; accept a strict JSON command in content.
    if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return ParseOutcome::Invalid;
        }
        if let Some(value) = sanitize_json_text(trimmed) {
            if let Ok(request) = serde_json::from_value::<CommandRequest>(value) {
                if registry::get(&request.intent).is_some() {
                    return ParseOutcome::Command(request);
                }
            }
        }
        // Plain prose — treated as the model saying it cannot help.
        return ParseOutcome::NoMatch;
    }

    ParseOutcome::Invalid
}

fn system_prompt(context: &ConversationContext) -> String {
    let mut prompt = String::from(
        "You are Nova, a Windows desktop voice assistant. Convert the user's spoken request into exactly ONE tool call from the provided tools.\n\
         Rules:\n\
         - Call exactly one tool per request. Never invent tools; use only the provided ones.\n\
         - Extract parameters faithfully from the user's words; do not add information the user did not say.\n\
         - If the user refers to a previous todo or app with words like \"it\", \"that\", \"the other one\", use the recent context below.\n\
         - If the request cannot be fulfilled with the available tools, reply briefly with plain text (no tool call).\n\
         - Do not describe the call; just make it.\n",
    );
    let summary = context.compact_summary();
    if !summary.is_empty() {
        prompt.push_str("\nRecent context:\n");
        prompt.push_str(&summary);
    }
    prompt
}
