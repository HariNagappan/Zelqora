//! Whisper model discovery and download.
//!
//! Search order: `NOVA_MODELS_DIR` → Nova app-data dir → repo `models/`
//! (development). Downloads go to the first usable location.

use std::path::PathBuf;

pub const DEFAULT_MODEL_FILE: &str = "ggml-tiny.en.bin";
pub const DEFAULT_MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin";

/// Minimum plausible size for a whisper model (guards against error pages).
const MIN_MODEL_BYTES: u64 = 20 * 1024 * 1024;

/// Candidate paths for the whisper model, in priority order.
pub fn candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(dir) = std::env::var("NOVA_MODELS_DIR") {
        paths.push(PathBuf::from(dir).join(DEFAULT_MODEL_FILE));
    }
    if let Some(data_dir) = dirs::data_dir() {
        paths.push(
            data_dir
                .join("Nova")
                .join("models")
                .join(DEFAULT_MODEL_FILE),
        );
    }
    // Repo-relative, development builds: <project>/models/... — the running
    // executable lives at src-tauri/target/<profile>/nova.exe, so walk up 3.
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..3 {
            dir = dir.and_then(|d| d.parent().map(|p| p.to_path_buf()));
        }
        if let Some(root) = dir {
            paths.push(root.join("models").join(DEFAULT_MODEL_FILE));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("models").join(DEFAULT_MODEL_FILE));
    }

    paths
}

/// First existing model file among the candidates.
pub fn resolve_model_path() -> Option<PathBuf> {
    candidate_paths().into_iter().find(|p| p.is_file())
}

/// Directory where a downloaded model should live.
pub fn download_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("NOVA_MODELS_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(data_dir) = dirs::data_dir() {
        return data_dir.join("Nova").join("models");
    }
    PathBuf::from("models")
}

/// Download the default model if it is not already present.
pub async fn download_default_model() -> Result<PathBuf, String> {
    if let Some(existing) = resolve_model_path() {
        return Ok(existing);
    }

    let dir = download_dir();
    let target = dir.join(DEFAULT_MODEL_FILE);
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {dir:?}: {e}"))?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(DEFAULT_MODEL_URL)
        .send()
        .await
        .map_err(|e| format!("download failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("download failed: HTTP {}", response.status()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("download failed: {e}"))?;
    if (bytes.len() as u64) < MIN_MODEL_BYTES {
        return Err(format!(
            "downloaded file is unexpectedly small ({} bytes)",
            bytes.len()
        ));
    }

    let tmp = target.with_extension("download");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("could not write model: {e}"))?;
    std::fs::rename(&tmp, &target).map_err(|e| format!("could not finalize model: {e}"))?;
    eprintln!(
        "[nova] whisper model downloaded to {} ({:.1} MB)",
        target.display(),
        bytes.len() as f64 / 1_048_576.0
    );
    Ok(target)
}
