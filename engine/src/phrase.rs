//! Derive 16-bar phrase boundaries from a sidecar. Pure data.

use crate::sidecar::Sidecar;

/// Return ascending timestamps of phrase boundaries inside `sc`'s body.
///
/// Algorithm:
/// 1. Anchor on the first downbeat at or after `sc.intro_end_seconds` (or 0 if None).
/// 2. From that anchor index, every `phrase_bars`-th downbeat is a phrase boundary.
/// 3. Skip any boundaries at or after `sc.outro_start_seconds` (if set).
///
/// Returns an empty Vec if there are no downbeats or the body is too short.
pub fn phrase_boundaries(sc: &Sidecar, phrase_bars: usize) -> Vec<f64> {
    // Prefer explicit sidecar-set boundaries (v2 analyzer with phrase confidence above threshold).
    if let Some(explicit) = &sc.phrase_boundaries_seconds {
        if !explicit.is_empty() {
            return explicit.clone();
        }
    }
    if phrase_bars == 0 || sc.downbeats_seconds.is_empty() {
        return Vec::new();
    }
    let intro_end = sc.intro_end_seconds.unwrap_or(0.0);
    let outro_start = sc.outro_start_seconds.unwrap_or(f64::INFINITY);

    let anchor_idx = sc
        .downbeats_seconds
        .iter()
        .position(|d| *d >= intro_end)
        .unwrap_or(sc.downbeats_seconds.len());

    let mut out = Vec::new();
    let mut idx = anchor_idx;
    while idx < sc.downbeats_seconds.len() {
        let t = sc.downbeats_seconds[idx];
        if t >= outro_start {
            break;
        }
        out.push(t);
        idx += phrase_bars;
    }
    out
}
