//! Nova — application bootstrap: modules, plugins, state, tray and the
//! invoke handlers exposed to the dashboard and overlay.

pub mod agent;
pub mod appearance;
pub mod audio;
pub mod commands;
pub mod database;
pub mod error;
pub mod events;
pub mod onboarding;
pub mod preferences;
pub mod runtime;
pub mod secrets;
pub mod settings;
pub mod skills;
pub mod state;
pub mod stt;
pub mod tts;
pub mod util;
pub mod wakeword;
pub mod windows;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Manager, State};

use crate::appearance::Appearance;
use crate::database as db;
use crate::error::{NovaError, NovaResult};
use crate::events::{MicrophoneStatus, RuntimeEvent, ServiceState, TranscriptSource};
use crate::onboarding::Onboarding;
use crate::preferences::BehaviourPrefs;
use crate::settings::Settings;
use crate::state::AppState;

pub fn run() {
    // Load `.env` (optional; Settings takes precedence for the provider value).
    let _ = dotenvy::dotenv();

    tauri::Builder::default()
        // Single instance must be the first plugin.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            crate::runtime::show_main(app, Some("home"));
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        app.state::<AppState>()
                            .send_event(RuntimeEvent::PushToTalkPressed);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();

            let db_path = db::default_db_path()?;
            let conn = db::open(&db_path)?;
            let settings = db::load_settings(&conn)?;
            app.manage(AppState::new(conn, settings));

            crate::runtime::init(&handle)?;

            // Show the dashboard unless launched hidden — and always on the
            // very first run so onboarding can happen.
            let onboarding_complete = {
                let state = handle.state::<AppState>();
                let conn = state.db.lock().expect("db lock poisoned");
                let onboarding = db::load_json_or_default::<Onboarding>(
                    &conn,
                    crate::onboarding::ONBOARDING_KEY,
                )?;
                onboarding.completed
            };
            let hidden = std::env::args().any(|arg| arg == "--hidden")
                || handle.state::<AppState>().settings_snapshot().start_minimized;
            if !hidden || !onboarding_complete {
                crate::runtime::show_main(&handle, None);
                if !onboarding_complete {
                    crate::runtime::show_main(&handle, Some("onboarding"));
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // status + settings
            get_status,
            get_settings,
            update_settings,
            set_provider_credential,
            clear_provider_credential,
            get_behaviour,
            update_behaviour,
            get_appearance,
            update_appearance,
            reset_appearance,
            get_onboarding,
            update_onboarding,
            reset_onboarding,
            // audio + devices
            list_microphones,
            list_voices,
            get_microphone_status,
            start_microphone_test,
            stop_microphone_test,
            open_microphone_settings,
            download_model,
            // tasks
            list_todos,
            list_tasks,
            get_task_counts,
            get_todo,
            create_todo,
            create_task,
            update_todo,
            update_task,
            set_todo_completed,
            delete_todo,
            clear_completed_todos,
            search_todos,
            get_today_overview,
            // activity
            list_activity,
            list_activity_view,
            get_activity_counts,
            clear_activity_history,
            retry_activity,
            // runtime + desktop
            activate,
            set_paused,
            set_service_state,
            confirm_pending,
            process_text,
            cancel_current,
            show_floating_assistant,
            hide_floating_assistant,
            set_overlay_size,
            open_main,
            quit_nova,
            get_diagnostics,
            check_for_updates,
            open_logs_folder,
            open_documentation,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Nova")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    // Keep running in the tray when the windows are closed.
                    api.prevent_exit();
                }
            }
        });
}

// ---------------------------------------------------------------------------
// Status + settings
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatusInfo {
    state: String,
    detail: Option<String>,
    paused: bool,
    service: String,
    wake_enabled: bool,
    wake_word: String,
    model_ready: bool,
    model_path: Option<String>,
    tts_enabled: bool,
    stt_provider: crate::settings::SttProvider,
    llm_provider: crate::settings::LlmProviderKind,
    llm_configured: bool,
    microphone: MicrophoneStatus,
    overlay_mode: String,
    overlay_enabled: bool,
    theme: String,
    onboarding_complete: bool,
    version: String,
    /// How the stored provider credential is protected at rest.
    credential_protection: String,
}

