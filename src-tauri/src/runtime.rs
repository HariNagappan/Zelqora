//! Background runtime: the agent event loop, state machine transitions,
//! overlay show/hide management, tray menu, and audio engine lifecycle.

use std::path::PathBuf;
use std::time::Duration;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, LogicalSize, Manager, Runtime, Size};

use crate::agent::AgentTurnResult;
use crate::appearance::Appearance;
use crate::audio::AudioEngine;
use crate::events::{
    self, nova_can_transition, MicrophoneStatus, NavigatePayload, NovaState, RuntimeEvent,
    ServicePayload, ServiceState, StatePayload, TranscriptSource,
};
use crate::preferences::BehaviourPrefs;
use crate::settings::Settings;
use crate::state::AppState;
use crate::stt::WhisperHandle;
use crate::tts::TtsHandle;

/// Collapsed pill size (logical pixels).
const OVERLAY_MINI: (f64, f64) = (268.0, 60.0);
/// How many times a dead capture stream is rebuilt before Nova reports the
/// microphone as unavailable instead of retrying forever.
const MAX_AUDIO_RESTARTS: u32 = 3;
/// Expanded overlay size (logical pixels) — the 380×240 panel plus margins.
const OVERLAY_EXPANDED: (f64, f64) = (400.0, 268.0);
/// Gap between the overlay and the screen edge.
const OVERLAY_MARGIN: f64 = 20.0;

/// Wire everything up. Called once from the Tauri setup hook.
pub fn init(app: &AppHandle) -> tauri::Result<()> {
    // Persisted preferences: behaviour + appearance + onboarding.
    {
        let state = app.state::<AppState>();
        let (behaviour, appearance) = {
            let conn = state.db.lock().expect("db lock poisoned");
            (
                crate::database::load_json_or_default::<BehaviourPrefs>(
                    &conn,
                    crate::preferences::BEHAVIOUR_KEY,
                )
                .unwrap_or_default(),
                crate::database::load_json_or_default::<Appearance>(
                    &conn,
                    crate::appearance::APPEARANCE_KEY,
                )
                .unwrap_or_default(),
            )
        };
        state.set_behaviour(behaviour.sanitized());
        state.set_appearance(appearance.normalized());
        // Enforce the configured history retention on startup.
        let retention = state.behaviour_snapshot().history_retention_days;
        prune_history(app, retention);
    }

    // Workers.
    let tts = TtsHandle::spawn(app.clone());
    let events_tx = spawn_agent_loop(app.clone());

    {
        let state = app.state::<AppState>();
        let mut ctl = state.ctl.lock().expect("ctl lock poisoned");
        ctl.events_tx = Some(events_tx);
        ctl.tts = Some(tts);
    }

    // Local speech recognition (optional — model may not be downloaded yet).
    if let Some(model_path) = crate::stt::model::resolve_model_path() {
        reload_whisper(app, model_path);
    } else {
        eprintln!(
            "[nova] whisper model not found — wake word & local STT disabled until \
             `npm run models` (or Settings → Download model) is used"
        );
    }

    build_tray(app)?;
    register_hotkey(app);
    start_audio(app);
    set_overlay_mode(app, "mini");

    // Initial state.
    let settings = app.state::<AppState>().settings_snapshot();
    let model_ready = {
        let state = app.state::<AppState>();
        let ctl = state.ctl.lock().expect("ctl lock poisoned");
        ctl.model_ready
    };
    if settings.wake_enabled && model_ready {
        set_state(app, NovaState::ListeningForWakeWord, None);
    } else {
        set_state(app, NovaState::Idle, None);
    }

    Ok(())
}

