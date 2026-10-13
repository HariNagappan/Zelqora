//! Audio capture engine: cpal input stream -> 16 kHz mono f32 -> VAD ->
//! local whisper transcription -> runtime events.
//!
//! The cpal stream lives on its own thread (it is not `Send` on Windows);
//! a second thread runs the capture state machine (wake word / command).

pub mod resample;
pub mod vad;

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tauri::{AppHandle, Manager};

use crate::error::{NovaError, NovaResult};
use crate::events::{RuntimeEvent, TranscriptSource};
use crate::settings::{Settings, SttProvider};
use crate::state::AppState;
use crate::stt::{CloudRecognizer, SpeechRecognizer, WhisperHandle};
use crate::wakeword::match_wake;

use resample::StreamingResampler;
use vad::{Vad, VadEvent};

/// Everything downstream of the microphone works at 16 kHz mono.
pub const TARGET_RATE: f32 = 16_000.0;
/// Pre-roll kept ahead of a detected speech onset (~300 ms).
const PRE_ROLL_LEN: usize = 4_800;
/// Longest audio window handed to whisper for a wake-word check.
const WAKE_MAX_SAMPLES: usize = (3.2 * TARGET_RATE) as usize;
/// Hard cap on a command recording (8 s).
const COMMAND_MAX_SAMPLES: usize = (8.0 * TARGET_RATE) as usize;
/// If no speech starts within this time after wake/PTT, give up.
const COMMAND_NO_SPEECH_TIMEOUT: Duration = Duration::from_secs(6);

/// Handle to a running audio engine. Dropping it stops capture.
pub struct AudioEngine {
    control: Sender<ControlMessage>,
    join: Option<std::thread::JoinHandle<()>>,
}

enum ControlMessage {
    Stop,
}

impl AudioEngine {
    /// Start capturing from the configured (or default) microphone.
    pub fn start(app: AppHandle) -> NovaResult<Self> {
        let (control_tx, control_rx) = mpsc::channel::<ControlMessage>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

        let join = std::thread::spawn(move || audio_thread_main(app, control_rx, ready_tx));

        match ready_rx.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(())) => Ok(AudioEngine {
                control: control_tx,
                join: Some(join),
            }),
            Ok(Err(message)) => Err(NovaError::Audio(message)),
            Err(_) => Err(NovaError::Audio("audio initialization timed out".into())),
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.control.send(ControlMessage::Stop);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn audio_thread_main(
    app: AppHandle,
    control_rx: Receiver<ControlMessage>,
    ready_tx: Sender<Result<(), String>>,
) {
    let device = match pick_device(&app) {
        Ok(device) => device,
        Err(message) => {
            let _ = ready_tx.send(Err(message));
            return;
        }
    };

    let supported = match device.default_input_config() {
        Ok(supported) => supported,
        Err(err) => {
            let _ = ready_tx.send(Err(format!("no usable microphone config: {err}")));
            return;
        }
    };

    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let native_rate = config.sample_rate as f32;
    let channels = config.channels as usize;

    let (audio_tx, audio_rx) = mpsc::channel::<Vec<f32>>();
    // A stream can die mid-run (device change, exclusive access, driver reset).
    // The error callback reports it so the runtime can rebuild the engine
    // instead of leaving Nova listening to nothing.
    let (stream_err_tx, stream_err_rx) = mpsc::channel::<String>();
    let err_fn = move |err: cpal::Error| {
        // Buffer xruns occur under system load and are not actionable.
        if err.kind() != cpal::ErrorKind::Xrun {
            eprintln!("[nova] audio stream error: {err}");
            let _ = stream_err_tx.send(err.to_string());
        }
    };

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            config.clone(),
            move |data: &[f32], _| {
                send_mono(
                    &audio_tx,
                    data.chunks(channels)
                        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32),
                );
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            config.clone(),
            move |data: &[i16], _| {
                send_mono(
                    &audio_tx,
                    data.chunks(channels).map(|frame| {
                        frame.iter().map(|&s| s as f32 / 32768.0).sum::<f32>()
                            / frame.len() as f32
                    }),
                );
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            config.clone(),
            move |data: &[u16], _| {
                send_mono(
                    &audio_tx,
                    data.chunks(channels).map(|frame| {
                        frame
                            .iter()
                            .map(|&s| (s as f32 - 32768.0) / 32768.0)
                            .sum::<f32>()
                            / frame.len() as f32
                    }),
                );
            },
            err_fn,
            None,
        ),
        other => {
            let _ = ready_tx.send(Err(format!("unsupported sample format: {other:?}")));
            return;
        }
    };

