//! Dispatches validated commands to the skill implementations.
//! The executor only ever calls predefined safe operations — there is no path
//! from model output to arbitrary shell or script execution.

use serde_json::json;
use tauri::{AppHandle, Runtime};

use crate::commands::SkillOutcome;
use crate::error::NovaResult;
use crate::skills;

/// Execute a validated command.
///
/// `source` is `voice` | `text` | `onboarding` and is recorded on created tasks
/// so the Tasks screen can mark voice-captured items.
pub fn execute<R: Runtime>(
    app: &AppHandle<R>,
    intent: &str,
    params: &serde_json::Value,
    source: &str,
) -> NovaResult<SkillOutcome> {
    match intent {
        "ADD_TODO" => skills::todo::add(app, params, source),
        "LIST_TODOS" => skills::todo::list(app, params),
        "SUMMARIZE_TODOS" => skills::todo::summarize(app, params),
        "COMPLETE_TODO" => skills::todo::complete(app, params),
        "EDIT_TODO" => skills::todo::edit(app, params),
        "DELETE_TODO" => skills::todo::delete(app, params),
        "CLEAR_COMPLETED_TODOS" => skills::todo::clear_completed(app, params),
        "SEARCH_TODOS" => skills::todo::search(app, params),
        "OPEN_APPLICATION" => skills::windows::open_application(app, params),
        "OPEN_FILE" => skills::windows::open_file(app, params),
        "OPEN_FOLDER" => skills::windows::open_folder(app, params),
        "TAKE_SCREENSHOT" => skills::windows::take_screenshot(app, params),
        "SHUTDOWN_PC" => skills::windows::shutdown_pc(app, params),
        "CANCEL_SHUTDOWN" => skills::windows::cancel_shutdown(app, params),
        // Registered but always refused — defense in depth (the validator
        // already marks these BLOCKED before we get here).
        "RUN_SCRIPT" | "EXECUTE_SHELL_COMMAND" => Ok(SkillOutcome {
            success: false,
            response: "I can't run scripts or shell commands.".into(),
            data: json!({ "blocked": true }),
        }),
        other => Ok(SkillOutcome {
            success: false,
            response: "I don't know how to do that yet.".into(),
            data: json!({ "unknownIntent": other }),
        }),
    }
}

#[cfg(test)]
mod tests {
    use crate::commands::registry;

    #[test]
    fn every_non_blocked_registry_intent_has_an_execution_path() {
        // The match arms above are the authority; keep them in sync with the
        // registry so a command can never be advertised without an executor.
        const HANDLED: &[&str] = &[
            "ADD_TODO",
            "LIST_TODOS",
            "SUMMARIZE_TODOS",
            "COMPLETE_TODO",
            "EDIT_TODO",
            "DELETE_TODO",
            "CLEAR_COMPLETED_TODOS",
            "SEARCH_TODOS",
            "OPEN_APPLICATION",
            "OPEN_FILE",
            "OPEN_FOLDER",
            "TAKE_SCREENSHOT",
            "SHUTDOWN_PC",
            "CANCEL_SHUTDOWN",
            "RUN_SCRIPT",
            "EXECUTE_SHELL_COMMAND",
        ];
        for def in registry::COMMANDS {
            assert!(
                HANDLED.contains(&def.name),
                "{} is registered but has no executor arm",
                def.name
            );
        }
    }
}