fn prune_history(app: &AppHandle, retention_days: i64) {
    // Scoped so the connection guard is released before the result is reported.
    let outcome: Option<crate::error::NovaResult<usize>> = (|| {
        let state = app.state::<AppState>();
        let locked = state.db.lock();
        match locked {
            Ok(conn) => Some(crate::database::prune_activity(&conn, retention_days)),
            Err(_) => None,
        }
    })();
    match outcome {
        Some(Ok(removed)) if removed > 0 => {
            eprintln!("[nova] pruned {removed} activity entries older than {retention_days} days")
        }
        Some(Err(err)) => eprintln!("[nova] could not prune activity history: {err}"),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// State + tray
// ---------------------------------------------------------------------------

/// Move the state machine to `state`, rejecting illegal transitions.
///
/// Illegal transitions keep the previous state (and log why) so the UI can
/// never display a state the pipeline cannot actually be in. Every state can
/// recover to Idle / Error / microphone states, so a rejected transition can
/// never wedge the assistant.
pub fn set_state<R: Runtime>(app: &AppHandle<R>, state: NovaState, detail: Option<String>) {
    {
        let app_state = app.state::<AppState>();
        let mut ctl = app_state.ctl.lock().expect("ctl lock poisoned");
        if !nova_can_transition(ctl.state, state) {
            eprintln!(
                "[nova] refused state transition {} -> {}",
                ctl.state.as_str(),
                state.as_str()
            );
            return;
        }
        ctl.state = state;
        ctl.state_detail = detail.clone();
    }
    update_tray_status(app, state);
    let (paused, service) = {
        let app_state = app.state::<AppState>();
        let ctl = app_state.ctl.lock().expect("ctl lock poisoned");
        (ctl.paused, ctl.service)
    };
    events::emit_nova(
        app,
        events::EVT_STATE,
        StatePayload {
            state: state.as_str().to_string(),
            detail,
            paused,
            service: service.as_str().to_string(),
        },
    );
}

pub fn current_state<R: Runtime>(app: &AppHandle<R>) -> NovaState {
    let app_state = app.state::<AppState>();
    let ctl = app_state.ctl.lock().expect("ctl lock poisoned");
    ctl.state
}

pub fn set_awaiting_confirmation<R: Runtime>(app: &AppHandle<R>, awaiting: bool) {
    let app_state = app.state::<AppState>();
    let mut ctl = app_state.ctl.lock().expect("ctl lock poisoned");
    ctl.awaiting_confirmation = awaiting;
    if awaiting {
        ctl.request_command_capture = true;
    }
}

pub fn set_awaiting_clarification<R: Runtime>(app: &AppHandle<R>, awaiting: bool) {
    let app_state = app.state::<AppState>();
    let mut ctl = app_state.ctl.lock().expect("ctl lock poisoned");
    ctl.awaiting_clarification = awaiting;
    if awaiting {
        ctl.request_command_capture = true;
    }
}

/// Stop the current interaction without executing anything (Esc / Stop listening).
pub fn cancel_interaction(app: &AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut ctl = state.ctl.lock().expect("ctl lock poisoned");
        ctl.request_cancel = true;
        ctl.request_finalize_capture = false;
        ctl.awaiting_confirmation = false;
        ctl.awaiting_clarification = false;
    }
    events::emit_nova(
        app,
        events::EVT_CONFIRM,
        events::ConfirmPayload {
            pending: false,
            prompt: None,
            confirm_label: None,
            kind: None,
            options: None,
        },
    );
    set_state(app, NovaState::Cancelled, Some("Cancelled — nothing changed".into()));
    schedule_settle(app, Duration::from_millis(1600));
}

/// Return to the resting state after a terminal state was shown.
fn schedule_settle(app: &AppHandle, delay: Duration) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        let state = current_state(&app);
        if matches!(
            state,
            NovaState::Success | NovaState::Cancelled | NovaState::Error
        ) {
            settle_idle(&app);
        }
    });
}

/// Resting state: keep wake listening when possible, otherwise Idle.
pub fn settle_idle(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings_snapshot();
    let ctl = state.ctl.lock().expect("ctl lock poisoned");
    let (paused, service, model_ready, overlay_enabled) = (
        ctl.paused,
        ctl.service,
        ctl.model_ready,
        settings.overlay_enabled,
    );
    drop(ctl);

    if service == ServiceState::Stopped {
        set_state(app, NovaState::Idle, Some("Nova is stopped".into()));
        hide_overlay(app);
        return;
    }
    if paused {
        set_state(app, NovaState::Idle, Some("Listening paused".into()));
    } else if settings.wake_enabled && model_ready {
        set_state(app, NovaState::ListeningForWakeWord, None);
    } else {
        set_state(app, NovaState::Idle, None);
    }

    // The floating assistant stays available as the collapsed pill.
    if overlay_enabled {
        set_overlay_mode(app, "mini");
    } else {
        hide_overlay(app);
    }
}

