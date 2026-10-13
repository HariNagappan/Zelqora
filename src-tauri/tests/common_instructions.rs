//! End-to-end verification of the instructions a person actually gives Nova.
//!
//! The session is driven through the *real* orchestrator — interpretation,
//! validation, risk gate, skill execution, activity log — on Tauri's mock
//! runtime, against an in-memory database, so it never touches the user's own
//! tasks. Speech recognition is deliberately not involved: the same entry point
//! the microphone pipeline uses (`handle_transcript`) is fed typed text, which
//! is exactly what `process_text` does for the dashboard and the overlay.
//!
//! Run it with output:
//!   cargo test --manifest-path src-tauri/Cargo.toml --test common_instructions -- --nocapture

use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;

use nova_lib::agent::{handle_transcript, AgentTurnResult};
use nova_lib::database::{self, STATUS_AWAITING_CONFIRMATION, STATUS_CANCELLED, STATUS_NOT_HEARD};
use nova_lib::events::TranscriptSource;
use nova_lib::settings::{LlmProviderKind, Settings};
use nova_lib::state::AppState;

type TestApp = tauri::App<tauri::test::MockRuntime>;

fn test_app() -> TestApp {
    let mut settings = Settings::default();
    settings.llm_provider = LlmProviderKind::Offline; // no network in tests
    settings.tts_enabled = false;
    settings.confirm_destructive = true;

    let app = mock_builder()
        .build(mock_context(noop_assets()))
        .expect("failed to build the mock app");
    let conn = database::open_in_memory().expect("open in-memory database");
    app.manage(AppState::new(conn, settings));
    app
}

/// Say something as if it had been typed in the dashboard or the overlay.
fn say(app: &TestApp, text: &str) -> AgentTurnResult {
    let handle = app.handle().clone();
    tauri::async_runtime::block_on(handle_transcript(&handle, text, TranscriptSource::Text))
}

/// Print one line of evidence and return the result.
fn step(app: &TestApp, instruction: &str) -> AgentTurnResult {
    let result = say(app, instruction);
    println!(
        "\n» {instruction}\n  ← [{}] {}{}",
        result.status,
        result.response,
        if result.awaiting_confirmation {
            "  (waiting for confirmation)"
        } else if result.awaiting_clarification {
            "  (waiting for an answer)"
        } else {
            ""
        }
    );
    result
}

fn todos(app: &TestApp) -> Vec<database::Todo> {
    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock");
    database::list_todos(&conn, None).expect("list todos")
}