#[tauri::command]
fn get_status(state: State<'_, AppState>) -> StatusInfo {
    let settings = state.settings_snapshot();
    let ctl = state.ctl.lock().expect("ctl lock poisoned");
    let db = state.db.lock().ok();
    let onboarding_complete = db
        .as_ref()
        .map(|conn| {
            db::load_json_or_default::<Onboarding>(conn, crate::onboarding::ONBOARDING_KEY)
                .map(|o| o.completed)
                .unwrap_or(false)
        })
        .unwrap_or(false);

    StatusInfo {
        state: ctl.state.as_str().to_string(),
        detail: ctl.state_detail.clone(),
        paused: ctl.paused,
        service: ctl.service.as_str().to_string(),
        wake_enabled: settings.wake_enabled,
        wake_word: settings.wake_word.clone(),
        model_ready: ctl.model_ready,
        model_path: ctl.model_path.as_ref().map(|p| p.display().to_string()),
        tts_enabled: settings.tts_enabled,
        stt_provider: settings.stt_provider,
        llm_provider: settings.llm_provider,
        llm_configured: settings.has_api_key(),
        microphone: ctl.microphone.clone(),
        overlay_mode: ctl.overlay_mode.clone(),
        overlay_enabled: settings.overlay_enabled,
        theme: state.appearance_snapshot().theme,
        onboarding_complete,
        version: env!("CARGO_PKG_VERSION").to_string(),
        credential_protection: if crate::secrets::available() {
            "Windows DPAPI (current user)".to_string()
        } else {
            "unprotected (unsupported platform)".to_string()
        },
    }
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    // The stored provider credential never leaves the backend; the UI only
    // learns whether one is configured (see `StatusInfo::llm_configured`).
    let mut settings = state.settings_snapshot();
    settings.openai_api_key.clear();
    settings
}

/// Partial update from the Settings screen: every field is optional and only
/// what the UI sent is changed, so an edit can never wipe unrelated values
/// (including the provider credential, which is write-only — see
/// `set_provider_credential`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct SettingsPatch {
    #[serde(default)]
    wake_word: Option<String>,
    #[serde(default)]
    wake_enabled: Option<bool>,
    #[serde(default)]
    sensitivity: Option<f32>,
    /// Empty string selects the system default device.
    #[serde(default)]
    microphone: Option<String>,
    #[serde(default)]
    push_to_talk_hotkey: Option<String>,
    #[serde(default)]
    stt_provider: Option<crate::settings::SttProvider>,
    #[serde(default)]
    tts_enabled: Option<bool>,
    /// Empty string selects the system default voice.
    #[serde(default)]
    tts_voice: Option<String>,
    #[serde(default)]
    tts_rate: Option<f32>,
    #[serde(default)]
    tts_volume: Option<f32>,
    #[serde(default)]
    llm_provider: Option<crate::settings::LlmProviderKind>,
    #[serde(default)]
    openai_base_url: Option<String>,
    #[serde(default)]
    openai_model: Option<String>,
    #[serde(default)]
    confirm_destructive: Option<bool>,
    #[serde(default)]
    autostart: Option<bool>,
    #[serde(default)]
    start_minimized: Option<bool>,
    #[serde(default)]
    overlay_enabled: Option<bool>,
}