fn update_tray_status<R: Runtime>(app: &AppHandle<R>, state: NovaState) {
    let (tooltip, status_label) = {
        let app_state = app.state::<AppState>();
        let ctl = app_state.ctl.lock().expect("ctl lock poisoned");
        let label = match ctl.service {
            ServiceState::Running => state.label().to_string(),
            ServiceState::Paused => "Listening paused".to_string(),
            ServiceState::Stopped => "Nova is stopped".to_string(),
        };
        let tooltip = match ctl.service {
            ServiceState::Running => format!("Nova — {label}"),
            ServiceState::Paused => "Nova — Paused".to_string(),
            ServiceState::Stopped => "Nova — Stopped".to_string(),
        };
        (tooltip, format!("Nova · {label}"))
    };
    if let Some(tray) = app.tray_by_id("nova-tray") {
        let _ = tray.set_tooltip(Some(&tooltip));
    }
    let item = {
        let app_state = app.state::<AppState>();
        let ctl = app_state.ctl.lock().expect("ctl lock poisoned");
        ctl.tray_status.clone()
    };
    if let Some(item) = item {
        let _ = item.set_text(status_label);
    }
}

fn sync_pause_menu(app: &AppHandle, service: ServiceState) {
    let app_state = app.state::<AppState>();
    let ctl = app_state.ctl.lock().expect("ctl lock poisoned");
    if let Some(item) = &ctl.tray_pause {
        let _ = item.set_checked(service != ServiceState::Running);
        let _ = item.set_text(match service {
            ServiceState::Running => "Stop listening",
            ServiceState::Paused => "Start listening",
            ServiceState::Stopped => "Start Nova",
        });
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    // The menu mirrors the designed tray: status line, open, listening control,
    // tasks, activity, settings, microphone help (only when relevant), exit.
    let status = MenuItem::with_id(app, "status", "Nova · starting…", false, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let open = MenuItem::with_id(app, "open", "Open Nova", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(
        app,
        "pause",
        "Stop listening",
        true,
        false,
        None::<&str>,
    )?;
    let tasks = MenuItem::with_id(app, "tasks", "Open Tasks", true, None::<&str>)?;
    let activity = MenuItem::with_id(app, "activity", "Recent activity", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let michelp = MenuItem::with_id(
        app,
        "michelp",
        "Open microphone settings",
        false,
        None::<&str>,
    )?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Exit Nova", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &status, &sep1, &open, &pause, &tasks, &activity, &settings, &michelp, &sep2, &quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("nova-tray")
        .tooltip("Nova")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app, Some("home")),
            "pause" => {
                let service = app.state::<AppState>().service_state();
                let next = match service {
                    ServiceState::Running => ServiceState::Paused,
                    _ => ServiceState::Running,
                };
                app.state::<AppState>()
                    .send_event(RuntimeEvent::SetServiceState(next));
            }
            "tasks" => show_main(app, Some("tasks")),
            "activity" => show_main(app, Some("activity")),
            "settings" => show_main(app, Some("settings")),
            "michelp" => open_windows_microphone_settings(),
            "quit" => {
                // Release the microphone, hotkey and audio before exiting.
                {
                    let state = app.state::<AppState>();
                    let mut ctl = state.ctl.lock().expect("ctl lock poisoned");
                    ctl.audio = None;
                    ctl.whisper = None;
                }
                release_hotkeys(app);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Left click opens the dashboard; the menu handles the rest.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                show_main(app, Some("home"));
            }
        });

    {
        let app_state = app.state::<AppState>();
        let mut ctl = app_state.ctl.lock().expect("ctl lock poisoned");
        ctl.tray_pause = Some(pause.clone());
        ctl.tray_status = Some(status.clone());
    }

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

/// Open Windows' microphone privacy page (used by the permission guidance).
pub fn open_windows_microphone_settings() {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", "ms-settings:privacy-microphone"])
            .spawn();
    }
}

pub fn show_main(app: &AppHandle, page: Option<&str>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    if let Some(page) = page {
        events::emit_nova(
            app,
            events::EVT_NAVIGATE,
            NavigatePayload {
                page: page.to_string(),
            },
        );
    }
}

// ---------------------------------------------------------------------------
// Overlay management
// ---------------------------------------------------------------------------

/// Show the floating assistant, expanding it for the interaction.
pub fn show_overlay(app: &AppHandle) {
    let settings = app.state::<AppState>().settings_snapshot();
    if !settings.overlay_enabled {
        return;
    }
    let Some(window) = app.get_webview_window("overlay") else {
        return;
    };
    {
        let app_state = app.state::<AppState>();
        let mut ctl = app_state.ctl.lock().expect("ctl lock poisoned");
        ctl.overlay_generation += 1;
    }
    set_overlay_mode(app, "expanded");
    let _ = window.show();
}

pub fn hide_overlay(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.hide();
    }
}

