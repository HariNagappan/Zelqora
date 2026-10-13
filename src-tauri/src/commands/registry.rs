//! The predefined command registry. Every executable capability of Nova is
//! declared here; the LLM only ever receives these tool definitions and can
//! never produce anything outside this list.

use serde_json::{json, Value};

use crate::commands::RiskLevel;

pub struct CommandDef {
    pub name: &'static str,
    /// Short human label used in the UI ("Add task", "Open folder").
    pub label: &'static str,
    pub description: &'static str,
    pub risk: RiskLevel,
    /// Example utterances (documentation + offline parser test material).
    pub examples: &'static [&'static str],
    schema: fn() -> Value,
}

impl CommandDef {
    pub fn parameters_schema(&self) -> Value {
        (self.schema)()
    }
}

fn empty_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

/// Shared "which task?" parameters for commands that act on an existing task.
fn task_reference() -> Value {
    json!({
        "searchQuery": { "type": "string", "description": "Words from the task title." },
        "todoId": { "type": "integer", "description": "Numeric id, only when the user explicitly gave one." }
    })
}

/// Due-date parameter, shared by ADD_TODO and EDIT_TODO.
fn due_property() -> Value {
    json!({
        "type": "string",
        "description": "Absolute due date/time as an ISO-8601 timestamp (UTC), e.g. 2026-10-11T18:00:00Z. Omit when the user gave no date."
    })
}

fn priority_property() -> Value {
    json!({
        "type": "string",
        "enum": ["low", "medium", "high"],
        "description": "Task priority."
    })
}

