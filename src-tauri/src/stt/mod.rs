//! Speech-to-text: a shared whisper.cpp worker (wake checks + local command
//! transcription) plus an optional OpenAI-compatible cloud recognizer.
//!
//! Privacy: wake-word audio is ALWAYS transcribed locally; the cloud
//! recognizer only ever receives the post-wake command segment, and only when
//! the user explicitly selects it in Settings.

pub mod cloud;
pub mod model;

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

pub use cloud::CloudRecognizer;

/// Speech recognition abstraction (kept small so providers can be swapped).
pub trait SpeechRecognizer: Send + Sync {
    fn name(&self) -> &'static str;
    /// Transcribe 16 kHz mono f32 audio. Blocking.
    fn transcribe(&self, samples: &[f32]) -> Option<String>;
}

/// Handle used by the audio pipeline to talk to the whisper worker thread.
#[derive(Clone)]
pub struct WhisperHandle {
    tx: Sender<WhisperJob>,
}

enum WhisperJob {
    Transcribe {
        samples: Vec<f32>,
        reply: Sender<Option<String>>,
    },
}

impl WhisperHandle {
    /// Spawn the worker for `model_path`; returns (handle, model_loaded).
    pub fn spawn(model_path: std::path::PathBuf) -> (Self, bool) {
        let (tx, rx) = mpsc::channel::<WhisperJob>();
        let (ready_tx, ready_rx) = mpsc::channel::<bool>();
        std::thread::Builder::new()
            .name("nova-whisper".into())
            .spawn(move || whisper_worker(model_path, rx, ready_tx))
            .expect("failed to spawn the whisper worker thread");
        let loaded = ready_rx
            .recv_timeout(Duration::from_secs(90))
            .unwrap_or(false);
        (WhisperHandle { tx }, loaded)
    }

    /// Blocking transcription of 16 kHz mono f32 samples.
    pub fn transcribe(&self, samples: Vec<f32>) -> Option<String> {
        if samples.len() < 1_600 {
            return None;
        }
        let (reply, reply_rx) = mpsc::channel();
        self.tx
            .send(WhisperJob::Transcribe { samples, reply })
            .ok()?;
        reply_rx
            .recv_timeout(Duration::from_secs(60))
            .ok()
            .flatten()
    }
}

fn whisper_worker(model_path: std::path::PathBuf, rx: Receiver<WhisperJob>, ready: Sender<bool>) {
    // Route whisper.cpp/ggml logs through the crate's hooks; without a log
    // backend configured these become no-ops, keeping the console clean.
    whisper_rs::install_logging_hooks();

    let context = match WhisperContext::new_with_params(
        &model_path,
        WhisperContextParameters::default(),
    ) {
        Ok(context) => context,
        Err(err) => {
            eprintln!(
                "[nova] failed to load whisper model {}: {err}",
                model_path.display()
            );
            let _ = ready.send(false);
            return;
        }
    };
    let mut state = match context.create_state() {
        Ok(state) => state,
        Err(err) => {
            eprintln!("[nova] failed to create whisper state: {err}");
            let _ = ready.send(false);
            return;
        }
    };
    let _ = ready.send(true);

    while let Ok(job) = rx.recv() {
        match job {
            WhisperJob::Transcribe { samples, reply } => {
                let _ = reply.send(run_transcription(&mut state, &samples));
            }
        }
    }
}

fn run_transcription(state: &mut WhisperState, samples: &[f32]) -> Option<String> {
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("en"));
    params.set_translate(false);
    params.set_no_context(true);
    params.set_single_segment(true);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_suppress_blank(true);
    params.set_temperature(0.0);

    state.full(params, samples).ok()?;

    let mut text = String::new();
    for index in 0..state.full_n_segments() {
        if let Some(segment) = state.get_segment(index) {
            if let Ok(part) = segment.to_str() {
                text.push_str(part);
            }
        }
    }
    clean_transcript(&text)
}

/// Whisper emits bracketed non-speech markers (`[BLANK_AUDIO]`, `[Silence]`, …).
/// They are not speech, so they must not reach the parser as a "heard" command;
/// the runtime turns an empty transcript into the designed "I didn't catch that"
/// recovery instead.
pub fn clean_transcript(raw: &str) -> Option<String> {
    const MARKERS: &[&str] = &[
        "blank_audio",
        "blank audio",
        "silence",
        "no speech",
        "nospeech",
        "inaudible",
        "unintelligible",
        "music",
        "noise",
        "background noise",
        "applause",
        "laughter",
        "beep",
    ];

    // Bracketed spans are removed when their content is a marker or empty;
    // anything else inside brackets is real speech and is kept (without the
    // brackets). Unbracketed words are never treated as markers: "add music to
    // my list" is a task, not a non-speech event.
    let mut out = String::new();
    let mut span = String::new();
    let mut in_span = false;
    for ch in raw.chars() {
        match ch {
            '[' | '(' | '<' if !in_span => {
                in_span = true;
                span.clear();
            }
            ']' | ')' | '>' if in_span => {
                in_span = false;
                let inner = span.trim();
                let normalized = inner.trim_matches('|').replace('_', " ").to_lowercase();
                let is_marker = inner.is_empty()
                    || inner.starts_with('|')
                    || MARKERS.iter().any(|marker| normalized.trim() == *marker);
                if !is_marker {
                    out.push(' ');
                    out.push_str(inner);
                }
            }
            _ => {
                if in_span {
                    span.push(ch);
                } else {
                    out.push(ch);
                }
            }
        }
    }
    if in_span {
        out.push_str(&span);
    }

    let cleaned = out
        .split_whitespace()
        .filter(|token| !matches!(*token, "[" | "]" | "(" | ")" | "<" | ">"))
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

/// Encode 16 kHz mono f32 samples as a 16-bit PCM WAV file (for cloud STT).
pub fn encode_wav_16k_mono(samples: &[f32]) -> Result<Vec<u8>, hound::Error> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)?;
        for &sample in samples {
            let clamped = sample.clamp(-1.0, 1.0);
            writer.write_sample((clamped * i16::MAX as f32) as i16)?;
        }
        writer.finalize()?;
    }
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_encoding_produces_valid_header() {
        let samples = vec![0.0f32; 16_000];
        let wav = encode_wav_16k_mono(&samples).unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        // 44-byte header + 2 bytes per sample
        assert_eq!(wav.len(), 44 + 16_000 * 2);
    }

    #[test]
    fn non_speech_markers_are_not_transcripts() {
        // Regression: whisper returned "[BLANK_AUDIO]" and Nova tried to run it
        // as a command instead of asking the user to repeat themselves.
        assert_eq!(clean_transcript("[BLANK_AUDIO]"), None);
        assert_eq!(clean_transcript("[ Silence ]"), None);
        assert_eq!(clean_transcript("<|nospeech|>"), None);
        assert_eq!(clean_transcript("   "), None);
        // Markers mixed with real speech keep the speech.
        assert_eq!(
            clean_transcript("[BLANK_AUDIO] add buy milk"),
            Some("add buy milk".to_string())
        );
        // Ordinary speech is untouched.
        assert_eq!(
            clean_transcript("add buy milk to my list"),
            Some("add buy milk to my list".to_string())
        );
        assert_eq!(
            clean_transcript("Hey Nova, add a task"),
            Some("Hey Nova, add a task".to_string())
        );
        // Unbracketed words that merely look like markers are real speech.
        assert_eq!(
            clean_transcript("add music to my list"),
            Some("add music to my list".to_string())
        );
        assert_eq!(
            clean_transcript("remind me about the silence"),
            Some("remind me about the silence".to_string())
        );
    }
}