/// Resize + reposition the overlay and tell the UI which mode it is in.
pub fn set_overlay_mode(app: &AppHandle, mode: &str) {
    let mode = if mode == "expanded" { "expanded" } else { "mini" };
    {
        let state = app.state::<AppState>();
        let mut ctl = state.ctl.lock().expect("ctl lock poisoned");
        ctl.overlay_mode = mode.to_string();
    }
    let Some(window) = app.get_webview_window("overlay") else {
        return;
    };
    let (width, height) = if mode == "expanded" {
        OVERLAY_EXPANDED
    } else {
        OVERLAY_MINI
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let _ = window.set_size(Size::Logical(LogicalSize::new(width, height)));

    // Bottom-right of the monitor the overlay is currently on.
    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten());
    if let Some(monitor) = monitor {
        let mpos = monitor.position();
        let msize = monitor.size();
        let physical_width = (width * scale) as i32;
        let physical_height = (height * scale) as i32;
        let x = mpos.x + msize.width as i32 - physical_width - (OVERLAY_MARGIN * scale) as i32;
        let y = mpos.y + msize.height as i32 - physical_height - (OVERLAY_MARGIN * scale) as i32;
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }

    events::emit_nova(
        app,
        events::EVT_OVERLAY,
        events::OverlayPayload {
            mode: mode.to_string(),
        },
    );
}

/// Hide the overlay after `delay`, unless a newer interaction replaced it.
pub fn schedule_overlay_hide(app: &AppHandle, delay: Duration) {
    let generation = {
        let app_state = app.state::<AppState>();
        let ctl = app_state.ctl.lock().expect("ctl lock poisoned");
        ctl.overlay_generation
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        let current = {
            let app_state = app.state::<AppState>();
            let ctl = app_state.ctl.lock().expect("ctl lock poisoned");
            ctl.overlay_generation
        };
        if current == generation {
            hide_overlay(&app);
        }
    });
}

// ---------------------------------------------------------------------------
// Audio engine lifecycle
// ---------------------------------------------------------------------------

pub fn start_audio(app: &AppHandle) {
    let settings = app.state::<AppState>().settings_snapshot();
    {
        let app_state = app.state::<AppState>();
        let mut ctl = app_state.ctl.lock().expect("ctl lock poisoned");
        ctl.audio = None; // drop the old engine (stops its thread)
    }
    match AudioEngine::start(app.clone()) {
        Ok(engine) => {
            let app_state = app.state::<AppState>();
            let mut ctl = app_state.ctl.lock().expect("ctl lock poisoned");
            ctl.audio = Some(engine);
            ctl.current_microphone = settings.microphone.clone();
            ctl.audio_restarts = 0;
            ctl.microphone = MicrophoneStatus {
                available: true,
                permission: "granted".into(),
                device: settings.microphone.clone(),
                message: None,
            };
            drop(ctl);
            let status = app_state.microphone_status();
            events::emit_nova(app, events::EVT_MIC, status);
        }
        Err(err) => {
            eprintln!("[nova] audio engine failed to start: {err}");
            report_microphone_failure(app, &err.to_string());
        }
    }
}

/// Classify an audio failure into "permission denied" vs "no microphone" and
/// put the assistant into the matching state.
pub fn report_microphone_failure(app: &AppHandle, message: &str) {
    let lower = message.to_ascii_lowercase();
    let denied = lower.contains("access is denied")
        || lower.contains("permission")
        || lower.contains("not permitted")
        || lower.contains("blocked");
    let status = MicrophoneStatus {
        available: false,
        permission: if denied { "denied" } else { "unknown" }.into(),
        device: None,
        message: Some(message.to_string()),
    };
    {
        let state = app.state::<AppState>();
        let mut ctl = state.ctl.lock().expect("ctl lock poisoned");
        ctl.microphone = status.clone();
    }
    events::emit_nova(app, events::EVT_MIC, status);
    let state = if denied {
        NovaState::MicrophoneDenied
    } else {
        NovaState::MicrophoneUnavailable
    };
    set_state(app, state, Some(message.to_string()));
}

