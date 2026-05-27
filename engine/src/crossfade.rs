//! Equal-power crossfade curves and stereo PCM crossfade mixing.

use std::f64::consts::FRAC_PI_2;

use crate::eq::{apply_swept_filter, FilterKind};

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

/// Equal-power crossfade with a 2-band EQ swap centered at the midpoint.
///
/// `crossfade_frames` is the number of stereo frames in the transition (length in
/// samples divided by 2). The buffers MUST have `crossfade_frames * 2` interleaved
/// samples each (or the function takes the shorter of the two).
pub fn mix_crossfade_eq(
    outgoing: &[f32],
    incoming: &[f32],
    crossfade_frames: usize,
    sample_rate: f64,
) -> Vec<f32> {
    let mut outgoing_filtered = outgoing.to_vec();
    let mut incoming_filtered = incoming.to_vec();

    // Outgoing: lowpass that sweeps 22 kHz → 200 Hz across t in [0.375, 0.5].
    // The cutoff plateau on either side ensures the highs pass cleanly outside the swap window.
    apply_swept_filter(
        &mut outgoing_filtered,
        FilterKind::LowPass,
        sample_rate,
        |t| {
            let lo = 200.0_f64;
            let hi = 22_000.0_f64;
            if t <= 0.375 {
                hi
            } else if t >= 0.5 {
                lo
            } else {
                // Log-space sweep from hi to lo across [0.375, 0.5].
                let local = (t - 0.375) / (0.5 - 0.375);
                let log_hi = hi.log10();
                let log_lo = lo.log10();
                10.0_f64.powf(log_hi + (log_lo - log_hi) * local)
            }
        },
    );

    // Incoming: highpass that sweeps 200 Hz → 22 kHz across t in [0.5, 0.625].
    apply_swept_filter(
        &mut incoming_filtered,
        FilterKind::HighPass,
        sample_rate,
        |t| {
            let lo = 200.0_f64;
            let hi = 22_000.0_f64;
            if t <= 0.5 {
                lo
            } else if t >= 0.625 {
                hi
            } else {
                let local = (t - 0.5) / (0.625 - 0.5);
                let log_lo = lo.log10();
                let log_hi = hi.log10();
                10.0_f64.powf(log_lo + (log_hi - log_lo) * local)
            }
        },
    );

    // Apply equal-power volume curves on top of the filtered signals.
    mix_crossfade(&outgoing_filtered, &incoming_filtered, crossfade_frames)
}
