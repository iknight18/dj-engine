//! Serde types matching schema/sidecar.schema.json (v1 + v2).
//!
//! All v2-only fields are Option<...>; v1 sidecars deserialize cleanly with those
//! fields left as None. The `beatmatchable()` / `phrase_aligned()` helpers paper
//! over the v1/v2 difference for callers.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BandValues {
    pub bass: f64,
    pub mid: f64,
    pub high: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Stems {
    #[serde(default)]
    pub drum_density: Vec<f64>,
    #[serde(default)]
    pub bass_density: Vec<f64>,
    #[serde(default)]
    pub vocal_density: Vec<f64>,
    #[serde(default)]
    pub vocal_segments: Vec<[f64; 2]>,
    #[serde(default)]
    pub energy_per_stem: HashMap<String, f64>,
    #[serde(default)]
    pub demucs_version: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BandCurves {
    pub bass: Vec<f64>,
    pub mid: Vec<f64>,
    pub high: Vec<f64>,
}

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

    // v2 additions
    #[serde(default)]
    pub phrase_boundaries_seconds: Option<Vec<f64>>,
    #[serde(default)]
    pub phrase_confidence: Option<f64>,
    #[serde(default)]
    pub band_energy_curves: Option<BandCurves>,
    #[serde(default)]
    pub band_energy_means: Option<BandValues>,
    #[serde(default)]
    pub is_beatmatchable: Option<bool>,
    #[serde(default)]
    pub is_phrase_aligned: Option<bool>,

    /// Stem-separation analytics (Demucs). Optional — older sidecars and
    /// error sidecars won't have it; the engine logs the data but does not
    /// drive selection on it (that's Phase D).
    #[serde(default)]
    pub stems: Option<Stems>,
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

    /// True if this track is safe to time-stretch + EQ-swap crossfade.
    /// v2 uses is_beatmatchable; v1 falls back to is_mixable.
    pub fn beatmatchable(&self) -> bool {
        self.is_beatmatchable.or(self.is_mixable).unwrap_or(false)
    }

    /// True if downbeats are accurate enough to snap to phrase boundaries.
    /// v2 uses is_phrase_aligned; v1 conservatively returns false.
    pub fn phrase_aligned(&self) -> bool {
        self.is_phrase_aligned.unwrap_or(false)
    }
}
