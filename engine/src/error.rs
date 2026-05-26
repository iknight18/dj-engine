//! Engine-wide error type.

use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("sidecar at {path:?} is invalid: {reason}")]
    InvalidSidecar { path: PathBuf, reason: String },

    #[error("ffmpeg subprocess failed: {0}")]
    Ffmpeg(String),

    #[error("icecast connection failed: {0}")]
    Icecast(String),

    #[error("library is empty (no usable sidecars)")]
    EmptyLibrary,

    #[error("config error: {0}")]
    Config(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, EngineError>;