fn optional_device(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Clamp every user-editable field into its supported range before persisting,
/// so the UI can never store a value the runtime cannot honour.
fn sanitize_settings(mut settings: Settings) -> Settings {
    settings.sensitivity = settings.sensitivity.clamp(0.0, 1.0);
    settings.tts_rate = settings.tts_rate.clamp(0.5, 2.0);
    settings.tts_volume = settings.tts_volume.clamp(0.0, 1.0);
    settings.wake_word = settings.wake_word.trim().to_ascii_lowercase();
    if settings.wake_word.is_empty() {
        settings.wake_word = "hey nova".to_string();
    }
    if settings.push_to_talk_hotkey.trim().is_empty() {
        settings.push_to_talk_hotkey = "Ctrl+Shift+Space".to_string();
    }
    settings.openai_base_url = settings.openai_base_url.trim().to_string();
    if settings.openai_base_url.is_empty() {
        settings.openai_base_url = "https://api.openai.com/v1".to_string();
    }
    settings.openai_model = settings.openai_model.trim().to_string();
    if settings.openai_model.is_empty() {
        settings.openai_model = "gpt-4o-mini".to_string();
    }
    settings
}

#[tauri::command]
fn update_settings(state: State<'_, AppState>, patch: SettingsPatch) -> NovaResult<Settings> {
    let mut settings = state.settings_snapshot();
    if let Some(value) = patch.wake_word {
        settings.wake_word = value;
    }
    if let Some(value) = patch.wake_enabled {
        settings.wake_enabled = value;
    }
    if let Some(value) = patch.sensitivity {
        settings.sensitivity = value;
    }
    if let Some(value) = patch.microphone {
        settings.microphone = optional_device(value);
    }
    if let Some(value) = patch.push_to_talk_hotkey {
        settings.push_to_talk_hotkey = value;
    }
    if let Some(value) = patch.stt_provider {
        settings.stt_provider = value;
    }
    if let Some(value) = patch.tts_enabled {
        settings.tts_enabled = value;
    }
    if let Some(value) = patch.tts_voice {
        settings.tts_voice = optional_device(value);
    }
    if let Some(value) = patch.tts_rate {
        settings.tts_rate = value;
    }
    if let Some(value) = patch.tts_volume {
        settings.tts_volume = value;
    }
    if let Some(value) = patch.llm_provider {
        settings.llm_provider = value;
    }
    if let Some(value) = patch.openai_base_url {
        settings.openai_base_url = value;
    }
    if let Some(value) = patch.openai_model {
        settings.openai_model = value;
    }
    if let Some(value) = patch.confirm_destructive {
        settings.confirm_destructive = value;
    }
    if let Some(value) = patch.autostart {
        settings.autostart = value;
    }
    if let Some(value) = patch.start_minimized {
        settings.start_minimized = value;
    }
    if let Some(value) = patch.overlay_enabled {
        settings.overlay_enabled = value;
    }

    persist_settings(&state, sanitize_settings(settings))
}

/// Store the provider credential. The value is written to the database
/// (DPAPI-protected) and never read back into the UI.
#[tauri::command]
fn set_provider_credential(
    state: State<'_, AppState>,
    credential: String,
) -> NovaResult<Settings> {
    let mut settings = state.settings_snapshot();
    settings.openai_api_key = credential.trim().to_string();
    persist_settings(&state, settings)
}

/// Remove the stored provider credential (falls back to offline rules, or to
/// the OPENAI_API_KEY environment value when one is set).
#[tauri::command]
fn clear_provider_credential(state: State<'_, AppState>) -> NovaResult<Settings> {
    let mut settings = state.settings_snapshot();
    settings.openai_api_key.clear();
    persist_settings(&state, settings)
}

fn persist_settings(state: &State<'_, AppState>, settings: Settings) -> NovaResult<Settings> {
    {
        let conn = state
            .db
            .lock()
            .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
        db::save_settings(&conn, &settings)?;
    }
    *state
        .settings
        .write()
        .map_err(|_| NovaError::Other("settings lock poisoned".into()))? = settings.clone();
    state.send_event(RuntimeEvent::SettingsChanged(Box::new(settings.clone())));

    let mut view = settings;
    view.openai_api_key.clear();
    Ok(view)
}

#[tauri::command]
fn get_behaviour(state: State<'_, AppState>) -> BehaviourPrefs {
    state.behaviour_snapshot()
}

#[tauri::command]
fn update_behaviour(
    state: State<'_, AppState>,
    prefs: BehaviourPrefs,
) -> NovaResult<BehaviourPrefs> {
    let prefs = prefs.sanitized();
    {
        let conn = state
            .db
            .lock()
            .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
        db::save_json(&conn, crate::preferences::BEHAVIOUR_KEY, &prefs)?;
    }
    state.set_behaviour(prefs.clone());
    state.send_event(RuntimeEvent::BehaviourChanged(Box::new(prefs.clone())));
    Ok(prefs)
}

#[tauri::command]
fn get_appearance(state: State<'_, AppState>) -> Appearance {
    state.appearance_snapshot()
}

/// Persist the applied appearance. Theme ids and accents are validated here —
/// the UI can send a preview freely, but only a normalized configuration is
/// ever stored.
#[tauri::command]
fn update_appearance(state: State<'_, AppState>, appearance: Appearance) -> NovaResult<Appearance> {
    let appearance = appearance.normalized();
    {
        let conn = state
            .db
            .lock()
            .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
        db::save_json(&conn, crate::appearance::APPEARANCE_KEY, &appearance)?;
    }
    state.set_appearance(appearance.clone());
    state.send_event(RuntimeEvent::AppearanceChanged(Box::new(appearance.clone())));
    Ok(appearance)
}

#[tauri::command]
fn reset_appearance(state: State<'_, AppState>) -> NovaResult<Appearance> {
    update_appearance(state, Appearance::default())
}

#[tauri::command]
fn get_onboarding(state: State<'_, AppState>) -> NovaResult<Onboarding> {
    let conn = state
        .db
        .lock()
        .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
    db::load_json_or_default::<Onboarding>(&conn, crate::onboarding::ONBOARDING_KEY)
}

#[tauri::command]
fn update_onboarding(state: State<'_, AppState>, onboarding: Onboarding) -> NovaResult<Onboarding> {
    {
        let conn = state
            .db
            .lock()
            .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
        db::save_json(&conn, crate::onboarding::ONBOARDING_KEY, &onboarding)?;
    }
    Ok(onboarding)
}

#[tauri::command]
fn reset_onboarding(state: State<'_, AppState>) -> NovaResult<Onboarding> {
    let fresh = Onboarding::default();
    update_onboarding(state, fresh)
}

// ---------------------------------------------------------------------------
// Audio devices
// ---------------------------------------------------------------------------

#[tauri::command]
fn list_microphones() -> Vec<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let mut names = Vec::new();
    if let Ok(devices) = host.input_devices() {
        for device in devices {
            if let Ok(description) = device.description() {
                names.push(description.name().to_string());
            }
        }
    }
    names
}