pub fn reload_whisper(app: &AppHandle, model_path: PathBuf) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (handle, loaded) = WhisperHandle::spawn(model_path.clone());
        {
            let app_state = app.state::<AppState>();
            let mut ctl = app_state.ctl.lock().expect("ctl lock poisoned");
            ctl.whisper = Some(handle);
            ctl.model_ready = loaded;
            ctl.model_path = Some(model_path.clone());
        }
        eprintln!(
            "[nova] whisper model {}: {}",
            if loaded { "loaded" } else { "failed to load" },
            model_path.display()
        );
        if loaded {
            let state = app.state::<AppState>();
            let settings = state.settings_snapshot();
            let (paused, service) = {
                let ctl = state.ctl.lock().expect("ctl lock poisoned");
                (ctl.paused, ctl.service)
            };
            if !paused && service != ServiceState::Stopped && settings.wake_enabled {
                set_state(&app, NovaState::ListeningForWakeWord, None);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Hotkey + settings application
// ---------------------------------------------------------------------------

pub fn register_hotkey(app: &AppHandle) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    let hotkey = app.state::<AppState>().settings_snapshot().push_to_talk_hotkey;
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    if hotkey.trim().is_empty() {
        return;
    }
    if let Err(err) = shortcuts.register(hotkey.as_str()) {
        eprintln!("[nova] could not register push-to-talk hotkey '{hotkey}': {err}");
    }
}

/// Release every registered global shortcut (called on shutdown so the hotkey
/// is never left registered by a dying process).
pub fn release_hotkeys(app: &AppHandle) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let _ = app.global_shortcut().unregister_all();
}

fn apply_service_state(app: &AppHandle, service: ServiceState) {
    let app_state = app.state::<AppState>();
    app_state.set_service_state(service);
    sync_pause_menu(app, service);

    match service {
        ServiceState::Running => {
            let settings = app_state.settings_snapshot();
            let model_ready = {
                let ctl = app_state.ctl.lock().expect("ctl lock poisoned");
                ctl.model_ready
            };
            if settings.wake_enabled && model_ready {
                set_state(app, NovaState::ListeningForWakeWord, None);
            } else {
                set_state(app, NovaState::Idle, None);
            }
            // Make sure the capture engine is running again.
            let engine_missing = {
                let ctl = app_state.ctl.lock().expect("ctl lock poisoned");
                ctl.audio.is_none()
            };
            if engine_missing {
                start_audio(app);
            }
            show_overlay(app);
            set_overlay_mode(app, "mini");
        }
        ServiceState::Paused => {
            set_state(app, NovaState::Idle, Some("Listening paused".into()));
            set_overlay_mode(app, "mini");
        }
        ServiceState::Stopped => {
            // Release the microphone and stop wake-word detection entirely.
            {
                let mut ctl = app_state.ctl.lock().expect("ctl lock poisoned");
                ctl.audio = None;
            }
            set_state(app, NovaState::Idle, Some("Nova is stopped".into()));
            hide_overlay(app);
        }
    }

    events::emit_nova(
        app,
        events::EVT_SERVICE,
        ServicePayload {
            service: service.as_str().to_string(),
            label: match service {
                ServiceState::Running => "Running".into(),
                ServiceState::Paused => "Paused".into(),
                ServiceState::Stopped => "Stopped".into(),
            },
            microphone: app_state.microphone_status(),
        },
    );
}

