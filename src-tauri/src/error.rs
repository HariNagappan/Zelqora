use serde::Serialize;

/// Unified error type for Nova's backend.
#[derive(Debug, thiserror::Error)]
pub enum NovaError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("audio file error: {0}")]
    Wav(#[from] hound::Error),

    #[error("network error: {0}")]
    Network(String),

    #[error("speech recognition error: {0}")]
    Speech(String),

    #[error("audio error: {0}")]
    Audio(String),

    #[error("invalid command: {0}")]
    InvalidCommand(String),

    #[error("unsupported: {0}")]
    Unsupported(String),

    #[error("no api key configured")]
    MissingApiKey,

    #[error("{0}")]
    Other(String),
}

impl From<reqwest::Error> for NovaError {
    fn from(value: reqwest::Error) -> Self {
        NovaError::Network(value.to_string())
    }
}

impl Serialize for NovaError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type NovaResult<T> = Result<T, NovaError>;