    let stream = match stream {
        Ok(stream) => stream,
        Err(err) => {
            let _ = ready_tx.send(Err(format!("could not open the microphone: {err}")));
            return;
        }
    };
    if let Err(err) = stream.play() {
        let _ = ready_tx.send(Err(format!("could not start the microphone: {err}")));
        return;
    }

    // The capture state machine consumes native-rate mono chunks.
    let pipeline = std::thread::spawn({
        let app = app.clone();
        move || pipeline_main(app, audio_rx, native_rate)
    });

    let _ = ready_tx.send(Ok(()));

    // Keep the stream alive until asked to stop, or until it dies.
    let mut failure: Option<String> = None;
    loop {
        if let Ok(message) = stream_err_rx.try_recv() {
            failure = Some(message);
            break;
        }
        match control_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(ControlMessage::Stop) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    drop(stream);
    // Dropping the stream drops the audio callback (and its sender clone), which
    // ends the capture pipeline thread cleanly.
    let _ = pipeline;

    if let Some(message) = failure {
        app.state::<AppState>()
            .send_event(RuntimeEvent::AudioStreamFailed { message });
    }
}

fn send_mono(tx: &Sender<Vec<f32>>, samples: impl Iterator<Item = f32>) {
    let mono: Vec<f32> = samples.collect();
    let _ = tx.send(mono);
}

fn pick_device(app: &AppHandle) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    let wanted = app.state::<AppState>().settings_snapshot().microphone;

    if let Some(wanted) = wanted {
        if let Ok(devices) = host.input_devices() {
            for device in devices {
                let matches = device
                    .description()
                    .map(|description| description.name() == wanted)
                    .unwrap_or(false);
                if matches {
                    return Ok(device);
                }
            }
        }
        eprintln!("[nova] configured microphone '{wanted}' not found; using the default device");
    }

    host.default_input_device()
        .ok_or_else(|| "no microphone available".to_string())
}

// ---------------------------------------------------------------------------
// Capture state machine (wake word -> command)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureMode {
    Wake,
    Command,
}

