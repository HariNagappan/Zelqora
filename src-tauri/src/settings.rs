//! User-configurable settings, persisted as a single JSON row in SQLite.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SttProvider {
    Local,
    Cloud,
}

impl Default for SttProvider {
    fn default() -> Self {
        SttProvider::Local
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmProviderKind {
    /// Use the cloud provider when an API key is configured, else the offline parser.
    Auto,
    Cloud,
    Offline,
}

impl Default for LlmProviderKind {
    fn default() -> Self {
        LlmProviderKind::Auto
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Wake phrase, e.g. "hey nova".
    pub wake_word: String,
    /// Whether wake-word listening is enabled at all.
    pub wake_enabled: bool,
    /// 0.0 (strict) .. 1.0 (easy). Maps to VAD threshold + wake fuzzy matching.
    pub sensitivity: f32,
    /// Preferred microphone device name (None = system default).
    pub microphone: Option<String>,
    /// Push-to-talk global shortcut (e.g. "Ctrl+Shift+Space").
    pub push_to_talk_hotkey: String,
    /// Speech-to-text provider.
    pub stt_provider: SttProvider,
    /// Speak responses out loud.
    pub tts_enabled: bool,
    /// Preferred voice name (None = system default).
    pub tts_voice: Option<String>,
    /// Speaking rate (0.5 .. 2.0).
    pub tts_rate: f32,
    /// Speaking volume (0.0 .. 1.0).
    pub tts_volume: f32,
    /// AI provider selection.
    pub llm_provider: LlmProviderKind,
    /// OpenAI-compatible base URL.
    pub openai_base_url: String,
    /// OpenAI-compatible model name.
    pub openai_model: String,
    /// OpenAI-compatible API key (empty = none).
    pub openai_api_key: String,
    /// Ask for confirmation before destructive actions.
    pub confirm_destructive: bool,
    /// Launch Nova automatically with Windows.
    pub autostart: bool,
    /// Start hidden (tray only) when launched manually.
    pub start_minimized: bool,
    /// Show the floating overlay during interactions.
    pub overlay_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            wake_word: "hey nova".into(),
            wake_enabled: true,
            sensitivity: 0.5,
            microphone: None,
            push_to_talk_hotkey: "Ctrl+Shift+Space".into(),
            stt_provider: SttProvider::Local,
            tts_enabled: true,
            tts_voice: None,
            tts_rate: 1.0,
            tts_volume: 1.0,
            llm_provider: LlmProviderKind::Auto,
            openai_base_url: "https://api.openai.com/v1".into(),
            openai_model: "gpt-4o-mini".into(),
            openai_api_key: String::new(),
            confirm_destructive: true,
            autostart: false,
            start_minimized: false,
            overlay_enabled: true,
        }
    }
}

impl Settings {
    /// RMS threshold for the voice-activity detector. Lower = more sensitive.
    pub fn vad_threshold(&self) -> f32 {
        // sensitivity 0.0 -> 0.020 (needs louder speech), 1.0 -> 0.005 (picks up quiet speech)
        (0.020 - 0.015 * self.sensitivity.clamp(0.0, 1.0)).clamp(0.003, 0.05)
    }

    /// Minimum token similarity for fuzzy wake-phrase matches.
    pub fn wake_fuzzy_ratio(&self) -> f32 {
        // sensitivity 0.0 -> 0.90 (strict), 1.0 -> 0.60 (tolerant)
        (0.90 - 0.30 * self.sensitivity.clamp(0.0, 1.0)).clamp(0.55, 0.95)
    }

    /// Whether an API key is available (settings or environment).
    pub fn has_api_key(&self) -> bool {
        !self.openai_api_key.trim().is_empty() || std::env::var("OPENAI_API_KEY").is_ok()
    }

    /// The effective LLM provider after resolving `Auto`.
    pub fn effective_llm_provider(&self) -> LlmProviderKind {
        match self.llm_provider {
            LlmProviderKind::Auto => {
                if self.has_api_key() {
                    LlmProviderKind::Cloud
                } else {
                    LlmProviderKind::Offline
                }
            }
            other => other,
        }
    }

    /// Resolve the API key: settings first, then environment.
    pub fn resolved_api_key(&self) -> Option<String> {
        let key = self.openai_api_key.trim();
        if !key.is_empty() {
            return Some(key.to_string());
        }
        std::env::var("OPENAI_API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
    }

    /// Resolve the base URL: settings first, then environment, then default.
    pub fn resolved_base_url(&self) -> String {
        let from_settings = self.openai_base_url.trim();
        if !from_settings.is_empty() {
            return from_settings.trim_end_matches('/').to_string();
        }
        std::env::var("OPENAI_BASE_URL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(|v| v.trim_end_matches('/').to_string())
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string())
    }

    /// Resolve the model: settings first, then environment, then default.
    pub fn resolved_model(&self) -> String {
        let from_settings = self.openai_model.trim();
        if !from_settings.is_empty() {
            return from_settings.to_string();
        }
        std::env::var("OPENAI_MODEL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "gpt-4o-mini".to_string())
    }
}
