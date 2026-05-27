//! DJ Engine — Rust orchestration around ffmpeg subprocesses.

pub mod align;
pub mod automation;
pub mod buffer;
pub mod camelot;
pub mod config;
pub mod crossfade;
pub mod energy_avg;
pub mod eq;
pub mod error;
pub mod ffmpeg;
pub mod icecast_meta;
pub mod library;
pub mod logging_setup;
pub mod mixer;
pub mod phrase;
pub mod recency;
pub mod recipe;
pub mod selector;
pub mod sidecar;
pub mod supervisor;
pub mod transition;