fn apply_settings_change(app: &AppHandle, settings: &Settings) {
    // Live TTS updates.
    {
        let app_state = app.state::<AppState>();
        let ctl = app_state.ctl.lock().expect("ctl lock poisoned");
        if let Some(tts) = &ctl.tts {
            tts.update(settings);
        }
    }

    // Push-to-talk hotkey.
    register_hotkey(app);

    // Auto-start sync (best effort — surfaces in Settings when it fails).
    {
        use tauri_plugin_autostart::ManagerExt;
        let autolaunch = app.autolaunch();
        let enabled = autolaunch.is_enabled().unwrap_or(false);
        if settings.autostart && !enabled {
            if let Err(err) = autolaunch.enable() {
                eprintln!("[nova] could not enable auto-start: {err}");
            }
        } else if !settings.autostart && enabled {
            if let Err(err) = autolaunch.disable() {
                eprintln!("[nova] could not disable auto-start: {err}");
            }
        }
    }

    // Microphone change → restart the audio engine.
    let mic_changed = {
        let app_state = app.state::<AppState>();
        let ctl = app_state.ctl.lock().expect("ctl lock poisoned");
        ctl.current_microphone != settings.microphone
    };
    if mic_changed {
        start_audio(app);
    }

    // Overlay visibility follows the setting immediately.
    if !settings.overlay_enabled {
        hide_overlay(app);
    }

    // Adopt the resting state when idle.
    if matches!(
        current_state(app),
        NovaState::Idle | NovaState::ListeningForWakeWord
    ) {
        settle_idle(app);
    }
}

/// Apply behaviour preferences (retention is enforced immediately).
fn apply_behaviour_change(app: &AppHandle, prefs: &BehaviourPrefs) {
    prune_history(app, prefs.history_retention_days);
    // Re-render the overlay in case "keep expanded" changed.
    let state = current_state(app);
    if state.is_resting() {
        set_overlay_mode(app, if prefs.overlay_keep_expanded { "expanded" } else { "mini" });
    }
}

/// Mirror the applied appearance to the overlay window and re-emit it so a
/// window that loads later can request it via `get_appearance`.
fn apply_appearance_change<R: Runtime>(app: &AppHandle<R>, appearance: &Appearance) {
    events::emit_nova(app, events::EVT_APPEARANCE, appearance.clone());
}

// ---------------------------------------------------------------------------
// The agent event loop
// ---------------------------------------------------------------------------

pub fn spawn_agent_loop(app: AppHandle) -> tokio::sync::mpsc::UnboundedSender<RuntimeEvent> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<RuntimeEvent>();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            handle_runtime_event(&app, event).await;
        }
    });
    tx
}