fn pipeline_main(app: AppHandle, audio_rx: Receiver<Vec<f32>>, native_rate: f32) {
    let mut resampler = StreamingResampler::new(native_rate, TARGET_RATE);
    let mut vad = Vad::new();
    let mut mode = CaptureMode::Wake;
    let mut pre_roll: Vec<f32> = Vec::with_capacity(PRE_ROLL_LEN);
    let mut segment: Vec<f32> = Vec::new();
    let mut command_timer: Option<Instant> = None;
    let mut last_level_at = Instant::now() - Duration::from_secs(1);
    let mut smoothed_level = 0.0f32;

    let mut settings = app.state::<AppState>().settings_snapshot();
    let mut tick: u32 = 0;

    while let Ok(chunk) = audio_rx.recv() {
        tick = tick.wrapping_add(1);
        if tick % 25 == 1 {
            settings = app.state::<AppState>().settings_snapshot();
        }

        let flags = read_control_flags(&app);
        let (paused, request_capture, request_finalize, request_cancel, mic_test) = (
            flags.paused,
            flags.request_capture,
            flags.request_finalize,
            flags.cancel,
            flags.mic_test,
        );
        if request_cancel {
            // The user stopped the interaction: drop whatever we captured and
            // go back to waiting for the wake word. Nothing is transcribed.
            vad.reset();
            segment.clear();
            pre_roll.clear();
            command_timer = None;
            mode = CaptureMode::Wake;
            continue;
        }
        if paused {
            vad.reset();
            segment.clear();
            pre_roll.clear();
            continue;
        }

        if request_capture && mode != CaptureMode::Command {
            mode = CaptureMode::Command;
            segment.clear();
            command_timer = Some(Instant::now());
        }

        if request_finalize && mode == CaptureMode::Command && !segment.is_empty() {
            let taken = std::mem::take(&mut segment);
            handle_segment(&app, &settings, &mut mode, &mut command_timer, taken);
        }

        let samples = resampler.process(&chunk);

        // Live input level for the overlay waveform and the microphone test.
        // Only emitted while Nova is actually listening to a command or while
        // Settings is running its test — the wake-word buffer stays silent.
        if (mode == CaptureMode::Command || mic_test) && last_level_at.elapsed() >= Duration::from_millis(45) {
            last_level_at = Instant::now();
            let raw = vad::rms(&samples).clamp(0.0, 1.0);
            // Fast attack, slow release so the visualisation doesn't flicker.
            smoothed_level = if raw > smoothed_level {
                raw
            } else {
                smoothed_level * 0.82 + raw * 0.18
            };
            app.state::<AppState>()
                .ctl
                .lock()
                .expect("ctl lock poisoned")
                .last_level = smoothed_level;
            crate::events::emit_nova(
                &app,
                crate::events::EVT_LEVEL,
                crate::events::LevelPayload {
                    level: smoothed_level,
                    testing: mic_test,
                },
            );
        }

        for frame in samples.chunks(vad::FRAME_LEN) {
            let ev = vad.process(frame, settings.vad_threshold());
            match ev {
                VadEvent::SpeechStart => {
                    segment.clear();
                    segment.extend_from_slice(&pre_roll);
                    segment.extend_from_slice(frame);
                    pre_roll.clear();
                }
                VadEvent::Speech => {
                    if !segment.is_empty() {
                        segment.extend_from_slice(frame);
                    }
                }
                VadEvent::SpeechEnd => {
                    if !segment.is_empty() {
                        let taken = std::mem::take(&mut segment);
                        handle_segment(&app, &settings, &mut mode, &mut command_timer, taken);
                    }
                }
                VadEvent::Silence => {
                    if segment.is_empty() {
                        pre_roll.extend_from_slice(frame);
                        if pre_roll.len() > PRE_ROLL_LEN {
                            let excess = pre_roll.len() - PRE_ROLL_LEN;
                            pre_roll.drain(0..excess);
                        }
                    }
                }
            }

            // Enforce per-mode caps.
            if !segment.is_empty() {
                let cap = match mode {
                    CaptureMode::Wake => WAKE_MAX_SAMPLES,
                    CaptureMode::Command => COMMAND_MAX_SAMPLES,
                };
                if segment.len() >= cap {
                    let taken = std::mem::take(&mut segment);
                    handle_segment(&app, &settings, &mut mode, &mut command_timer, taken);
                }
            }

            // Command capture that never received speech times out.
            if mode == CaptureMode::Command && segment.is_empty() {
                if let Some(timer) = command_timer {
                    if timer.elapsed() > COMMAND_NO_SPEECH_TIMEOUT {
                        mode = CaptureMode::Wake;
                        command_timer = None;
                        app.state::<AppState>()
                            .send_event(RuntimeEvent::CommandTimeout);
                    }
                }
            }
        }
    }
}

/// Flags the capture state machine reads once per audio chunk.
struct ControlFlags {
    paused: bool,
    request_capture: bool,
    request_finalize: bool,
    cancel: bool,
    mic_test: bool,
}