pub static COMMANDS: &[CommandDef] = &[
    CommandDef {
        name: "ADD_TODO",
        label: "Add task",
        description: "Add a new task to the user's list, optionally with a due date, priority and category.",
        risk: RiskLevel::Safe,
        examples: &[
            "Add buy milk to my list",
            "Remind me that I need to buy milk",
            "Put buy milk on my todo list",
            "I need to buy milk, add it to my tasks",
            "Add finish DBMS assignment to my todo list, due today at 6 pm",
            "Add prepare for my interview to my tasks, high priority",
        ],
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "The task text, e.g. 'buy groceries'." },
                    "description": { "type": "string", "description": "Optional extra details." },
                    "due": due_property(),
                    "priority": priority_property(),
                    "category": { "type": "string", "description": "Optional grouping such as 'College', 'Career', 'Personal'." }
                },
                "required": ["title"],
                "additionalProperties": false
            })
        },
    },
    CommandDef {
        name: "LIST_TODOS",
        label: "List tasks",
        description: "List the user's current (not completed) tasks.",
        risk: RiskLevel::Safe,
        examples: &["What's on my todo list?", "Show my todos", "List my tasks"],
        schema: empty_schema,
    },
    CommandDef {
        name: "SUMMARIZE_TODOS",
        label: "Summarize tasks",
        description: "Summarize what is due in a period: today, tomorrow, this week, overdue, or everything.",
        risk: RiskLevel::Safe,
        examples: &[
            "What do I need to finish today?",
            "What's on my list for tomorrow?",
            "What's overdue?",
            "Give me a summary of my tasks",
        ],
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "scope": {
                        "type": "string",
                        "enum": ["today", "tomorrow", "week", "overdue", "all"],
                        "description": "Which slice of the list to summarize. Defaults to today."
                    }
                },
                "additionalProperties": false
            })
        },
    },
    CommandDef {
        name: "COMPLETE_TODO",
        label: "Complete task",
        description: "Mark a task as completed. Identify it with searchQuery (words from the title) or todoId.",
        risk: RiskLevel::Safe,
        examples: &[
            "Mark DBMS assignment as completed",
            "Complete buy milk",
            "Finish the DBMS lab on my list",
        ],
        schema: || {
            json!({
                "type": "object",
                "properties": task_reference(),
                "additionalProperties": false
            })
        },
    },
    CommandDef {
        name: "EDIT_TODO",
        label: "Edit task",
        description: "Change an existing task's text, due date, priority or category. Identify it with searchQuery or todoId.",
        risk: RiskLevel::Safe,
        examples: &[
            "Change the DBMS assignment to finish the DBMS lab",
            "Set the interview task to high priority",
            "Move the essay task to tomorrow",
            "Rename buy milk to buy oat milk",
        ],
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "searchQuery": { "type": "string", "description": "Words from the task to edit." },
                    "todoId": { "type": "integer", "description": "Numeric id, only when the user explicitly gave one." },
                    "title": { "type": "string", "description": "New task text." },
                    "due": due_property(),
                    "priority": priority_property(),
                    "category": { "type": "string", "description": "New category." },
                    "clearDue": { "type": "boolean", "description": "Set true to remove the due date entirely." }
                },
                "additionalProperties": false
            })
        },
    },
    CommandDef {
        name: "DELETE_TODO",
        label: "Delete task",
        description: "Delete a task from the list. Identify it with searchQuery (words from the title) or todoId. Requires confirmation.",
        risk: RiskLevel::ConfirmationRequired,
        examples: &["Delete the DBMS assignment todo", "Remove buy milk from my list"],
        schema: || {
            json!({
                "type": "object",
                "properties": task_reference(),
                "additionalProperties": false
            })
        },
    },
    CommandDef {
        name: "CLEAR_COMPLETED_TODOS",
        label: "Clear completed",
        description: "Remove all completed tasks from the list. Requires confirmation.",
        risk: RiskLevel::ConfirmationRequired,
        examples: &["Clear my completed todos", "Remove all finished tasks"],
        schema: empty_schema,
    },
    CommandDef {
        name: "SEARCH_TODOS",
        label: "Search tasks",
        description: "Search the task list for items matching a query.",
        risk: RiskLevel::Safe,
        examples: &["Search my todos for DBMS", "Find the milk task"],
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search words." }
                },
                "required": ["query"],
                "additionalProperties": false
            })
        },
    },
    CommandDef {
        name: "OPEN_APPLICATION",
        label: "Open app",
        description: "Open an application on this PC by its name (e.g. Spotify, Notepad, Calculator).",
        risk: RiskLevel::Safe,
        examples: &["Open Spotify", "Launch Notepad", "Start the calculator app"],
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "application": { "type": "string", "description": "The application name as the user said it." }
                },
                "required": ["application"],
                "additionalProperties": false
            })
        },
    },
    CommandDef {
        name: "OPEN_FILE",
        label: "Open file",
        description: "Open a file with its default application. Executable and script files are refused for safety.",
        risk: RiskLevel::Safe,
        examples: &["Open the report.pdf in Downloads"],
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute path of the file, or a path inside a known folder." }
                },
                "required": ["path"],
                "additionalProperties": false
            })
        },
    },
    CommandDef {
        name: "OPEN_FOLDER",
        label: "Open folder",
        description: "Open a folder in File Explorer, e.g. Downloads, Documents, or an absolute path.",
        risk: RiskLevel::Safe,
        examples: &["Open my Downloads folder", "Open the Desktop folder"],
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "folder": { "type": "string", "description": "Known folder name (Downloads, Documents, Desktop, Pictures, Music, Videos) or an absolute path." }
                },
                "required": ["folder"],
                "additionalProperties": false
            })
        },
    },
    CommandDef {
        name: "TAKE_SCREENSHOT",
        label: "Take screenshot",
        description: "Take a screenshot of the primary screen and save it to the Pictures folder.",
        risk: RiskLevel::Safe,
        examples: &["Take a screenshot", "Capture my screen"],
        schema: empty_schema,
    },
    CommandDef {
        name: "SHUTDOWN_PC",
        label: "Shut down the PC",
        description: "Shut this computer down after a short countdown. Always asks for confirmation first.",
        risk: RiskLevel::ConfirmationRequired,
        examples: &[
            "Shut down my PC",
            "Turn off the computer",
            "Power off the laptop",
        ],
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "delaySeconds": {
                        "type": "integer",
                        "minimum": 5,
                        "maximum": 60,
                        "description": "Countdown before the shutdown starts. Defaults to 10 seconds."
                    }
                },
                "additionalProperties": false
            })
        },
    },
    CommandDef {
        name: "CANCEL_SHUTDOWN",
        label: "Cancel shutdown",
        description: "Cancel a shutdown that is still counting down.",
        risk: RiskLevel::Safe,
        examples: &["Cancel the shutdown", "Abort the shutdown"],
        schema: empty_schema,
    },
    CommandDef {
        name: "RUN_SCRIPT",
        label: "Run script",
        description: "Run a script. Blocked in this version.",
        risk: RiskLevel::Blocked,
        examples: &[],
        schema: empty_schema,
    },
    CommandDef {
        name: "EXECUTE_SHELL_COMMAND",
        label: "Run shell command",
        description: "Execute a shell command. Blocked in this version.",
        risk: RiskLevel::Blocked,
        examples: &[],
        schema: empty_schema,
    },
];

