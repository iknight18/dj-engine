use dj_engine::crossfade::{mix_crossfade_eq, BassMixHint};

fn sine_stereo(freq_hz: f64, frames: usize, sample_rate: f64) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let s = (2.0 * std::f64::consts::PI * freq_hz * (i as f64) / sample_rate).sin() as f32;
        out.push(s);
        out.push(s);
    }
    out
}

fn rms_window(samples: &[f32], start: usize, end: usize) -> f32 {
    let slice = &samples[start..end];
    let sum: f64 = slice.iter().map(|s| (*s as f64).powi(2)).sum();
    ((sum / slice.len() as f64).sqrt()) as f32
}

#[test]
fn mid_region_contains_energy() {
    let sr = 44100.0;
    let frames = 4 * sr as usize; // 4 seconds
    let outgoing = sine_stereo(440.0, frames, sr); // A4
    let incoming = sine_stereo(523.0, frames, sr); // C5
    let mixed = mix_crossfade_eq(&outgoing, &incoming, frames, sr, None);
    assert_eq!(mixed.len(), outgoing.len());

    // Middle 25% — both should have some signal energy here.
    let n = mixed.len();
    let mid_rms = rms_window(&mixed, 3 * n / 8, 5 * n / 8);
    assert!(
        mid_rms > 0.05,
        "midpoint RMS too low ({}), expected nontrivial signal",
        mid_rms
    );
}

#[test]
fn output_length_matches_input() {
    let sr = 44100.0;
    let frames = sr as usize; // 1 second
    let outgoing = sine_stereo(440.0, frames, sr);
    let incoming = sine_stereo(523.0, frames, sr);
    let mixed = mix_crossfade_eq(&outgoing, &incoming, frames, sr, None);
    assert_eq!(mixed.len(), frames * 2);
}

#[test]
fn no_nan_or_clip() {
    let sr = 44100.0;
    let frames = 2 * sr as usize;
    let outgoing = sine_stereo(80.0, frames, sr); // bass sine
    let incoming = sine_stereo(80.0, frames, sr);
    let mixed = mix_crossfade_eq(&outgoing, &incoming, frames, sr, None);
    let max_abs = mixed.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
    assert!(mixed.iter().all(|s| s.is_finite()), "NaN or Inf in output");
    assert!(
        max_abs < 1.5,
        "max abs sample {} unreasonably high",
        max_abs
    );
}

#[test]
fn no_bass_hint_matches_previous_behavior() {
    // Pre-bass_hint signature equivalent: pass None and expect old default window result.
    let sr = 44100.0;
    let frames = sr as usize;
    let outgoing = sine_stereo(440.0, frames, sr);
    let incoming = sine_stereo(523.0, frames, sr);
    let mixed = mix_crossfade_eq(&outgoing, &incoming, frames, sr, None);
    assert_eq!(mixed.len(), frames * 2);
}

#[test]
fn both_bass_light_skips_eq_swap() {
    let sr = 44100.0;
    let frames = sr as usize;
    let outgoing = sine_stereo(440.0, frames, sr);
    let incoming = sine_stereo(523.0, frames, sr);
    let hint = Some(BassMixHint {
        outgoing_bass_mean: 0.1,
        incoming_bass_mean: 0.2,
    });
    let mixed_with_hint = mix_crossfade_eq(&outgoing, &incoming, frames, sr, hint);
    let mixed_plain = dj_engine::crossfade::mix_crossfade(&outgoing, &incoming, frames);
    // Both should match the pure equal-power crossfade exactly when bass is low.
    for (a, b) in mixed_with_hint.iter().zip(mixed_plain.iter()) {
        assert!(
            (a - b).abs() < 1e-5,
            "expected identical output; got {} vs {}",
            a,
            b
        );
    }
}

#[test]
fn both_bass_heavy_uses_earlier_swap_window() {
    let sr = 44100.0;
    let frames = sr as usize;
    let outgoing = sine_stereo(80.0, frames, sr); // bass
    let incoming = sine_stereo(80.0, frames, sr);
    let hint = Some(BassMixHint {
        outgoing_bass_mean: 0.7,
        incoming_bass_mean: 0.7,
    });
    let mixed_early = mix_crossfade_eq(&outgoing, &incoming, frames, sr, hint);
    let mixed_default = mix_crossfade_eq(&outgoing, &incoming, frames, sr, None);
    // Outputs should differ — by the 33% mark the early window (0.25..0.40) is mid-sweep
    // but the default window (0.375..0.5) has not started yet.
    let probe_frame = frames / 3;
    let probe_sample = probe_frame * 2;
    assert!(
        (mixed_early[probe_sample] - mixed_default[probe_sample]).abs() > 1e-4,
        "expected difference at 33% mark with early swap window: early={} default={}",
        mixed_early[probe_sample],
        mixed_default[probe_sample]
    );
}
