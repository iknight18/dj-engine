//! Biquad filters + swept filter helper. Pure DSP, no knowledge of tracks.

/// Direct Form I biquad. Coefficients use the Robert Bristow-Johnson cookbook.
/// Q is fixed at 1/sqrt(2) (Butterworth — flat passband, no resonance).
#[derive(Debug, Clone, Copy)]
pub struct Biquad {
    // Normalized coefficients (a0 already divided out).
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    // State.
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

const Q_BUTTERWORTH: f64 = 0.707_106_781_186_547_5; // 1 / sqrt(2)

impl Biquad {
    pub fn lowpass(sample_rate: f64, cutoff_hz: f64) -> Self {
        let (b0, b1, b2, a1, a2) = lowpass_coeffs(sample_rate, cutoff_hz);
        Self {
            b0,
            b1,
            b2,
            a1,
            a2,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Apply the filter to one sample. Stateful — call in order.
    pub fn process(&mut self, x: f32) -> f32 {
        let x = x as f64;
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y as f32
    }
}

fn lowpass_coeffs(sample_rate: f64, cutoff_hz: f64) -> (f64, f64, f64, f64, f64) {
    let cutoff = cutoff_hz.clamp(1.0, sample_rate * 0.49);
    let w0 = 2.0 * std::f64::consts::PI * cutoff / sample_rate;
    let cos_w0 = w0.cos();
    let alpha = w0.sin() / (2.0 * Q_BUTTERWORTH);
    let a0 = 1.0 + alpha;

    let b0 = ((1.0 - cos_w0) / 2.0) / a0;
    let b1 = (1.0 - cos_w0) / a0;
    let b2 = ((1.0 - cos_w0) / 2.0) / a0;
    let a1 = (-2.0 * cos_w0) / a0;
    let a2 = (1.0 - alpha) / a0;
    (b0, b1, b2, a1, a2)
}

impl Biquad {
    pub fn highpass(sample_rate: f64, cutoff_hz: f64) -> Self {
        let (b0, b1, b2, a1, a2) = highpass_coeffs(sample_rate, cutoff_hz);
        Self {
            b0,
            b1,
            b2,
            a1,
            a2,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Update coefficients without resetting state. Used for sweeps.
    pub fn set_cutoff(&mut self, sample_rate: f64, kind: FilterKind, cutoff_hz: f64) {
        let (b0, b1, b2, a1, a2) = match kind {
            FilterKind::LowPass => lowpass_coeffs(sample_rate, cutoff_hz),
            FilterKind::HighPass => highpass_coeffs(sample_rate, cutoff_hz),
        };
        self.b0 = b0;
        self.b1 = b1;
        self.b2 = b2;
        self.a1 = a1;
        self.a2 = a2;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    LowPass,
    HighPass,
}

/// Process a stereo interleaved buffer IN PLACE with a time-varying cutoff.
/// `cutoff_curve(t)` returns the cutoff in Hz for normalized time `t in [0, 1]`.
/// Filters are evaluated independently per channel; state persists across the buffer.
#[allow(dead_code)] // Re-introduced by sub-project 3 (Filter Sweep recipe); preserved meanwhile.
pub fn apply_swept_filter(
    samples: &mut [f32],
    kind: FilterKind,
    sample_rate: f64,
    cutoff_curve: impl Fn(f64) -> f64,
) {
    if samples.len() < 2 {
        return;
    }
    let frames = samples.len() / 2;
    let initial = cutoff_curve(0.0);
    let mut filt_l = match kind {
        FilterKind::LowPass => Biquad::lowpass(sample_rate, initial),
        FilterKind::HighPass => Biquad::highpass(sample_rate, initial),
    };
    let mut filt_r = match kind {
        FilterKind::LowPass => Biquad::lowpass(sample_rate, initial),
        FilterKind::HighPass => Biquad::highpass(sample_rate, initial),
    };

    // Update cutoff every 256 frames — fine enough for a smooth audible sweep,
    // coarse enough to avoid hammering the coefficient recompute.
    const RECOMPUTE_EVERY: usize = 256;
    for frame in 0..frames {
        if frame > 0 && frame % RECOMPUTE_EVERY == 0 {
            let t = frame as f64 / frames.max(1) as f64;
            let cutoff = cutoff_curve(t);
            filt_l.set_cutoff(sample_rate, kind, cutoff);
            filt_r.set_cutoff(sample_rate, kind, cutoff);
        }
        let l = samples[frame * 2];
        let r = samples[frame * 2 + 1];
        samples[frame * 2] = filt_l.process(l);
        samples[frame * 2 + 1] = filt_r.process(r);
    }
}

fn highpass_coeffs(sample_rate: f64, cutoff_hz: f64) -> (f64, f64, f64, f64, f64) {
    let cutoff = cutoff_hz.clamp(1.0, sample_rate * 0.49);
    let w0 = 2.0 * std::f64::consts::PI * cutoff / sample_rate;
    let cos_w0 = w0.cos();
    let alpha = w0.sin() / (2.0 * Q_BUTTERWORTH);
    let a0 = 1.0 + alpha;

    let b0 = ((1.0 + cos_w0) / 2.0) / a0;
    let b1 = (-(1.0 + cos_w0)) / a0;
    let b2 = ((1.0 + cos_w0) / 2.0) / a0;
    let a1 = (-2.0 * cos_w0) / a0;
    let a2 = (1.0 - alpha) / a0;
    (b0, b1, b2, a1, a2)
}
