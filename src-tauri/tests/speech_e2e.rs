//! End-to-end local speech test: real whisper.cpp transcription of a
//! generated "Hey Nova …" WAV fixture, then wake-phrase matching.
//!
//! Requires the downloaded model and the fixture; run explicitly with:
//!
//! ```bash
//! npm run models
//! cargo test --manifest-path src-tauri/Cargo.toml -- --ignored
//! ```

use std::path::PathBuf;

use nova_lib::stt::WhisperHandle;
use nova_lib::wakeword::match_wake;

#[test]
#[ignore = "requires models/ggml-tiny.en.bin (npm run models) and fixtures/hey_nova.wav"]
fn transcribes_generated_hey_nova_fixture() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let model = manifest.join("..").join("models").join("ggml-tiny.en.bin");
    assert!(
        model.is_file(),
        "model missing at {} — run `npm run models` first",
        model.display()
    );

    let fixture = manifest
        .join("..")
        .join("fixtures")
        .join("hey_nova.wav");
    assert!(
        fixture.is_file(),
        "fixture missing at {} — generate it with Windows TTS",
        fixture.display()
    );

    let mut reader = hound::WavReader::open(&fixture).expect("open wav fixture");
    let spec = reader.spec();
    assert_eq!(spec.channels, 1, "fixture must be mono");
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|sample| sample.expect("read sample") as f32 / 32768.0)
        .collect();

    let samples = if spec.sample_rate != 16_000 {
        let mut resampler =
            nova_lib::audio::resample::StreamingResampler::new(spec.sample_rate as f32, 16_000.0);
        resampler.process(&samples)
    } else {
        samples
    };

    let (whisper, loaded) = WhisperHandle::spawn(model);
    assert!(loaded, "whisper model failed to load");

    let text = whisper
        .transcribe(samples)
        .expect("transcription produced no text");
    eprintln!("[fixture transcription] {text:?}");

    let matched = match_wake(&text, "hey nova", 0.75);
    assert!(
        matched.matched,
        "wake phrase not detected in transcription: {text:?}"
    );

    // The fixture also contains a command after the wake word; the residual
    // should carry it through to the agent.
    if let Some(residual) = matched.command {
        eprintln!("[fixture residual command] {residual:?}");
        assert!(
            residual.contains("milk"),
            "expected the residual command to mention 'milk', got {residual:?}"
        );
    }
}
