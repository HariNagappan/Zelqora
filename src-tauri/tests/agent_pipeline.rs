//! End-to-end agent pipeline tests on Tauri's mock runtime.
//!
//! These exercise the REAL orchestrator glue — not just its pieces:
//! text → interpretation (offline rules) → validation → risk gate →
//! skill execution → response → activity log — including the confirmation
//! flow. No window, microphone or network is involved.

use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;

use nova_lib::agent::{handle_transcript, AgentTurnResult};
use nova_lib::database;
use nova_lib::events::TranscriptSource;
use nova_lib::settings::{LlmProviderKind, Settings};
use nova_lib::state::AppState;

type TestApp = tauri::App<tauri::test::MockRuntime>;

fn test_app() -> TestApp {
    let mut settings = Settings::default();
    // Deterministic and offline for tests.
    settings.llm_provider = LlmProviderKind::Offline;
    settings.tts_enabled = false;
    settings.confirm_destructive = true;

    let app = mock_builder()
        .build(mock_context(noop_assets()))
        .expect("failed to build the mock app");

    let conn = database::open_in_memory().expect("open in-memory database");
    app.manage(AppState::new(conn, settings));
    app
}

fn say(app: &TestApp, text: &str) -> AgentTurnResult {
    let handle = app.handle().clone();
    tauri::async_runtime::block_on(handle_transcript(&handle, text, TranscriptSource::Text))
}

/// Same pipeline, but tagged as a spoken command — this is the path the audio
/// pipeline uses, and it is what marks tasks as voice-created.
fn say_voice(app: &TestApp, text: &str) -> AgentTurnResult {
    let handle = app.handle().clone();
    tauri::async_runtime::block_on(handle_transcript(&handle, text, TranscriptSource::Voice))
}

#[test]
fn add_list_complete_delete_confirmation_flow() {
    let app = test_app();

    let added = say(&app, "add buy milk to my list");
    assert!(added.success, "add failed: {}", added.response);
    assert!(
        added.response.to_lowercase().contains("buy milk"),
        "unexpected add response: {}",
        added.response
    );

    let listed = say(&app, "what's on my todo list");
    assert!(
        listed.response.to_lowercase().contains("buy milk"),
        "list missed the todo: {}",
        listed.response
    );

    // Pronoun resolution: "it" refers to the todo we just added.
    let completed = say(&app, "complete it");
    assert!(completed.success, "complete failed: {}", completed.response);
    assert!(
        completed.response.to_lowercase().contains("completed"),
        "unexpected complete response: {}",
        completed.response
    );

    let added = say(&app, "add finish the DBMS lab to my tasks");
    assert!(added.success, "second add failed: {}", added.response);

    // Destructive action must stop at the confirmation gate first.
    let deleting = say(&app, "delete the DBMS lab todo");
    assert!(
        deleting.awaiting_confirmation,
        "expected a confirmation prompt, got: {}",
        deleting.response
    );
    assert!(
        deleting.response.to_lowercase().contains("delete"),
        "unexpected confirmation prompt: {}",
        deleting.response
    );

    let confirmed = say(&app, "yes");
    assert!(confirmed.success, "confirmation failed: {}", confirmed.response);
    assert!(
        confirmed.response.to_lowercase().contains("deleted"),
        "unexpected delete response: {}",
        confirmed.response
    );

    // Database state: only the completed milk todo remains.
    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock");
    let todos = database::list_todos(&conn, None).unwrap();
    assert_eq!(todos.len(), 1, "unexpected todos: {todos:?}");
    assert!(todos[0].completed, "expected the remaining todo completed");
    assert!(todos[0].title.to_lowercase().contains("milk"));

    // Activity log captured the successful intents.
    let entries = database::list_activity(&conn, 100, 0).unwrap();
    for intent in ["ADD_TODO", "COMPLETE_TODO", "DELETE_TODO"] {
        assert!(
            entries
                .iter()
                .any(|e| e.intent.as_deref() == Some(intent) && e.success),
            "missing successful {intent} in the activity log"
        );
    }
}

