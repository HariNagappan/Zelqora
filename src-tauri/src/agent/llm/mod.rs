//! Interpretation of natural language into structured commands.
//!
//! Two providers share this facade:
//! * `openai` — OpenAI-compatible chat completions with tool calling (cloud).
//! * `offline` — deterministic, generalized rule-based matching (no network).

pub mod offline;
pub mod openai;
pub mod slots;

use serde_json::Value;

use crate::commands::CommandRequest;
use crate::error::NovaResult;
use crate::settings::{LlmProviderKind, Settings};

use super::context::ConversationContext;

pub struct Interpretation {
    pub command: CommandRequest,
    pub provider: &'static str,
}

/// Interpret an utterance with the configured provider. When a cloud provider
/// fails but the offline parser understands the request, the offline result is
/// used and clearly marked in the activity log.
pub async fn interpret(
    settings: &Settings,
    text: &str,
    context: &ConversationContext,
) -> NovaResult<Option<Interpretation>> {
    match settings.effective_llm_provider() {
        LlmProviderKind::Cloud => match openai::interpret(settings, text, context).await {
            Ok(Some(interpretation)) => Ok(Some(interpretation)),
            Ok(None) => Ok(None),
            Err(err) => {
                eprintln!("[nova] cloud interpretation failed: {err}");
                if let Some(command) = offline::interpret(text, context) {
                    Ok(Some(Interpretation {
                        command,
                        provider: "offline (cloud unavailable)",
                    }))
                } else {
                    Err(err)
                }
            }
        },
        _ => Ok(offline::interpret(text, context).map(|command| Interpretation {
            command,
            provider: "offline",
        })),
    }
}

/// Extract the first JSON object from arbitrary model output (handles code
/// fences and surrounding prose). Returns `None` when no parseable object
/// exists — callers must NOT execute anything in that case.
pub fn sanitize_json_text(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    let without_fences = strip_code_fences(trimmed);
    let candidate = extract_first_json_object(without_fences)?;
    serde_json::from_str(candidate).ok()
}

fn strip_code_fences(text: &str) -> &str {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("```") {
        // Skip the language tag on the first line, if any.
        let rest = match rest.find('\n') {
            Some(idx) => &rest[idx + 1..],
            None => rest,
        };
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
        return rest.trim();
    }
    t
}

/// Scan for the first balanced `{...}` object (string-aware).
fn extract_first_json_object(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let mut start = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, &byte) in bytes.iter().enumerate() {
        let ch = byte as char;
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => {
                if start.is_none() {
                    start = Some(idx);
                }
                depth += 1;
            }
            '}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(s) = start {
                            return Some(&text[s..=idx]);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_plain_json() {
        let value = sanitize_json_text(r#"{"intent":"LIST_TODOS","parameters":{}}"#).unwrap();
        assert_eq!(value["intent"], "LIST_TODOS");
    }

    #[test]
    fn extracts_json_from_fences_and_prose() {
        let raw = "Sure!\n```json\n{\"intent\": \"ADD_TODO\", \"parameters\": {\"title\": \"milk\"}}\n```\nDone.";
        let value = sanitize_json_text(raw).unwrap();
        assert_eq!(value["intent"], "ADD_TODO");
        assert_eq!(value["parameters"]["title"], "milk");
    }

    #[test]
    fn handles_nested_braces_and_strings() {
        let raw = r#"prefix {"a": {"b": "}"}, "c": 1} suffix"#;
        let value = sanitize_json_text(raw).unwrap();
        assert_eq!(value["c"], 1);
    }

    #[test]
    fn rejects_garbage() {
        assert!(sanitize_json_text("absolutely no json here").is_none());
        assert!(sanitize_json_text("{broken").is_none());
        assert!(sanitize_json_text("").is_none());
    }

    #[test]
    fn ignores_unrelated_json_shapes() {
        // An array is not a command object.
        assert!(sanitize_json_text("[1, 2, 3]").is_none());
        let _ = json!(null);
    }
}
