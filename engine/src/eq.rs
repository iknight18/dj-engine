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
