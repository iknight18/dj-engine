//! Pick crossfade length, variant, and snapped start times for a pair of tracks.

use crate::camelot::{CamelotKey, KeyMatch};
use crate::phrase::phrase_boundaries;
use crate::sidecar::Sidecar;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransitionPlan {
    pub outgoing_crossfade_start_seconds: f64,
    pub incoming_crossfade_start_seconds: f64,
    /// 0 means hard cut (no crossfade). Other values: 8, 16, 32 bars.
    pub length_bars: usize,
    pub use_eq_swap: bool,
    /// True iff the planner judged the pair too far apart in BPM/confidence to
    /// beatmatch. HardCut should skip its TempoAlign phase when this is set.
    pub skip_tempo_align: bool,
}

// Tunable thresholds. Adjust by ear after listening to several hours of output.
const HARD_CUT_ENERGY_DELTA: f64 = 0.50; // was 0.35
const QUICK_CUT_ENERGY_DELTA: f64 = 0.20;
const LONG_BLEND_BPM_DELTA: f64 = 0.02;
const LONG_BLEND_ENERGY_DELTA: f64 = 0.10;
const PHRASE_BARS: usize = 16;

pub fn plan_transition(outgoing: &Sidecar, incoming: &Sidecar) -> TransitionPlan {
    // Gate: if the BPM ratio is too wide or either track's confidence is too low,
    // force a hard cut and skip TempoAlign so the incoming plays at its natural BPM.
    let out_bpm = outgoing.bpm.unwrap_or(120.0);
    let inc_bpm = incoming.bpm.unwrap_or(out_bpm);
    let ratio = out_bpm / inc_bpm.max(1.0);
    let ratio_abs_delta = (ratio - 1.0).abs();
    let out_conf = outgoing.bpm_confidence.unwrap_or(1.0);
    let inc_conf = incoming.bpm_confidence.unwrap_or(1.0);
    let bpm_ratio_too_wide = ratio_abs_delta > 0.08;
    let confidence_too_low = out_conf < 0.7 || inc_conf < 0.7;
    let skip_tempo_align = bpm_ratio_too_wide || confidence_too_low;
    let (length_bars, use_eq_swap) = if skip_tempo_align {
        (0usize, false)
    } else {
        pick_length_and_variant(outgoing, incoming)
    };

    let raw_outgoing_start = outgoing.outro_start_seconds.unwrap_or(0.0);
    let raw_incoming_start = incoming.intro_end_seconds.unwrap_or(0.0);

    let outgoing_boundaries = phrase_boundaries(outgoing, PHRASE_BARS);
    let incoming_boundaries = phrase_boundaries(incoming, PHRASE_BARS);

    let outgoing_crossfade_start_seconds =
        nearest(&outgoing_boundaries, raw_outgoing_start).unwrap_or(raw_outgoing_start);

    let incoming_crossfade_start_seconds = incoming_boundaries
        .first()
        .copied()
        .unwrap_or(raw_incoming_start);

    TransitionPlan {
        outgoing_crossfade_start_seconds,
        incoming_crossfade_start_seconds,
        length_bars,
        use_eq_swap,
        skip_tempo_align,
    }
}

fn nearest(boundaries: &[f64], target: f64) -> Option<f64> {
    boundaries.iter().copied().min_by(|a, b| {
        (*a - target)
            .abs()
            .partial_cmp(&(*b - target).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn pick_length_and_variant(out: &Sidecar, inc: &Sidecar) -> (usize, bool) {
    let is_beatmatchable = inc.beatmatchable();
    let is_phrase_aligned = inc.phrase_aligned();

    // Non-beatmatchable → safe 16-bar volume crossfade, no EQ, no hard cut.
    if !is_beatmatchable {
        return (16, false);
    }

    let out_bpm = out.bpm.unwrap_or(120.0);
    let inc_bpm = inc.bpm.unwrap_or(out_bpm);
    let bpm_delta = ((inc_bpm - out_bpm).abs() / out_bpm.max(1.0)).abs();

    let out_energy = out.energy_mean.unwrap_or(0.5);
    let inc_energy = inc.energy_mean.unwrap_or(out_energy);
    let energy_delta = (inc_energy - out_energy).abs();

    let key_match = match (
        out.key.as_deref().and_then(CamelotKey::parse),
        inc.key.as_deref().and_then(CamelotKey::parse),
    ) {
        (Some(a), Some(b)) => a.compatibility(&b),
        _ => KeyMatch::None,
    };

    let bars = if energy_delta > HARD_CUT_ENERGY_DELTA {
        0
    } else if energy_delta > QUICK_CUT_ENERGY_DELTA
        || matches!(key_match, KeyMatch::Relative | KeyMatch::None)
    {
        8
    } else if bpm_delta <= LONG_BLEND_BPM_DELTA
        && energy_delta <= LONG_BLEND_ENERGY_DELTA
        && matches!(key_match, KeyMatch::Identical)
    {
        32
    } else {
        16
    };

    // EQ swap gated on phrase alignment (so non-aligned blends are clean volume crossfades).
    let use_eq_swap = bars >= 8 && is_phrase_aligned;
    (bars, use_eq_swap)
}
