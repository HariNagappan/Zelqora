//! Strict validation of structured commands before execution.
//!
//! Validation is also *normalizing*: field values are trimmed and canonicalized
//! (priority, scope, ISO due dates) so skills never re-parse user text.

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Map, Value};

use crate::commands::registry::{self, CommandDef};
use crate::commands::{CommandRequest, RiskLevel};
use crate::error::{NovaError, NovaResult};

/// A command that passed validation: known intent, sane + normalized parameters.
#[derive(Debug, Clone)]
pub struct ValidatedCommand {
    pub intent: String,
    pub label: String,
    pub risk: RiskLevel,
    pub params: Value,
}

pub fn validate(request: &CommandRequest) -> NovaResult<ValidatedCommand> {
    let def = registry::get(&request.intent).ok_or_else(|| {
        NovaError::Unsupported(format!("unknown command '{}'", request.intent))
    })?;

    let mut params = match request.parameters.clone() {
        Value::Null => Value::Object(Map::new()),
        Value::Object(map) => Value::Object(map),
        _ => {
            return Err(NovaError::InvalidCommand(
                "parameters must be a JSON object".into(),
            ))
        }
    };

    reject_unknown_fields(def, &params)?;
    normalize(def, &mut params)?;
    check_required(def, &params)?;

    Ok(ValidatedCommand {
        intent: def.name.to_string(),
        label: def.label.to_string(),
        risk: def.risk,
        params,
    })
}

fn reject_unknown_fields(def: &CommandDef, params: &Value) -> NovaResult<()> {
    let Some(obj) = params.as_object() else {
        return Ok(());
    };
    let Some(schema) = def.parameters_schema().get("properties").cloned() else {
        return Ok(());
    };
    let Some(allowed) = schema.as_object() else {
        return Ok(());
    };
    for key in obj.keys() {
        if !allowed.contains_key(key) {
            return Err(NovaError::InvalidCommand(format!(
                "'{}' does not accept a parameter named '{}'",
                def.name, key
            )));
        }
    }
    Ok(())
}

/// Drop empty strings, trim everything, canonicalize enums and due dates.
fn normalize(def: &CommandDef, params: &mut Value) -> NovaResult<()> {
    let Some(obj) = params.as_object_mut() else {
        return Ok(());
    };

    // Trim string fields and remove empty ones (an empty string means "unset").
    let keys: Vec<String> = obj.keys().cloned().collect();
    for key in keys {
        let Some(value) = obj.get(&key) else { continue };
        if value.is_null() {
            obj.remove(&key);
            continue;
        }
        if let Some(text) = value.as_str() {
            let text = text.trim().to_string();
            if text.is_empty() {
                obj.remove(&key);
            } else {
                obj.insert(key, Value::String(text));
            }
        }
    }

    if obj.contains_key("priority") {
        let priority = obj
            .get("priority")
            .and_then(|v| v.as_str())
            .map(|v| v.to_ascii_lowercase())
            .unwrap_or_default();
        match priority.as_str() {
            "low" | "1" => {
                obj.insert("priority".into(), json!("low"));
            }
            "medium" | "med" | "normal" | "2" => {
                obj.insert("priority".into(), json!("medium"));
            }
            "high" | "urgent" | "3" => {
                obj.insert("priority".into(), json!("high"));
            }
            other => {
                return Err(NovaError::InvalidCommand(format!(
                    "'{}' received an unsupported priority '{other}'",
                    def.name
                )))
            }
        }
    }

    if obj.contains_key("due") {
        let raw = obj.get("due").and_then(|v| v.as_str()).unwrap_or_default();
        obj.insert("due".into(), json!(parse_due(raw, def.name)?));
    }

    if obj.contains_key("scope") {
        let scope = obj
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or("today")
            .to_ascii_lowercase();
        let canonical = match scope.as_str() {
            "today" | "now" => "today",
            "tomorrow" => "tomorrow",
            "week" | "this week" | "next week" | "7 days" => "week",
            "overdue" | "late" | "past due" => "overdue",
            "all" | "everything" | "every" => "all",
            other => {
                return Err(NovaError::InvalidCommand(format!(
                    "'{}' received an unsupported scope '{other}'",
                    def.name
                )))
            }
        };
        obj.insert("scope".into(), json!(canonical));
    }

    if obj.contains_key("category") {
        let category = obj
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if category.chars().count() > 40 {
            return Err(NovaError::InvalidCommand(
                "'category' is limited to 40 characters".into(),
            ));
        }
    }

    if obj.contains_key("title") {
        let len = obj
            .get("title")
            .and_then(|v| v.as_str())
            .map(|t| t.chars().count())
            .unwrap_or(0);
        if len > 300 {
            return Err(NovaError::InvalidCommand(
                "'title' is limited to 300 characters".into(),
            ));
        }
    }

    Ok(())
}