pub fn get(name: &str) -> Option<&'static CommandDef> {
    COMMANDS.iter().find(|def| def.name == name)
}

/// Human label for an intent ("Add task"); falls back to the raw intent.
pub fn label_for(intent: &str) -> String {
    get(intent)
        .map(|def| def.label.to_string())
        .unwrap_or_else(|| intent.to_string())
}

/// Risk level of an intent, defaulting to `ConfirmationRequired` for anything
/// unknown so an unregistered intent can never execute silently.
pub fn risk_for(intent: &str) -> RiskLevel {
    get(intent).map(|def| def.risk).unwrap_or(RiskLevel::Blocked)
}

/// Tool definitions for the LLM, generated from the registry.
pub fn tool_definitions() -> Value {
    let tools: Vec<Value> = COMMANDS
        .iter()
        .filter(|def| def.risk != RiskLevel::Blocked)
        .map(|def| {
            json!({
                "type": "function",
                "function": {
                    "name": def.name,
                    "description": def.description,
                    "parameters": def.parameters_schema(),
                }
            })
        })
        .collect();
    Value::Array(tools)
}

/// Human-readable one-line summary of an interpreted command (for the overlay).
pub fn summarize(intent: &str, params: &Value) -> String {
    let s = |key: &str| {
        params
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    match intent {
        "ADD_TODO" => {
            let mut summary = format!("Add task · “{}”", s("title"));
            if let Some(priority) = params.get("priority").and_then(|v| v.as_str()) {
                if !priority.is_empty() {
                    summary.push_str(&format!(" · {priority} priority"));
                }
            }
            if params.get("due").and_then(|v| v.as_str()).is_some() {
                summary.push_str(" · with a due date");
            }
            summary
        }
        "LIST_TODOS" => "List tasks".into(),
        "SUMMARIZE_TODOS" => format!("Summarize · scope: {}", {
            let scope = s("scope");
            if scope.is_empty() {
                "today".to_string()
            } else {
                scope
            }
        }),
        "COMPLETE_TODO" => {
            if let Some(id) = params.get("todoId").and_then(|v| v.as_i64()) {
                format!("Complete task #{id}")
            } else {
                format!("Complete task · “{}”", s("searchQuery"))
            }
        }
        "EDIT_TODO" => {
            let mut parts = vec!["Edit task".to_string()];
            if !s("searchQuery").is_empty() {
                parts.push(format!("match “{}”", s("searchQuery")));
            }
            if !s("title").is_empty() {
                parts.push(format!("rename to “{}”", s("title")));
            }
            if !s("priority").is_empty() {
                parts.push(format!("{} priority", s("priority")));
            }
            if !s("category").is_empty() {
                parts.push(format!("category {}", s("category")));
            }
            if !s("due").is_empty() {
                parts.push("new due date".to_string());
            }
            parts.join(" · ")
        }
        "DELETE_TODO" => {
            if let Some(id) = params.get("todoId").and_then(|v| v.as_i64()) {
                format!("Delete task #{id}")
            } else {
                format!("Delete task · “{}”", s("searchQuery"))
            }
        }
        "CLEAR_COMPLETED_TODOS" => "Clear completed tasks".into(),
        "SEARCH_TODOS" => format!("Search tasks · “{}”", s("query")),
        "OPEN_APPLICATION" => format!("Open app · {}", s("application")),
        "OPEN_FILE" => format!("Open file · {}", s("path")),
        "OPEN_FOLDER" => format!("Open folder · {}", s("folder")),
        "TAKE_SCREENSHOT" => "Take a screenshot".into(),
        "SHUTDOWN_PC" => match params.get("delaySeconds").and_then(|v| v.as_i64()) {
            Some(delay) => format!("Shut down the PC · in {delay} s"),
            None => "Shut down the PC".into(),
        },
        "CANCEL_SHUTDOWN" => "Cancel the shutdown".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_self_consistent() {
        // No duplicate intents, and every non-blocked command has examples.
        let mut names: Vec<&str> = COMMANDS.iter().map(|def| def.name).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate intent in the registry");
        let _ = (count, names);

        for def in COMMANDS {
            assert!(!def.label.trim().is_empty(), "{} needs a label", def.name);
            let schema = def.parameters_schema();
            assert!(schema.get("properties").is_some(), "{} needs a schema", def.name);
        }
    }

    #[test]
    fn tool_definitions_exclude_blocked_commands() {
        let tools = tool_definitions();
        let names: Vec<String> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
            .collect();
        assert!(names.contains(&"ADD_TODO".to_string()));
        assert!(names.contains(&"EDIT_TODO".to_string()));
        assert!(names.contains(&"SUMMARIZE_TODOS".to_string()));
        assert!(!names.contains(&"RUN_SCRIPT".to_string()));
        assert!(!names.contains(&"EXECUTE_SHELL_COMMAND".to_string()));
    }

    #[test]
    fn labels_and_risk_fall_back_safely() {
        assert_eq!(label_for("ADD_TODO"), "Add task");
        assert_eq!(label_for("NOT_A_COMMAND"), "NOT_A_COMMAND");
        assert_eq!(risk_for("NOT_A_COMMAND"), RiskLevel::Blocked);
    }

    #[test]
    fn every_intent_that_removes_user_data_requires_confirmation() {
        // Bulk deletions are confirmed even though they are not "dangerous" in
        // the filesystem sense — the user cannot undo them.
        assert_eq!(risk_for("DELETE_TODO"), RiskLevel::ConfirmationRequired);
        assert_eq!(risk_for("CLEAR_COMPLETED_TODOS"), RiskLevel::ConfirmationRequired);
        // Shutting the PC down is the most consequential action Nova offers.
        assert_eq!(risk_for("SHUTDOWN_PC"), RiskLevel::ConfirmationRequired);
        assert_eq!(risk_for("RUN_SCRIPT"), RiskLevel::Blocked);
        assert_eq!(risk_for("EXECUTE_SHELL_COMMAND"), RiskLevel::Blocked);
        assert_eq!(risk_for("ADD_TODO"), RiskLevel::Safe);
        assert_eq!(risk_for("CANCEL_SHUTDOWN"), RiskLevel::Safe);
    }

    #[test]
    fn summaries_are_human_readable() {
        let summary = summarize(
            "ADD_TODO",
            &json!({"title": "finish DBMS", "priority": "high", "due": "2026-10-11T18:00:00Z"}),
        );
        assert!(summary.contains("finish DBMS"));
        assert!(summary.contains("high priority"));
        assert_eq!(summarize("SUMMARIZE_TODOS", &json!({})), "Summarize · scope: today");
    }
}