async fn handle_runtime_event(app: &AppHandle, event: RuntimeEvent) {
    match event {
        RuntimeEvent::WakeDetected { residual_command } => {
            // Speaking the wake phrase again after a finished interaction (a
            // success, a cancellation or an error) is the normal way to start
            // over: settle to rest first so the transition is legal.
            if current_state(app).is_terminal() {
                settle_idle(app);
            }
            set_state(app, NovaState::WakeDetected, Some("Listening…".into()));
            show_overlay(app);
            match residual_command {
                Some(residual) if !residual.trim().is_empty() => {
                    set_state(app, NovaState::Processing, None);
                    process_and_respond(app, residual, TranscriptSource::Voice).await;
                }
                _ => {
                    set_state(app, NovaState::ListeningForCommand, None);
                }
            }
        }

        RuntimeEvent::TranscribeStart { source } => {
            let label = match source {
                TranscriptSource::Voice => "Transcribing what you said…",
                TranscriptSource::Text => "Reading your command…",
            };
            set_state(app, NovaState::Transcribing, Some(label.into()));
        }

        RuntimeEvent::PushToTalkPressed => {
            if current_state(app).is_terminal() {
                settle_idle(app);
            }
            let state = current_state(app);
            if matches!(state, NovaState::ListeningForCommand | NovaState::WakeDetected) {
                // Second press: stop capturing and process what we have.
                {
                    let app_state = app.state::<AppState>();
                    let mut ctl = app_state.ctl.lock().expect("ctl lock poisoned");
                    ctl.request_finalize_capture = true;
                }
                set_state(app, NovaState::Transcribing, Some("Push-to-talk".into()));
            } else {
                {
                    let app_state = app.state::<AppState>();
                    let mut ctl = app_state.ctl.lock().expect("ctl lock poisoned");
                    ctl.request_command_capture = true;
                }
                show_overlay(app);
                set_state(app, NovaState::ListeningForCommand, Some("Push-to-talk".into()));
            }
        }

        RuntimeEvent::Activate => {
            // Tray / dashboard activation behaves like a wake without the phrase.
            if current_state(app).is_terminal() {
                settle_idle(app);
            }
            let service = app.state::<AppState>().service_state();
            if service != ServiceState::Running {
                apply_service_state(app, ServiceState::Running);
            }
            {
                let app_state = app.state::<AppState>();
                let mut ctl = app_state.ctl.lock().expect("ctl lock poisoned");
                ctl.request_command_capture = true;
            }
            show_overlay(app);
            set_state(app, NovaState::ListeningForCommand, None);
        }

        RuntimeEvent::TranscriptReady { text, source } => {
            if source == TranscriptSource::Voice && !text.trim().is_empty() {
                events::emit_nova(
                    app,
                    events::EVT_TRANSCRIPT,
                    events::TranscriptPayload {
                        text: text.clone(),
                        final_transcript: true,
                        source: source.as_str().to_string(),
                    },
                );
            }
            // Typed commands show the assistant too, unless the user asked for
            // quiet background handling.
            if source == TranscriptSource::Text {
                let show = app.state::<AppState>().behaviour_snapshot().overlay_after_command;
                if show {
                    show_overlay(app);
                }
            }
            set_state(app, NovaState::Processing, None);
            process_and_respond(app, text, source).await;
        }

        RuntimeEvent::ConfirmResolved(confirmed) => {
            let result = crate::agent::handle_confirmation(app, confirmed).await;
            finish_turn(app, &result).await;
        }

        RuntimeEvent::ClarificationTimeout => {
            if current_state(app) == NovaState::AwaitingClarification {
                set_state(app, NovaState::Idle, Some("I didn't hear an answer.".into()));
                schedule_settle(app, Duration::from_millis(1400));
            }
        }

        RuntimeEvent::CancelRequested => {
            cancel_interaction(app);
        }

        RuntimeEvent::SpeechDone => {
            let (awaiting_confirmation, awaiting_clarification) = {
                let app_state = app.state::<AppState>();
                let ctl = app_state.ctl.lock().expect("ctl lock poisoned");
                (ctl.awaiting_confirmation, ctl.awaiting_clarification)
            };
            if awaiting_clarification {
                set_state(
                    app,
                    NovaState::AwaitingClarification,
                    Some("Waiting for your answer".into()),
                );
                return;
            }
            if awaiting_confirmation {
                set_state(
                    app,
                    NovaState::AwaitingConfirmation,
                    Some("Waiting for your answer".into()),
                );
                return;
            }
            if matches!(current_state(app), NovaState::Speaking) {
                settle_idle(app);
            }
        }

        RuntimeEvent::CommandTimeout => {
            let waiting = {
                let app_state = app.state::<AppState>();
                let ctl = app_state.ctl.lock().expect("ctl lock poisoned");
                ctl.awaiting_confirmation || ctl.awaiting_clarification
            };
            // A pending question already arms capture; the audio pipeline
            // re-times it instead of us ending the interaction here.
            if !waiting
                && matches!(
                    current_state(app),
                    NovaState::ListeningForCommand | NovaState::Transcribing
                )
            {
                set_state(app, NovaState::Idle, Some("I didn't hear anything.".into()));
                schedule_settle(app, Duration::from_millis(1800));
            }
        }

        RuntimeEvent::SetPaused(paused) => {
            apply_service_state(
                app,
                if paused {
                    ServiceState::Paused
                } else {
                    ServiceState::Running
                },
            );
        }

        RuntimeEvent::SetServiceState(service) => {
            apply_service_state(app, service);
        }

        RuntimeEvent::AudioStreamFailed { message } => {
            eprintln!("[nova] microphone stream failed: {message}");
            // Drop the dead engine (its thread has already returned).
            let attempt = {
                let state = app.state::<AppState>();
                let mut ctl = state.ctl.lock().expect("ctl lock poisoned");
                ctl.audio = None;
                ctl.audio_restarts += 1;
                ctl.audio_restarts
            };

            if attempt <= MAX_AUDIO_RESTARTS {
                // A device change or a format reset is usually transient: rebuild
                // the stream and tell the UI what is happening meanwhile.
                let status = MicrophoneStatus {
                    available: false,
                    permission: "granted".into(),
                    device: None,
                    message: Some(format!("Rebuilding the microphone stream ({message})")),
                };
                {
                    let state = app.state::<AppState>();
                    let mut ctl = state.ctl.lock().expect("ctl lock poisoned");
                    ctl.microphone = status.clone();
                }
                events::emit_nova(app, events::EVT_MIC, status);
                tokio::time::sleep(Duration::from_millis(1200)).await;
                start_audio(app);
            } else {
                // Give up after repeated failures and say exactly what happened.
                report_microphone_failure(app, &message);
            }
        }

        RuntimeEvent::MicrophoneStatusChanged(status) => {
            {
                let app_state = app.state::<AppState>();
                let mut ctl = app_state.ctl.lock().expect("ctl lock poisoned");
                ctl.microphone = (*status).clone();
            }
            events::emit_nova(app, events::EVT_MIC, (*status).clone());
            if status.available {
                settle_idle(app);
            } else if status.permission == "denied" {
                set_state(
                    app,
                    NovaState::MicrophoneDenied,
                    status.message.clone(),
                );
            } else {
                set_state(
                    app,
                    NovaState::MicrophoneUnavailable,
                    status.message.clone(),
                );
            }
        }

        RuntimeEvent::SettingsChanged(settings) => {
            apply_settings_change(app, &settings);
        }

        RuntimeEvent::BehaviourChanged(prefs) => {
            apply_behaviour_change(app, &prefs);
        }

        RuntimeEvent::AppearanceChanged(appearance) => {
            apply_appearance_change(app, &appearance);
        }
    }
}

