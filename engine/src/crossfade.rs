//! Equal-power crossfade curves and stereo PCM crossfade mixing.

use std::f64::consts::FRAC_PI_2;

use crate::eq::{apply_swept_filter, FilterKind};

/// Hint passed to `mix_crossfade_eq` so the EQ swap can adapt to the bass
/// content of the two clips. `outgoing_bass_mean` / `incoming_bass_mean` are
/// the per-band-RMS bass means from the analyzer sidecar (0..1).
#[derive(Debug, Clone, Copy)]
pub struct BassMixHint {
    pub outgoing_bass_mean: f64,
    pub incoming_bass_mean: f64,
}

/// Return (outgoing_gain, incoming_gain) for `t` in [0, 1].
/// `t = 0` -> full outgoing, no incoming. `t = 1` -> no outgoing, full incoming.
pub fn equal_power_curve(t: f64) -> (f64, f64) {
    let t = t.clamp(0.0, 1.0);
    let in_gain = (t * FRAC_PI_2).sin();
    let out_gain = (t * FRAC_PI_2).cos();
    (out_gain, in_gain)
}

/// Crossfade two interleaved-stereo f32 PCM buffers over `crossfade_frames` frames.
/// Returns a buffer the same length as the inputs.
pub fn mix_crossfade(outgoing: &[f32], incoming: &[f32], crossfade_frames: usize) -> Vec<f32> {
    let len = outgoing.len().min(incoming.len());
    let mut out = Vec::with_capacity(len);
    let crossfade_samples = crossfade_frames * 2;
    for i in 0..len {
        let t = if crossfade_samples > 0 {
            (i / 2) as f64 / crossfade_frames.max(1) as f64
        } else {
            1.0
        };
        let (g_out, g_in) = equal_power_curve(t);
        let sample = (outgoing[i] as f64) * g_out + (incoming[i] as f64) * g_in;
        out.push(sample as f32);
    }
    out
}

/// Equal-power crossfade with a 2-band EQ swap.
///
/// Behavior is tuned by the optional `bass_hint`:
///   - Both bass means < 0.3 → skip the EQ entirely and return a pure equal-power
///     crossfade. Running biquads on near-empty bass is just noise.
///   - Both bass means > 0.5 → use an earlier, tighter swap window so the basslines
///     don't fight in the overlap.
///   - Otherwise → the default centered swap window.
///
/// `crossfade_frames` is the number of stereo frames in the transition (length in
/// samples divided by 2). The buffers MUST have `crossfade_frames * 2` interleaved
/// samples each (or the function takes the shorter of the two).
pub fn mix_crossfade_eq(
    outgoing: &[f32],
    incoming: &[f32],
    crossfade_frames: usize,
    sample_rate: f64,
    bass_hint: Option<BassMixHint>,
) -> Vec<f32> {
    // If both tracks are bass-light, the bass-clash problem doesn't exist — pure
    // equal-power crossfade sounds cleaner than running biquads on near-empty signals.
    if let Some(h) = bass_hint {
        if h.outgoing_bass_mean < 0.3 && h.incoming_bass_mean < 0.3 {
            return mix_crossfade(outgoing, incoming, crossfade_frames);
        }
    }

    // Choose swap-window timing based on bass content.
    let (out_swap_start, out_swap_end): (f64, f64) = match bass_hint {
        Some(h) if h.outgoing_bass_mean > 0.5 && h.incoming_bass_mean > 0.5 => (0.25, 0.40),
        _ => (0.375, 0.5),
    };
    let in_swap_start = out_swap_end;
    let in_swap_end = (out_swap_end + (out_swap_end - out_swap_start)).min(1.0);

    let mut outgoing_filtered = outgoing.to_vec();
    let mut incoming_filtered = incoming.to_vec();

    // Outgoing: lowpass sweeps from 22 kHz → 200 Hz across [out_swap_start, out_swap_end].
    apply_swept_filter(
        &mut outgoing_filtered,
        FilterKind::LowPass,
        sample_rate,
        |t| sweep_log(t, out_swap_start, out_swap_end, 22_000.0, 200.0),
    );

    // Incoming: highpass sweeps from 200 Hz → 22 kHz on the mirror window.
    apply_swept_filter(
        &mut incoming_filtered,
        FilterKind::HighPass,
        sample_rate,
        |t| sweep_log(t, in_swap_start, in_swap_end, 200.0, 22_000.0),
    );

    // Apply equal-power volume curves on top of the filtered signals.
    mix_crossfade(&outgoing_filtered, &incoming_filtered, crossfade_frames)
}

/// Log-space sweep between `from_hz` at/before `start` and `to_hz` at/after `end`.
/// Plateaus on both sides of the [start, end] interval.
fn sweep_log(t: f64, start: f64, end: f64, from_hz: f64, to_hz: f64) -> f64 {
    if t < start {
        from_hz
    } else if t >= end {
        to_hz
    } else {
        let local = (t - start) / (end - start);
        let log_from = from_hz.log10();
        let log_to = to_hz.log10();
        10.0_f64.powf(log_from + (log_to - log_from) * local)
    }
}