#[tauri::command]
fn list_voices() -> Vec<crate::tts::VoiceInfo> {
    crate::tts::list_system_voices()
}

#[tauri::command]
fn get_microphone_status(state: State<'_, AppState>) -> MicrophoneStatus {
    state.microphone_status()
}

/// Start a short microphone level sweep (Settings → Test microphone and the
/// onboarding mic test). Levels arrive as `nova:level` events.
#[tauri::command]
fn start_microphone_test(app: AppHandle) {
    crate::runtime::start_microphone_test(&app);
}

#[tauri::command]
fn stop_microphone_test(app: AppHandle) {
    crate::runtime::stop_microphone_test(&app);
}

#[tauri::command]
fn open_microphone_settings() {
    crate::runtime::open_windows_microphone_settings();
}

#[tauri::command]
async fn download_model(app: AppHandle) -> NovaResult<String> {
    if let Some(path) = crate::stt::model::resolve_model_path() {
        return Ok(path.display().to_string());
    }
    let path = crate::stt::model::download_default_model()
        .await
        .map_err(NovaError::Other)?;
    crate::runtime::reload_whisper(&app, path.clone());
    Ok(path.display().to_string())
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

/// Payload for creating a task from the dashboard, matching the task editor.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskInput {
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    category: Option<String>,
    /// RFC 3339 due date (the editor converts the local picker value).
    #[serde(default)]
    due: Option<String>,
    /// `manual` | `voice` | `onboarding`
    #[serde(default)]
    source: Option<String>,
}