#[test]
fn a_realistic_session_of_common_instructions() {
    let app = test_app();
    println!("\n════ Nova · common instructions verification ════");

    // 1 ── capture a task
    let added = step(&app, "add buy milk to my list");
    assert!(added.success, "add failed: {}", added.response);
    assert_eq!(
        added.command.as_ref().map(|c| c.intent.as_str()),
        Some("ADD_TODO")
    );
    assert_eq!(todos(&app).len(), 1);

    // 2 ── capture a task with a due date, priority and category
    let detailed = step(
        &app,
        "add finish the DBMS assignment to my to-dos due today at 6 pm high priority for college",
    );
    assert!(detailed.success, "{}", detailed.response);
    let stored = todos(&app);
    let dbms = stored
        .iter()
        .find(|task| task.title.contains("DBMS"))
        .expect("the DBMS task should exist");
    assert_eq!(dbms.priority, "high");
    assert_eq!(dbms.category, "College");
    assert!(dbms.due_at.is_some(), "a due date should have been stored");
    println!(
        "  · stored: “{}” · {} priority · {} · due {}",
        dbms.title,
        dbms.priority,
        dbms.category,
        dbms.due_at.clone().unwrap_or_default()
    );

    // 3 ── recall the list
    let listed = step(&app, "what's on my todo list");
    assert!(listed.success && listed.response.contains("buy milk"));

    // 4 ── summarise the day
    let summary = step(&app, "what do I need to finish today");
    assert!(summary.success, "{}", summary.response);
    assert!(
        summary.response.contains("DBMS"),
        "summary missed today's task: {}",
        summary.response
    );

    // 5 ── complete something
    let completed = step(&app, "mark buy milk as completed");
    assert!(completed.success, "{}", completed.response);
    assert!(todos(&app).iter().any(|task| task.completed));

    // 6 ── search
    let found = step(&app, "search my todos for milk");
    assert!(found.success && found.response.contains("milk"));

    // 7 ── edit an existing task
    let edited = step(&app, "set the DBMS assignment to low priority");
    assert!(edited.success, "{}", edited.response);
    assert_eq!(
        todos(&app)
            .iter()
            .find(|task| task.title.contains("DBMS"))
            .map(|task| task.priority.clone()),
        Some("low".to_string())
    );

    // 8 ── a destructive action asks first, and cancelling changes nothing
    let asked = step(&app, "delete the DBMS assignment");
    assert!(asked.awaiting_confirmation, "{}", asked.response);
    assert!(asked.response.to_lowercase().contains("delete"));

    let cancelled = step(&app, "no thanks");
    assert!(cancelled.success, "{}", cancelled.response);
    assert!(
        cancelled.response.to_lowercase().contains("cancelled"),
        "{}",
        cancelled.response
    );
    assert_eq!(todos(&app).len(), 2, "cancelling must not delete anything");

    // 9 ── the same request, confirmed
    let asked_again = step(&app, "delete the DBMS assignment");
    assert!(asked_again.awaiting_confirmation);
    let confirmed = step(&app, "yes");
    assert!(confirmed.success, "{}", confirmed.response);
    assert_eq!(todos(&app).len(), 1, "only the completed milk task remains");

    // 10 ── a request Nova cannot do is refused honestly
    let unsupported = step(&app, "play some music");
    assert!(!unsupported.success);
    assert!(
        unsupported.response.contains("don't know"),
        "{}",
        unsupported.response
    );

    // 11 ── web search is explicitly not supported
    let web = step(&app, "search the web for laptops");
    assert!(!web.success && web.status == "unsupported", "{}", web.response);

    // 12 ── an unfinished "add" becomes a question, not a refusal
    let dangling = step(&app, "can you add a");
    assert!(dangling.awaiting_clarification, "{}", dangling.response);
    let answered = step(&app, "buy bread");
    assert!(answered.success, "{}", answered.response);
    assert!(todos(&app).iter().any(|task| task.title == "buy bread"));

    // 13 ── the same submission twice is not executed twice
    let first = say(&app, "add water the plants to my list");
    let second = say(&app, "add water the plants to my list");
    assert!(first.success && second.success);
    assert!(
        second.response.contains("already handled"),
        "{}",
        second.response
    );
    assert_eq!(
        todos(&app)
            .iter()
            .filter(|task| task.title.contains("water the plants"))
            .count(),
        1
    );
    println!("\n» (repeated) “add water the plants to my list”\n  ← [{}] {}", second.status, second.response);

    // 14 ── desktop integrations
    let shot = step(&app, "take a screenshot");
    assert!(shot.success, "screenshot failed: {}", shot.response);
    assert!(shot.response.to_lowercase().contains("screenshot"));

    let folder = step(&app, "open my downloads folder");
    assert_eq!(
        folder.command.as_ref().map(|c| c.intent.as_str()),
        Some("OPEN_FOLDER")
    );
    assert!(folder.success, "{}", folder.response);

    // 15 ── a blocked capability stays blocked
    let blocked = step(&app, "run a script");
    assert!(!blocked.success);

    // 16 ── nothing heard (the state an empty voice capture produces)
    let nothing = step(&app, "");
    assert_eq!(nothing.status, STATUS_NOT_HEARD, "{}", nothing.response);

    // ── the activity log tells the same story ────────────────────────────────
    let state = app.state::<AppState>();
    let conn = state.db.lock().expect("db lock");
    let entries = database::list_activity(&conn, 100, 0).expect("activity");
    let count = |status: &str| entries.iter().filter(|e| e.status == status).count();
    println!(
        "\n── activity log: {} entries · {} success · {} unsupported · {} cancelled · {} awaiting confirmation · {} not heard",
        entries.len(),
        count("success"),
        count("unsupported"),
        count(STATUS_CANCELLED),
        count(STATUS_AWAITING_CONFIRMATION),
        count(STATUS_NOT_HEARD),
    );
    assert!(count("success") >= 10, "expected successful commands");
    assert!(count("unsupported") >= 2, "unsupported requests must be logged");
    assert!(count(STATUS_CANCELLED) >= 1, "the cancellation must be logged");
    assert!(
        count(STATUS_AWAITING_CONFIRMATION) >= 2,
        "both confirmations must be logged"
    );
    assert_eq!(count(STATUS_NOT_HEARD), 1);
    println!("════ session verified ════\n");
}
