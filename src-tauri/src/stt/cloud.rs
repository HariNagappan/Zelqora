//! OpenAI-compatible cloud speech-to-text (opt-in only; see the privacy note
//! in `stt/mod.rs`).

use std::time::Duration;

use crate::error::{NovaError, NovaResult};
use crate::settings::Settings;

use super::SpeechRecognizer;

pub struct CloudRecognizer {
    base_url: String,
    api_key: String,
    model: String,
}

impl CloudRecognizer {
    /// Build from settings; fails with `MissingApiKey` when no key is set.
    pub fn from_settings(settings: &Settings) -> NovaResult<Self> {
        let api_key = settings.resolved_api_key().ok_or(NovaError::MissingApiKey)?;
        let model = std::env::var("NOVA_STT_MODEL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "whisper-1".to_string());
        Ok(CloudRecognizer {
            base_url: settings.resolved_base_url(),
            api_key,
            model,
        })
    }

    pub async fn transcribe_async(&self, samples: &[f32]) -> NovaResult<Option<String>> {
        let wav = super::encode_wav_16k_mono(samples)?;

        let part = reqwest::multipart::Part::bytes(wav)
            .file_name("command.wav")
            .mime_str("audio/wav")
            .map_err(|e| NovaError::Network(e.to_string()))?;
        let form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("model", self.model.clone());

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(45))
            .build()
            .map_err(|e| NovaError::Network(e.to_string()))?;

        let response = client
            .post(format!("{}/audio/transcriptions", self.base_url))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await
            .map_err(|e| NovaError::Network(format!("cloud transcription failed: {e}")))?;

        if !response.status().is_success() {
            return Err(NovaError::Network(format!(
                "cloud transcription error: HTTP {}",
                response.status()
            )));
        }

        let payload: serde_json::Value = response
            .json()
            .await
            .map_err(|e| NovaError::Network(e.to_string()))?;
        let text = payload
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        Ok(if text.is_empty() { None } else { Some(text) })
    }
}

impl SpeechRecognizer for CloudRecognizer {
    fn name(&self) -> &'static str {
        "cloud"
    }

    fn transcribe(&self, samples: &[f32]) -> Option<String> {
        tauri::async_runtime::block_on(self.transcribe_async(samples))
            .ok()
            .flatten()
    }
}