fn read_control_flags(app: &AppHandle) -> ControlFlags {
    let state = app.state::<AppState>();
    let mut ctl = state.ctl.lock().expect("ctl lock poisoned");
    ControlFlags {
        paused: ctl.paused,
        request_capture: std::mem::take(&mut ctl.request_command_capture),
        request_finalize: std::mem::take(&mut ctl.request_finalize_capture),
        cancel: std::mem::take(&mut ctl.request_cancel),
        mic_test: ctl.mic_test,
    }
}

fn whisper_handle(app: &AppHandle) -> Option<WhisperHandle> {
    let state = app.state::<AppState>();
    let ctl = state.ctl.lock().expect("ctl lock poisoned");
    ctl.whisper.clone()
}

fn handle_segment(
    app: &AppHandle,
    settings: &Settings,
    mode: &mut CaptureMode,
    command_timer: &mut Option<Instant>,
    segment: Vec<f32>,
) {
    let duration_s = segment.len() as f32 / TARGET_RATE;
    if duration_s < 0.2 {
        return;
    }

    match *mode {
        CaptureMode::Wake => {
            if !settings.wake_enabled {
                return;
            }
            // Wake checks always run locally — audio never goes to a cloud
            // service while Nova is merely waiting for its name.
            let Some(whisper) = whisper_handle(app) else {
                return;
            };
            if let Some(text) = whisper.transcribe(segment) {
                let matched = match_wake(&text, &settings.wake_word, settings.wake_fuzzy_ratio());
                if matched.matched {
                    eprintln!("[nova] wake word detected: {text:?}");
                    app.state::<AppState>()
                        .send_event(RuntimeEvent::WakeDetected {
                            residual_command: matched.command,
                        });
                    *mode = CaptureMode::Command;
                    *command_timer = Some(Instant::now());
                }
            }
        }
        CaptureMode::Command => {
            *command_timer = None;
            let segment = trim_trailing_silence(segment, settings.vad_threshold());
            // Announce the recognition step so the overlay can show
            // "Transcribing…" instead of claiming it is still listening.
            app.state::<AppState>()
                .send_event(RuntimeEvent::TranscribeStart {
                    source: TranscriptSource::Voice,
                });
            let text = transcribe_command(app, settings, segment).unwrap_or_default();
            eprintln!("[nova] command transcript: {text:?}");
            app.state::<AppState>()
                .send_event(RuntimeEvent::TranscriptReady {
                    text,
                    source: TranscriptSource::Voice,
                });

            // Re-arm only while a question is pending; otherwise back to wake.
            let awaiting = {
                let state = app.state::<AppState>();
                let ctl = state.ctl.lock().expect("ctl lock poisoned");
                ctl.awaiting_confirmation || ctl.awaiting_clarification
            };
            if awaiting {
                *command_timer = Some(Instant::now());
            } else {
                *mode = CaptureMode::Wake;
            }
        }
    }
}

/// Route command transcription through the configured STT provider.
fn transcribe_command(app: &AppHandle, settings: &Settings, segment: Vec<f32>) -> Option<String> {
    if settings.stt_provider == SttProvider::Cloud {
        match CloudRecognizer::from_settings(settings) {
            Ok(recognizer) => return recognizer.transcribe(&segment),
            Err(err) => {
                eprintln!("[nova] cloud STT unavailable ({err}); using local whisper");
            }
        }
    }
    whisper_handle(app)?.transcribe(segment)
}

/// Trim trailing silence (up to 1 s) so whisper does not decode dead air.
fn trim_trailing_silence(mut segment: Vec<f32>, threshold: f32) -> Vec<f32> {
    let step = (0.1 * TARGET_RATE) as usize;
    let min_len = (0.3 * TARGET_RATE) as usize;
    while segment.len() > min_len {
        let tail_start = segment.len() - step;
        if vad::rms(&segment[tail_start..]) < threshold * 0.8 {
            segment.truncate(tail_start);
        } else {
            break;
        }
    }
    segment
}
