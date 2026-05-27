//! Equal-power crossfade curve helper. Used by `Curve::EqualPowerIn/Out` in
//! the automation layer. The mix-two-buffers helpers were removed when the
//! two-deck tick-driven mixer landed; per-tick fader application now lives in
//! `Deck::pull`.

use std::f64::consts::FRAC_PI_2;

/// Return (outgoing_gain, incoming_gain) for `t` in [0, 1].
pub fn equal_power_curve(t: f64) -> (f64, f64) {
    let t = t.clamp(0.0, 1.0);
    let in_gain = (t * FRAC_PI_2).sin();
    let out_gain = (t * FRAC_PI_2).cos();
    (out_gain, in_gain)
}