/// Payload for editing a task; omitted fields are left untouched.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskPatchInput {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    priority: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    due: Option<String>,
    /// Explicitly remove the due date.
    #[serde(default)]
    clear_due: Option<bool>,
}

fn validate_due(due: Option<&String>) -> NovaResult<Option<String>> {
    match due.map(|d| d.trim()).filter(|d| !d.is_empty()) {
        Some(raw) => Ok(Some(crate::commands::validator::parse_due(raw, "task editor")?)),
        None => Ok(None),
    }
}

#[tauri::command]
fn list_todos(state: State<'_, AppState>, completed: Option<bool>) -> NovaResult<Vec<db::Todo>> {
    let conn = state
        .db
        .lock()
        .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
    db::list_todos(&conn, completed)
}

/// The Tasks screen query: view (all/pending/completed/overdue) + search + sort.
#[tauri::command]
fn list_tasks(
    state: State<'_, AppState>,
    view: Option<db::TaskView>,
    query: Option<String>,
    sort: Option<db::TaskSort>,
) -> NovaResult<Vec<db::Todo>> {
    let conn = state
        .db
        .lock()
        .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
    db::list_tasks(
        &conn,
        &db::TaskFilter {
            view: view.unwrap_or_default(),
            query: query.filter(|q| !q.trim().is_empty()),
            sort: sort.unwrap_or_default(),
        },
    )
}

#[tauri::command]
fn get_task_counts(state: State<'_, AppState>) -> NovaResult<db::TaskCounts> {
    let conn = state
        .db
        .lock()
        .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
    db::task_counts(&conn)
}

#[tauri::command]
fn get_todo(state: State<'_, AppState>, id: i64) -> NovaResult<Option<db::Todo>> {
    let conn = state
        .db
        .lock()
        .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
    db::get_todo(&conn, id)
}

#[tauri::command]
fn create_todo(
    state: State<'_, AppState>,
    title: String,
    description: Option<String>,
) -> NovaResult<db::Todo> {
    create_task(
        state,
        TaskInput {
            title,
            description,
            priority: None,
            category: None,
            due: None,
            source: None,
        },
    )
}

#[tauri::command]
fn create_task(state: State<'_, AppState>, task: TaskInput) -> NovaResult<db::Todo> {
    let title = task.title.trim().to_string();
    if title.is_empty() {
        return Err(NovaError::InvalidCommand(
            "Give the task a name so Nova can find it later.".into(),
        ));
    }
    if title.chars().count() > 300 {
        return Err(NovaError::InvalidCommand(
            "Task titles are limited to 300 characters.".into(),
        ));
    }
    let conn = state
        .db
        .lock()
        .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
    db::insert_todo_full(
        &conn,
        db::NewTodo {
            title,
            description: task.description.unwrap_or_default(),
            priority: task.priority.unwrap_or_else(|| "medium".into()),
            category: task.category.unwrap_or_default(),
            due_at: validate_due(task.due.as_ref())?,
            source: task.source.unwrap_or_else(|| "manual".into()),
        },
    )
}

#[tauri::command]
fn update_todo(
    state: State<'_, AppState>,
    id: i64,
    title: Option<String>,
    description: Option<String>,
) -> NovaResult<Option<db::Todo>> {
    let conn = state
        .db
        .lock()
        .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
    db::update_todo(&conn, id, title.as_deref(), description.as_deref())
}

