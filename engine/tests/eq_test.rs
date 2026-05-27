use dj_engine::eq::Biquad;

fn rms(samples: &[f32]) -> f32 {
    let sum: f64 = samples.iter().map(|s| (*s as f64).powi(2)).sum();
    ((sum / samples.len() as f64).sqrt()) as f32
}

fn sine(freq_hz: f64, sample_rate: f64, duration_s: f64) -> Vec<f32> {
    let n = (duration_s * sample_rate) as usize;
    (0..n)
        .map(|i| (2.0 * std::f64::consts::PI * freq_hz * (i as f64) / sample_rate).sin() as f32)
        .collect()
}

#[test]
fn lowpass_passes_low_frequencies() {
    let sr = 44100.0;
    let signal = sine(100.0, sr, 1.0);
    let mut filter = Biquad::lowpass(sr, 500.0);
    let filtered: Vec<f32> = signal.iter().map(|s| filter.process(*s)).collect();

    // Allow a brief warm-up where the filter state is converging. Skip the first 200 samples.
    let in_rms = rms(&signal[200..]);
    let out_rms = rms(&filtered[200..]);
    let ratio_db = 20.0 * (out_rms / in_rms).log10();
    assert!(
        ratio_db.abs() < 1.0,
        "100 Hz should pass through a 500 Hz lowpass with <1 dB loss; got {} dB",
        ratio_db
    );
}

#[test]
fn lowpass_kills_high_frequencies() {
    let sr = 44100.0;
    let signal = sine(10_000.0, sr, 1.0);
    let mut filter = Biquad::lowpass(sr, 200.0);
    let filtered: Vec<f32> = signal.iter().map(|s| filter.process(*s)).collect();

    let in_rms = rms(&signal[200..]);
    let out_rms = rms(&filtered[200..]);
    assert!(
        out_rms < 0.05 * in_rms,
        "10 kHz signal should be <5% of input after 200 Hz lowpass; got out/in = {}",
        out_rms / in_rms
    );
}