/// Parse and sanity-check a due date. Accepts a full RFC 3339 timestamp (what
/// the offline parser and the cloud model produce); anything else is refused
/// rather than guessed.
pub fn parse_due(raw: &str, intent: &str) -> NovaResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(NovaError::InvalidCommand(format!(
            "'{intent}' received an empty due date"
        )));
    }
    let parsed = DateTime::parse_from_rfc3339(trimmed)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            // A bare date is accepted as midnight UTC.
            chrono::NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
                .map(|date| date.and_hms_opt(0, 0, 0).unwrap().and_utc())
        })
        .map_err(|_| {
            NovaError::InvalidCommand(format!(
                "'{intent}' received a due date that is not an ISO-8601 timestamp: '{trimmed}'"
            ))
        })?;

    // Guard against model hallucination: dates far outside a plausible range.
    let now = Utc::now();
    if parsed < now - Duration::days(365 * 5) || parsed > now + Duration::days(365 * 10) {
        return Err(NovaError::InvalidCommand(format!(
            "'{intent}' received a due date outside the supported range: '{trimmed}'"
        )));
    }
    Ok(parsed.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

fn nonempty(params: &Value, key: &str) -> bool {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

fn check_required(def: &CommandDef, params: &Value) -> NovaResult<()> {
    let missing =
        |what: &str| Err(NovaError::InvalidCommand(format!("'{}' requires {what}", def.name)));

    match def.name {
        "ADD_TODO" => {
            if !nonempty(params, "title") {
                return missing("a non-empty 'title'");
            }
        }
        "SEARCH_TODOS" => {
            if !nonempty(params, "query") {
                return missing("a non-empty 'query'");
            }
        }
        "OPEN_APPLICATION" => {
            if !nonempty(params, "application") {
                return missing("a non-empty 'application'");
            }
        }
        "OPEN_FILE" => {
            if !nonempty(params, "path") {
                return missing("a non-empty 'path'");
            }
        }
        "OPEN_FOLDER" => {
            if !nonempty(params, "folder") {
                return missing("a non-empty 'folder'");
            }
        }
        "COMPLETE_TODO" | "DELETE_TODO" => {
            if !has_target(params) {
                return missing("'searchQuery' or 'todoId'");
            }
        }
        "EDIT_TODO" => {
            if !has_target(params) {
                return missing("'searchQuery' or 'todoId'");
            }
            let clears_due = params
                .get("clearDue")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let has_change = ["title", "due", "priority", "category"]
                .iter()
                .any(|key| nonempty(params, key))
                || clears_due;
            if !has_change {
                return missing("at least one change ('title', 'due', 'priority' or 'category')");
            }
        }
        _ => {}
    }
    Ok(())
}

fn has_target(params: &Value) -> bool {
    let has_query = nonempty(params, "searchQuery");
    let has_id = params
        .get("todoId")
        .and_then(|v| v.as_i64())
        .map(|v| v > 0)
        .unwrap_or(false);
    has_query || has_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::registry;
    use serde_json::json;

    fn request(intent: &str, params: Value) -> CommandRequest {
        CommandRequest {
            intent: intent.into(),
            parameters: params,
        }
    }

    #[test]
    fn accepts_valid_commands() {
        let v = validate(&request("ADD_TODO", json!({"title": "buy milk"}))).unwrap();
        assert_eq!(v.intent, "ADD_TODO");
        assert_eq!(v.label, "Add task");
        assert_eq!(v.risk, RiskLevel::Safe);

        let v = validate(&request("DELETE_TODO", json!({"searchQuery": "milk"}))).unwrap();
        assert_eq!(v.risk, RiskLevel::ConfirmationRequired);
    }

    #[test]
    fn rejects_unknown_intent() {
        assert!(validate(&request("GO_NUCLEAR", json!({}))).is_err());
    }

    #[test]
    fn rejects_bad_params() {
        assert!(validate(&request("ADD_TODO", json!({}))).is_err());
        assert!(validate(&request("ADD_TODO", json!({"title": "  "}))).is_err());
        assert!(validate(&request("ADD_TODO", json!({"title": "x", "evil": 1}))).is_err());
        assert!(validate(&request("COMPLETE_TODO", json!({}))).is_err());
        assert!(validate(&request("LIST_TODOS", json!([1, 2]))).is_err());
    }

    #[test]
    fn normalizes_due_priority_and_scope() {
        let v = validate(&request(
            "ADD_TODO",
            json!({"title": "  finish DBMS  ", "priority": "HIGH", "due": "2026-10-11T18:00:00Z", "category": " College "}),
        ))
        .unwrap();
        assert_eq!(v.params["title"], "finish DBMS");
        assert_eq!(v.params["priority"], "high");
        assert_eq!(v.params["category"], "College");
        assert_eq!(v.params["due"], "2026-10-11T18:00:00.000Z");

        let v = validate(&request("ADD_TODO", json!({"title": "x", "due": "2026-10-11"}))).unwrap();
        assert_eq!(v.params["due"], "2026-10-11T00:00:00.000Z");

        let v = validate(&request("SUMMARIZE_TODOS", json!({"scope": "This Week"}))).unwrap();
        assert_eq!(v.params["scope"], "week");
        let v = validate(&request("SUMMARIZE_TODOS", json!({}))).unwrap();
        assert!(v.params.get("scope").is_none(), "defaults applied by the skill");
    }

    #[test]
    fn rejects_unparseable_due_dates_and_bad_enums() {
        assert!(validate(&request("ADD_TODO", json!({"title": "x", "due": "next tuesday"}))).is_err());
        assert!(validate(&request("ADD_TODO", json!({"title": "x", "due": "1999-01-01T00:00:00Z"}))).is_err());
        assert!(validate(&request("ADD_TODO", json!({"title": "x", "priority": "extreme"}))).is_err());
        assert!(validate(&request("SUMMARIZE_TODOS", json!({"scope": "last year"}))).is_err());
        assert!(validate(&request("ADD_TODO", json!({"title": "x", "category": "c".repeat(41).as_str()}))).is_err());
    }

    #[test]
    fn edit_requires_a_target_and_a_change() {
        assert!(validate(&request("EDIT_TODO", json!({"searchQuery": "milk"}))).is_err());
        assert!(validate(&request("EDIT_TODO", json!({"title": "new text"}))).is_err());
        assert!(validate(&request("EDIT_TODO", json!({"searchQuery": "milk", "title": "oat milk"}))).is_ok());
        assert!(validate(&request("EDIT_TODO", json!({"todoId": 4, "priority": "low"}))).is_ok());
        assert!(validate(&request("EDIT_TODO", json!({"todoId": 0, "priority": "low"}))).is_err());
    }

    #[test]
    fn blocked_commands_are_registered_but_flagged() {
        let def = registry::get("EXECUTE_SHELL_COMMAND").unwrap();
        assert_eq!(def.risk, RiskLevel::Blocked);
        let v = validate(&request("EXECUTE_SHELL_COMMAND", json!({}))).unwrap();
        assert_eq!(v.risk, RiskLevel::Blocked);
    }
}