async fn process_and_respond(app: &AppHandle, text: String, source: TranscriptSource) {
    let result = crate::agent::handle_transcript(app, &text, source).await;
    finish_turn(app, &result).await;
}

/// Emit final states + speak after an agent turn.
async fn finish_turn(app: &AppHandle, result: &AgentTurnResult) {
    let settings = app.state::<AppState>().settings_snapshot();
    let behaviour = app.state::<AppState>().behaviour_snapshot();
    let speak_enabled = settings.tts_enabled && !result.response.trim().is_empty();
    let is_prompt = result.awaiting_confirmation || result.awaiting_clarification;

    // Questions the user must answer: keep the capture armed and the overlay open.
    if is_prompt {
        if result.awaiting_clarification {
            set_state(
                app,
                NovaState::AwaitingClarification,
                Some(result.response.clone()),
            );
        } else {
            set_state(
                app,
                NovaState::AwaitingConfirmation,
                Some(result.response.clone()),
            );
        }
        show_overlay(app);
        if speak_enabled {
            speak(app, &result.response);
        }
        return;
    }

    if result.status == "cancelled" {
        set_state(app, NovaState::Cancelled, Some(result.response.clone()));
        schedule_settle(app, Duration::from_millis(1500));
        return;
    }

    if result.success {
        if speak_enabled {
            set_state(app, NovaState::Speaking, None);
            speak(app, &result.response);
        } else {
            set_state(app, NovaState::Success, Some(result.response.clone()));
            schedule_settle(app, Duration::from_millis(1800));
        }
        return;
    }

    // Failure: show the real reason and offer a recovery path.
    set_state(app, NovaState::Error, Some(result.response.clone()));
    if speak_enabled {
        speak(app, &result.response);
    }
    if behaviour.overlay_after_command {
        show_overlay(app);
    }
    schedule_settle(app, Duration::from_millis(6000));
}

fn speak(app: &AppHandle, text: &str) {
    let app_state = app.state::<AppState>();
    let ctl = app_state.ctl.lock().expect("ctl lock poisoned");
    if let Some(tts) = &ctl.tts {
        tts.speak(text.to_string());
    }
}

/// Ask the audio engine for a microphone level sweep (Settings mic test).
pub fn start_microphone_test(app: &AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut ctl = state.ctl.lock().expect("ctl lock poisoned");
        ctl.mic_test = true;
    }
    // Make sure capture is running, even if the assistant is stopped.
    let engine_missing = {
        let state = app.state::<AppState>();
        let ctl = state.ctl.lock().expect("ctl lock poisoned");
        ctl.audio.is_none()
    };
    if engine_missing {
        start_audio(app);
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(12)).await;
        stop_microphone_test(&app);
    });
}

pub fn stop_microphone_test(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut ctl = state.ctl.lock().expect("ctl lock poisoned");
    ctl.mic_test = false;
}
