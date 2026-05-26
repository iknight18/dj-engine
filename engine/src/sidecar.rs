//! Serde types matching schema/sidecar.schema.json.
//!
//! Optional fields reflect that an "error sidecar" (decoding failed) only has the
//! base identifying fields plus `error`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Sidecar {
    pub schema_version: u32,
    pub analyzer_version: String,
    pub source_path: String,
    pub source_mtime: f64,
    pub source_size_bytes: u64,

    #[serde(default)]
    pub error: Option<String>,

    pub duration_seconds: Option<f64>,
    pub sample_rate: Option<u32>,

    pub bpm: Option<f64>,
    pub bpm_confidence: Option<f64>,
    #[serde(default)]
    pub beats_seconds: Vec<f64>,
    #[serde(default)]
    pub downbeats_seconds: Vec<f64>,

    pub key: Option<String>,
    pub key_label: Option<String>,
    pub key_confidence: Option<f64>,

    #[serde(default)]
    pub energy_curve: Vec<f64>,
    pub energy_mean: Option<f64>,

    pub cue_in_seconds: Option<f64>,
    pub cue_out_seconds: Option<f64>,
    pub intro_end_seconds: Option<f64>,
    pub outro_start_seconds: Option<f64>,

    pub is_mixable: Option<bool>,
    #[serde(default)]
    pub mixable_reasons: Vec<String>,
}

impl Sidecar {
    /// True iff the sidecar has enough fields to actually play the track.
    pub fn is_playable(&self) -> bool {
        self.error.is_none()
            && self.bpm.is_some()
            && self.key.is_some()
            && self.cue_in_seconds.is_some()
            && self.cue_out_seconds.is_some()
            && self.intro_end_seconds.is_some()
            && self.outro_start_seconds.is_some()
    }
}