#[tauri::command]
fn update_task(
    state: State<'_, AppState>,
    id: i64,
    patch: TaskPatchInput,
) -> NovaResult<Option<db::Todo>> {
    let due = validate_due(patch.due.as_ref())?;
    let conn = state
        .db
        .lock()
        .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
    db::update_todo_full(
        &conn,
        id,
        db::TodoPatch {
            title: patch.title,
            description: patch.description,
            priority: patch.priority,
            category: patch.category,
            due_at: due,
            clear_due: patch.clear_due.unwrap_or(false),
        },
    )
}

#[tauri::command]
fn set_todo_completed(
    state: State<'_, AppState>,
    id: i64,
    completed: bool,
) -> NovaResult<Option<db::Todo>> {
    let conn = state
        .db
        .lock()
        .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
    db::set_todo_completed(&conn, id, completed)
}

#[tauri::command]
fn delete_todo(state: State<'_, AppState>, id: i64) -> NovaResult<bool> {
    let conn = state
        .db
        .lock()
        .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
    db::delete_todo(&conn, id)
}

#[tauri::command]
fn clear_completed_todos(state: State<'_, AppState>) -> NovaResult<usize> {
    let conn = state
        .db
        .lock()
        .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
    db::clear_completed_todos(&conn)
}

#[tauri::command]
fn search_todos(state: State<'_, AppState>, query: String) -> NovaResult<Vec<db::Todo>> {
    let conn = state
        .db
        .lock()
        .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
    db::search_todos(&conn, &query)
}

/// Counts for the Home "Today" card: pending today, overdue, completed today.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TodayOverview {
    pending_today: usize,
    overdue: usize,
    completed_today: usize,
    open_total: i64,
}

#[tauri::command]
fn get_today_overview(state: State<'_, AppState>) -> NovaResult<TodayOverview> {
    let conn = state
        .db
        .lock()
        .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
    let (pending_today, overdue, completed_today) = crate::skills::todo::today_counts(&conn)?;
    let counts = db::task_counts(&conn)?;
    Ok(TodayOverview {
        pending_today,
        overdue,
        completed_today,
        open_total: counts.pending,
    })
}

// ---------------------------------------------------------------------------
// Activity
// ---------------------------------------------------------------------------

#[tauri::command]
fn list_activity(
    state: State<'_, AppState>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> NovaResult<Vec<db::ActivityEntry>> {
    let conn = state
        .db
        .lock()
        .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
    db::list_activity(&conn, limit.unwrap_or(100).clamp(1, 500), offset.unwrap_or(0))
}

