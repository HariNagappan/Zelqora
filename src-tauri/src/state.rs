//! Shared application state managed by Tauri.

use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::menu::{CheckMenuItem, MenuItem};

use crate::agent::AgentSession;
use crate::appearance::Appearance;
use crate::audio::AudioEngine;
use crate::events::{MicrophoneStatus, NovaState, RuntimeEvent, ServiceState};
use crate::preferences::BehaviourPrefs;
use crate::settings::Settings;
use crate::stt::WhisperHandle;
use crate::tts::TtsHandle;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub settings: std::sync::RwLock<Settings>,
    pub behaviour: std::sync::RwLock<BehaviourPrefs>,
    pub appearance: std::sync::RwLock<Appearance>,
    pub ctl: Mutex<RuntimeCtl>,
    pub agent: tokio::sync::Mutex<AgentSession>,
}

/// Handles and flags owned by the background runtime.
pub struct RuntimeCtl {
    pub events_tx: Option<tokio::sync::mpsc::UnboundedSender<RuntimeEvent>>,
    pub audio: Option<AudioEngine>,
    pub whisper: Option<WhisperHandle>,
    pub tts: Option<TtsHandle>,
    pub paused: bool,
    /// Running / paused / stopped — mirrors the design's assistant status.
    pub service: ServiceState,
    /// Resolved path of the whisper model file, if found.
    pub model_path: Option<PathBuf>,
    /// Whether the whisper engine was created successfully.
    pub model_ready: bool,
    /// Bumped whenever the overlay is (re)shown so stale auto-hide timers can bail out.
    pub overlay_generation: u64,
    /// Consecutive audio-stream rebuilds since the last successful start.
    pub audio_restarts: u32,
    /// `mini` (collapsed pill) or `expanded` — kept in sync with the overlay window size.
    pub overlay_mode: String,
    /// Current high-level agent state (mirrored to the UI + tray tooltip).
    pub state: NovaState,
    pub state_detail: Option<String>,
    /// Set by the runtime when capture should start without a wake word
    /// (push-to-talk / confirmation follow-up); consumed by the pipeline.
    pub request_command_capture: bool,
    /// Set to stop the current command capture immediately (push-to-talk off).
    pub request_finalize_capture: bool,
    /// Set to abandon the current interaction (Esc / Stop listening).
    pub request_cancel: bool,
    /// True while the agent waits for a yes/no confirmation answer.
    pub awaiting_confirmation: bool,
    /// True while the agent waits for the user to disambiguate a request.
    pub awaiting_clarification: bool,
    /// True while the Settings microphone test sweep is running.
    pub mic_test: bool,
    /// Latest measured input level (0.0 .. 1.0) for the overlay/test meter.
    pub last_level: f32,
    /// Microphone availability/permission snapshot.
    pub microphone: MicrophoneStatus,
    /// Microphone the running audio engine captured from (for change detection).
    pub current_microphone: Option<String>,
    /// Tray "Pause Listening" item, kept so its label/check state can be synced.
    pub tray_pause: Option<CheckMenuItem<tauri::Wry>>,
    /// Tray status line, kept so the label can follow the assistant state.
    pub tray_status: Option<MenuItem<tauri::Wry>>,
}

impl AppState {
    pub fn new(db: Connection, settings: Settings) -> Self {
        AppState {
            db: Mutex::new(db),
            settings: std::sync::RwLock::new(settings),
            behaviour: std::sync::RwLock::new(BehaviourPrefs::default()),
            appearance: std::sync::RwLock::new(Appearance::default()),
            ctl: Mutex::new(RuntimeCtl {
                events_tx: None,
                audio: None,
                whisper: None,
                tts: None,
                paused: false,
                service: ServiceState::Running,
                model_path: None,
                model_ready: false,
                overlay_generation: 0,
                audio_restarts: 0,
                overlay_mode: "mini".into(),
                state: NovaState::Idle,
                state_detail: None,
                request_command_capture: false,
                request_finalize_capture: false,
                request_cancel: false,
                awaiting_confirmation: false,
                awaiting_clarification: false,
                mic_test: false,
                last_level: 0.0,
                microphone: MicrophoneStatus::default(),
                current_microphone: None,
                tray_pause: None,
                tray_status: None,
            }),
            agent: tokio::sync::Mutex::new(AgentSession::default()),
        }
    }

    pub fn settings_snapshot(&self) -> Settings {
        self.settings.read().expect("settings lock poisoned").clone()
    }

    pub fn behaviour_snapshot(&self) -> BehaviourPrefs {
        self.behaviour
            .read()
            .expect("behaviour lock poisoned")
            .clone()
    }

    pub fn set_behaviour(&self, prefs: BehaviourPrefs) {
        *self.behaviour.write().expect("behaviour lock poisoned") = prefs;
    }

    pub fn appearance_snapshot(&self) -> Appearance {
        self.appearance
            .read()
            .expect("appearance lock poisoned")
            .clone()
    }

    pub fn set_appearance(&self, appearance: Appearance) {
        *self.appearance.write().expect("appearance lock poisoned") = appearance;
    }

    pub fn is_paused(&self) -> bool {
        self.ctl.lock().expect("ctl lock poisoned").paused
    }

    pub fn set_paused(&self, paused: bool) {
        self.ctl.lock().expect("ctl lock poisoned").paused = paused;
    }

    pub fn service_state(&self) -> ServiceState {
        self.ctl.lock().expect("ctl lock poisoned").service
    }

    pub fn set_service_state(&self, service: ServiceState) {
        let mut ctl = self.ctl.lock().expect("ctl lock poisoned");
        ctl.service = service;
        ctl.paused = service != ServiceState::Running;
    }

    pub fn microphone_status(&self) -> MicrophoneStatus {
        self.ctl.lock().expect("ctl lock poisoned").microphone.clone()
    }

    pub fn overlay_mode(&self) -> String {
        self.ctl.lock().expect("ctl lock poisoned").overlay_mode.clone()
    }

    pub fn send_event(&self, event: RuntimeEvent) {
        let guard = self.ctl.lock().expect("ctl lock poisoned");
        if let Some(tx) = &guard.events_tx {
            let _ = tx.send(event);
        }
    }
}