#[test]
fn delete_can_be_cancelled() {
    let app = test_app();

    say(&app, "add call the bank to my list");

    let deleting = say(&app, "delete the bank todo");
    assert!(
        deleting.awaiting_confirmation,
        "expected confirmation, got: {}",
        deleting.response
    );

    let cancelled = say(&app, "no thanks");
    assert!(cancelled.success, "{}", cancelled.response);
    assert!(
        cancelled.response.to_lowercase().contains("cancelled"),
        "unexpected cancellation response: {}",
        cancelled.response
    );

    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock");
    let todos = database::list_todos(&conn, None).unwrap();
    assert_eq!(todos.len(), 1, "cancelled delete must not remove the todo");
}

#[test]
fn unknown_requests_get_a_safe_response_and_are_logged() {
    let app = test_app();

    let reply = say(&app, "make me a sandwich");
    assert!(!reply.success);
    assert!(
        reply.response.contains("don't know"),
        "unexpected response: {}",
        reply.response
    );

    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock");
    let entries = database::list_activity(&conn, 10, 0).unwrap();
    assert_eq!(entries.len(), 1, "interaction should be logged");
    assert!(entries[0].intent.is_none());
    assert!(!entries[0].success);
}

#[test]
fn spec_phrasings_all_land_in_the_same_capability() {
    let app = test_app();

    for phrase in [
        "Add buy milk to my list",
        "Remind me that I need to buy milk",
        "Put buy milk on my todo list",
        "I need to buy milk, add it to my tasks",
    ] {
        let reply = say(&app, phrase);
        assert!(reply.success, "{phrase} -> {}", reply.response);
        assert_eq!(
            reply.command.as_ref().map(|c| c.intent.as_str()),
            Some("ADD_TODO"),
            "phrase did not resolve to ADD_TODO: {phrase}"
        );
    }

    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock");
    assert_eq!(
        database::list_todos(&conn, None).unwrap().len(),
        4,
        "every phrasing should have created a todo"
    );
}

#[test]
fn voice_capture_stores_due_date_priority_and_category() {
    let app = test_app();

    let reply = say_voice(
        &app,
        "Add finish my DBMS assignment to my to-dos due today at 6 pm high priority for college",
    );
    assert!(reply.success, "{}", reply.response);

    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock");
    let todos = database::list_todos(&conn, None).unwrap();
    assert_eq!(todos.len(), 1);
    let task = &todos[0];
    assert_eq!(task.title, "finish my DBMS assignment");
    assert_eq!(task.priority, "high");
    assert_eq!(task.category, "College");
    assert!(task.due_at.is_some(), "a due date should have been parsed");
    assert_eq!(task.source, "voice");
}

#[test]
fn summarize_today_reports_what_is_due() {
    let app = test_app();
    say(&app, "add finish the DBMS lab to my to-dos due today at 5 pm");
    say(&app, "add call the dentist tomorrow");

    let reply = say(&app, "what do I need to finish today");
    assert!(reply.success, "{}", reply.response);
    assert_eq!(
        reply.command.as_ref().map(|c| c.intent.as_str()),
        Some("SUMMARIZE_TODOS")
    );
    assert!(
        reply.response.contains("DBMS lab"),
        "the summary should name today's task: {}",
        reply.response
    );

    let tomorrow = say(&app, "what's on my list for tomorrow");
    assert!(
        tomorrow.response.contains("dentist"),
        "tomorrow's summary missed the task: {}",
        tomorrow.response
    );
}

#[test]
fn edit_task_by_voice_changes_priority_and_title() {
    let app = test_app();
    say(&app, "add prepare for my technical interview to my tasks");

    let prioritized = say(&app, "set the interview task to high priority");
    assert!(prioritized.success, "{}", prioritized.response);

    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock");
    let tasks = database::list_todos(&conn, None).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].priority, "high");
    drop(conn);

    let renamed = say(&app, "rename the interview task to prepare for the system design round");
    assert!(renamed.success, "{}", renamed.response);

    let conn = state.db.lock().expect("db lock");
    let tasks = database::list_todos(&conn, None).unwrap();
    assert_eq!(tasks[0].title, "prepare for the system design round");
    assert_eq!(tasks[0].priority, "high", "the earlier edit must survive");
}

