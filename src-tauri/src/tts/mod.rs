//! Text-to-speech: abstraction + Windows (system voices) implementation.
//! Speaking happens on a worker thread so the agent loop never blocks.

use std::sync::mpsc::{self, Receiver, Sender};

use crate::events::RuntimeEvent;
use crate::settings::Settings;
use crate::state::AppState;

use tauri::{AppHandle, Manager};

/// Text-to-speech abstraction — providers can be swapped without touching
/// the agent.
pub trait TextToSpeech: Send {
    fn name(&self) -> &'static str;
    fn voices(&self) -> Vec<VoiceInfo>;
    fn set_voice(&mut self, voice_id: Option<&str>);
    fn set_rate(&mut self, rate: f32);
    fn set_volume(&mut self, volume: f32);
    /// Speak (blocking until the utterance finishes). Returns false on failure.
    fn speak(&mut self, text: &str) -> bool;
    fn stop(&mut self);
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VoiceInfo {
    pub id: String,
    pub name: String,
}

/// Windows implementation backed by the `tts` crate (SAPI / WinRT voices).
pub struct WindowsTts {
    inner: tts::Tts,
    voice_id: Option<String>,
    rate: f32,
    volume: f32,
}

impl WindowsTts {
    pub fn new(settings: &Settings) -> Option<Self> {
        let inner = tts::Tts::default().ok()?;
        let mut engine = WindowsTts {
            inner,
            voice_id: settings.tts_voice.clone(),
            rate: settings.tts_rate,
            volume: settings.tts_volume,
        };
        engine.apply_settings();
        Some(engine)
    }

    fn apply_settings(&mut self) {
        let _ = self.inner.set_rate(self.rate.clamp(0.1, 5.0));
        let _ = self.inner.set_volume(self.volume.clamp(0.0, 1.0));
        if let Some(wanted) = self.voice_id.clone() {
            if let Ok(voices) = self.inner.voices() {
                if let Some(voice) = voices
                    .into_iter()
                    .find(|v| v.id() == wanted || v.name() == wanted)
                {
                    let _ = self.inner.set_voice(&voice);
                }
            }
        }
    }
}

impl TextToSpeech for WindowsTts {
    fn name(&self) -> &'static str {
        "windows"
    }

    fn voices(&self) -> Vec<VoiceInfo> {
        self.inner
            .voices()
            .map(|voices| {
                voices
                    .into_iter()
                    .map(|v| VoiceInfo {
                        id: v.id(),
                        name: v.name(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn set_voice(&mut self, voice_id: Option<&str>) {
        self.voice_id = voice_id.map(|s| s.to_string());
        self.apply_settings();
    }

    fn set_rate(&mut self, rate: f32) {
        self.rate = rate;
        self.apply_settings();
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
        self.apply_settings();
    }

    fn speak(&mut self, text: &str) -> bool {
        matches!(self.inner.speak(text.to_string(), false), Ok(_))
    }

    fn stop(&mut self) {
        let _ = self.inner.stop();
    }
}

// ---------------------------------------------------------------------------
// Worker thread + handle
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct TtsHandle {
    tx: Sender<TtsJob>,
}

pub enum TtsJob {
    Speak { text: String },
    Update {
        voice_id: Option<String>,
        rate: f32,
        volume: f32,
    },
}

impl TtsHandle {
    pub fn spawn(app: AppHandle) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("nova-tts".into())
            .spawn(move || tts_worker(app, rx))
            .expect("failed to spawn the TTS worker thread");
        TtsHandle { tx }
    }

    pub fn speak(&self, text: String) {
        let _ = self.tx.send(TtsJob::Speak { text });
    }

    pub fn update(&self, settings: &Settings) {
        let _ = self.tx.send(TtsJob::Update {
            voice_id: settings.tts_voice.clone(),
            rate: settings.tts_rate,
            volume: settings.tts_volume,
        });
    }
}

fn tts_worker(app: AppHandle, rx: Receiver<TtsJob>) {
    let settings = app.state::<AppState>().settings_snapshot();

    let engine = WindowsTts::new(&settings);
    let Some(mut engine) = engine else {
        eprintln!("[nova] no system speech voices available; voice replies disabled");
        // Keep consuming jobs so producers never block, and keep the state
        // machine consistent by reporting completion immediately.
        while let Ok(job) = rx.recv() {
            if let TtsJob::Speak { .. } = job {
                app.state::<AppState>()
                    .send_event(RuntimeEvent::SpeechDone);
            }
        }
        return;
    };

    while let Ok(job) = rx.recv() {
        match job {
            TtsJob::Speak { text } => {
                if !text.trim().is_empty() {
                    let _ = engine.speak(&text);
                }
                app.state::<AppState>()
                    .send_event(RuntimeEvent::SpeechDone);
            }
            TtsJob::Update {
                voice_id,
                rate,
                volume,
            } => {
                engine.set_voice(voice_id.as_deref());
                engine.set_rate(rate);
                engine.set_volume(volume);
            }
        }
    }
}

/// List the system voices without owning a worker (used by Settings).
pub fn list_system_voices() -> Vec<VoiceInfo> {
    match tts::Tts::default() {
        Ok(engine) => engine
            .voices()
            .map(|voices| {
                voices
                    .into_iter()
                    .map(|v| VoiceInfo {
                        id: v.id(),
                        name: v.name(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    /// The Windows engine must at least enumerate without panicking.
    #[test]
    fn voice_listing_does_not_panic() {
        let voices = super::list_system_voices();
        // May legitimately be empty on a bare system; the call must not panic.
        let _ = voices.len();
    }
}