/// The Activity screen query: status filter (all/success/failed/cancelled) + search.
#[tauri::command]
fn list_activity_view(
    state: State<'_, AppState>,
    status: Option<String>,
    query: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> NovaResult<Vec<db::ActivityEntry>> {
    let conn = state
        .db
        .lock()
        .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
    db::list_activity_filtered(
        &conn,
        &db::ActivityFilter {
            status,
            query: query.filter(|q| !q.trim().is_empty()),
            limit: limit.unwrap_or(200).clamp(1, 500),
            offset: offset.unwrap_or(0),
        },
    )
}

/// (total, success, cancelled) — the chips at the top of the Activity screen.
#[tauri::command]
fn get_activity_counts(state: State<'_, AppState>) -> NovaResult<(i64, i64, i64)> {
    let conn = state
        .db
        .lock()
        .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
    db::activity_status_counts(&conn)
}

#[tauri::command]
fn clear_activity_history(app: AppHandle, state: State<'_, AppState>) -> NovaResult<usize> {
    let removed = {
        let conn = state
            .db
            .lock()
            .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
        db::clear_activity(&conn)?
    };
    crate::events::emit_nova(&app, crate::events::EVT_ACTIVITY, json!({}));
    Ok(removed)
}

/// Re-run a logged command. A fresh activity entry is written — history is
/// never rewritten, so the original outcome stays visible.
#[tauri::command]
fn retry_activity(state: State<'_, AppState>, id: i64) -> NovaResult<String> {
    let (transcript, intent) = {
        let conn = state
            .db
            .lock()
            .map_err(|_| NovaError::Other("database lock poisoned".into()))?;
        let entry = db::get_activity(&conn, id)?
            .ok_or_else(|| NovaError::InvalidCommand(format!("no activity entry #{id}")))?;
        if entry.transcript.trim().is_empty() {
            return Err(NovaError::InvalidCommand(
                "That entry has no command to retry.".into(),
            ));
        }
        (entry.transcript, entry.intent)
    };
    state.send_event(RuntimeEvent::TranscriptReady {
        text: transcript,
        source: TranscriptSource::Text,
    });
    Ok(intent.unwrap_or_else(|| "unknown".into()))
}

// ---------------------------------------------------------------------------
// Runtime / desktop controls
// ---------------------------------------------------------------------------

#[tauri::command]
fn activate(state: State<'_, AppState>) {
    state.send_event(RuntimeEvent::Activate);
}

#[tauri::command]
fn set_paused(state: State<'_, AppState>, paused: bool) {
    state.send_event(RuntimeEvent::SetPaused(paused));
}

/// Start (`running`), pause (`paused`) or stop (`stopped`) the assistant.
#[tauri::command]
fn set_service_state(state: State<'_, AppState>, service: ServiceState) -> NovaResult<()> {
    state.send_event(RuntimeEvent::SetServiceState(service));
    Ok(())
}

#[tauri::command]
fn confirm_pending(state: State<'_, AppState>, confirmed: bool) {
    state.send_event(RuntimeEvent::ConfirmResolved(confirmed));
}

#[tauri::command]
fn process_text(state: State<'_, AppState>, text: String) -> NovaResult<()> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(NovaError::InvalidCommand("empty command".into()));
    }
    state.send_event(RuntimeEvent::TranscriptReady {
        text,
        source: TranscriptSource::Text,
    });
    Ok(())
}

/// Stop the current interaction (overlay "Stop listening", Esc, tray cancel).
#[tauri::command]
fn cancel_current(state: State<'_, AppState>) {
    state.send_event(RuntimeEvent::CancelRequested);
}

#[tauri::command]
fn show_floating_assistant(app: AppHandle) {
    crate::runtime::show_overlay(&app);
}

#[tauri::command]
fn hide_floating_assistant(app: AppHandle) {
    crate::runtime::hide_overlay(&app);
}

/// Collapse or expand the floating assistant window (`mini` | `expanded`).
#[tauri::command]
fn set_overlay_size(app: AppHandle, mode: String) {
    crate::runtime::set_overlay_mode(&app, &mode);
}

#[tauri::command]
fn open_main(app: AppHandle, page: Option<String>) {
    crate::runtime::show_main(&app, page.as_deref());
}

/// Quit Nova completely: release the microphone, unregister the global hotkey
/// and exit. Used by the in-app tray panel and the native tray menu.
#[tauri::command]
fn quit_nova(app: AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut ctl = state.ctl.lock().expect("ctl lock poisoned");
        ctl.audio = None;
        ctl.whisper = None;
    }
    crate::runtime::release_hotkeys(&app);
    app.exit(0);
}