#[test]
fn ambiguous_delete_asks_before_touching_anything() {
    let app = test_app();
    say(&app, "add buy milk to my list");
    say(&app, "add buy bread to my list");

    let asked = say(&app, "delete the buy todo");
    assert!(
        asked.awaiting_clarification,
        "expected a clarification question, got: {}",
        asked.response
    );
    assert!(
        asked.response.to_lowercase().contains("which one"),
        "unexpected question: {}",
        asked.response
    );

    let state = app.state::<AppState>();
    {
        let conn = state.db.lock().expect("db lock");
        assert_eq!(database::list_todos(&conn, None).unwrap().len(), 2, "nothing deleted yet");
    }

    // Answer the question: the follow-up is used as the task reference, which
    // then still goes through the confirmation gate.
    let targeted = say(&app, "milk");
    assert!(
        targeted.awaiting_confirmation,
        "expected the delete confirmation, got: {}",
        targeted.response
    );
    let confirmed = say(&app, "yes");
    assert!(confirmed.success, "{}", confirmed.response);

    let conn = state.db.lock().expect("db lock");
    let remaining = database::list_todos(&conn, None).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].title, "buy bread");
}

#[test]
fn dangling_add_request_asks_for_the_task() {
    let app = test_app();

    let asked = say(&app, "Can you add a");
    assert!(
        asked.awaiting_clarification,
        "expected a question, got: {}",
        asked.response
    );
    assert!(
        asked.response.to_lowercase().contains("what would you like"),
        "unexpected question: {}",
        asked.response
    );

    let state = app.state::<AppState>();
    {
        let conn = state.db.lock().expect("db lock");
        assert!(
            database::list_todos(&conn, None).unwrap().is_empty(),
            "nothing may be created before the answer"
        );
    }

    // The answer supplies the missing title and the task is created.
    let answered = say(&app, "buy oat milk");
    assert!(answered.success, "{}", answered.response);
    let conn = state.db.lock().expect("db lock");
    let todos = database::list_todos(&conn, None).unwrap();
    assert_eq!(todos.len(), 1);
    assert_eq!(todos[0].title, "buy oat milk");
}

#[test]
fn activation_after_an_error_starts_a_new_interaction() {
    let app = test_app();

    // An unsupported request leaves the assistant in a terminal state.
    let unsupported = say(&app, "make me a sandwich");
    assert!(!unsupported.success);

    // The next command must still work (the offline path is the same one the
    // wake-word pipeline feeds).
    let recovered = say(&app, "add water the plants to my list");
    assert!(recovered.success, "{}", recovered.response);
}

#[test]
fn repeated_submission_is_not_executed_twice() {
    let app = test_app();

    let first = say(&app, "add book the library slot to my list");
    assert!(first.success, "{}", first.response);
    let second = say(&app, "add book the library slot to my list");
    assert!(second.success, "{}", second.response);
    assert!(
        second.response.contains("already handled"),
        "the repeat should be reported, not re-run: {}",
        second.response
    );

    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock");
    let todos = database::list_todos(&conn, None).unwrap();
    assert_eq!(todos.len(), 1, "the task must exist exactly once");
}

#[test]
fn blocked_intents_never_execute() {
    let app = test_app();

    // The offline parser refuses these outright, so they are reported as
    // unsupported rather than reaching the executor.
    let reply = say(&app, "run a script");
    assert!(!reply.success);
    assert!(
        reply.response.contains("don't know") || reply.response.contains("can't"),
        "unexpected response: {}",
        reply.response
    );
}

#[test]
fn confirmation_prompts_are_logged_as_awaiting() {
    let app = test_app();
    say(&app, "add clear the desk to my list");

    let asked = say(&app, "delete the desk todo");
    assert!(asked.awaiting_confirmation, "{}", asked.response);

    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock");
    let entries = database::list_activity(&conn, 10, 0).unwrap();
    assert!(
        entries
            .iter()
            .any(|entry| entry.status == database::STATUS_AWAITING_CONFIRMATION),
        "the pending confirmation should be visible in the activity log"
    );
}