// ---------------------------------------------------------------------------
// Diagnostics, docs, updates
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Check {
    name: String,
    ok: bool,
    detail: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Diagnostics {
    checks: Vec<Check>,
    version: String,
    data_dir: Option<String>,
    database_path: Option<String>,
    schema_version: Option<i64>,
    model_path: Option<String>,
    push_to_talk: String,
    credential_protection: String,
}

#[tauri::command]
fn get_diagnostics(state: State<'_, AppState>) -> Diagnostics {
    let settings = state.settings_snapshot();
    let mut checks = Vec::new();

    // Microphone
    let mic = state.microphone_status();
    checks.push(Check {
        name: "Microphone".into(),
        ok: mic.available,
        detail: if mic.available {
            mic.device
                .clone()
                .unwrap_or_else(|| "Using the system default device".into())
        } else {
            mic.message
                .clone()
                .unwrap_or_else(|| "No input device is available".into())
        },
    });

    // Speech engine
    let (model_ready, model_path) = {
        let ctl = state.ctl.lock().expect("ctl lock poisoned");
        (ctl.model_ready, ctl.model_path.clone())
    };
    checks.push(Check {
        name: "Speech engine".into(),
        ok: model_ready,
        detail: match &model_path {
            Some(path) if model_ready => format!("Local model loaded — {}", path.display()),
            Some(path) => format!("Model found but failed to load — {}", path.display()),
            None => "No speech model installed (run `npm run models`)".into(),
        },
    });

    // Database
    let (db_ok, schema, db_path) = {
        let conn = state.db.lock();
        match conn {
            Ok(conn) => (
                db::schema_version(&conn).is_ok(),
                db::schema_version(&conn).ok(),
                db::default_db_path().ok(),
            ),
            Err(_) => (false, None, None),
        }
    };
    checks.push(Check {
        name: "Local database".into(),
        ok: db_ok,
        detail: match (db_ok, schema) {
            (true, Some(version)) => format!("SQLite healthy — schema v{version}"),
            _ => "The local database could not be read".into(),
        },
    });

    // AI provider
    checks.push(Check {
        name: "Command understanding".into(),
        ok: true,
        detail: if settings.has_api_key() {
            format!(
                "Cloud model configured ({}) with the offline parser as a fallback",
                settings.resolved_model()
            )
        } else {
            "Offline rules only — no AI provider configured".into()
        },
    });

    // Text to speech
    checks.push(Check {
        name: "Voice replies".into(),
        ok: settings.tts_enabled,
        detail: if settings.tts_enabled {
            match settings.tts_voice.clone() {
                Some(voice) => format!("Enabled — {voice}"),
                None => "Enabled — system default voice".into(),
            }
        } else {
            "Disabled in Settings → Speech & AI".into()
        },
    });

    let _ = state.settings_snapshot();
    Diagnostics {
        checks,
        version: env!("CARGO_PKG_VERSION").to_string(),
        data_dir: db::data_dir().map(|p| p.display().to_string()),
        database_path: db_path.map(|p| p.display().to_string()),
        schema_version: schema,
        model_path: model_path.map(|p| p.display().to_string()),
        push_to_talk: settings.push_to_talk_hotkey.clone(),
        credential_protection: if crate::secrets::available() {
            "Provider credentials are encrypted with Windows DPAPI for the current user".into()
        } else {
            "Provider credentials are stored unprotected on this platform".into()
        },
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateInfo {
    supported: bool,
    available: bool,
    message: String,
}

/// Nova has no update feed configured in this build, and saying otherwise
/// would be a lie — the button reports exactly that.
#[tauri::command]
fn check_for_updates() -> UpdateInfo {
    UpdateInfo {
        supported: false,
        available: false,
        message: format!(
            "Nova {} is built from source; no update channel is configured in this build.",
            env!("CARGO_PKG_VERSION")
        ),
    }
}

#[tauri::command]
fn open_logs_folder() -> NovaResult<String> {
    let dir = db::data_dir()
        .ok_or_else(|| NovaError::Other("could not resolve the Nova data folder".into()))?;
    std::fs::create_dir_all(&dir)?;
    crate::windows::shell_execute(&dir.display().to_string())?;
    Ok(dir.display().to_string())
}

/// Open the bundled documentation (README) with the user's default handler.
#[tauri::command]
fn open_documentation() -> NovaResult<String> {
    let candidates = [
        std::env::current_dir().ok().map(|dir| dir.join("README.md")),
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join("README.md"))),
    ];
    let path = candidates
        .into_iter()
        .flatten()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            NovaError::Other("README.md was not found next to the application".into())
        })?;
    crate::windows::shell_execute(&path.display().to_string())?;
    Ok(path.display().to_string())
}
