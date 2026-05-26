use dj_engine::crossfade::{equal_power_curve, mix_crossfade};

#[test]
fn equal_power_curves_sum_to_constant_power() {
    let n = 1024;
    for i in 0..n {
        let t = i as f64 / (n - 1) as f64;
        let (out_gain, in_gain) = equal_power_curve(t);
        // Equal-power: out^2 + in^2 should equal 1.0.
        let sum = out_gain * out_gain + in_gain * in_gain;
        assert!((sum - 1.0).abs() < 1e-9, "t={}: sum={}", t, sum);
    }
}

#[test]
fn endpoints_are_clean() {
    let (out_gain, in_gain) = equal_power_curve(0.0);
    assert!((out_gain - 1.0).abs() < 1e-9);
    assert!(in_gain.abs() < 1e-9);

    let (out_gain, in_gain) = equal_power_curve(1.0);
    assert!(out_gain.abs() < 1e-9);
    assert!((in_gain - 1.0).abs() < 1e-9);
}

#[test]
fn mix_crossfade_produces_continuous_output() {
    // Two unit-amplitude signals, length 1000 (interleaved stereo = 500 frames).
    let outgoing = vec![1.0_f32; 1000];
    let incoming = vec![0.5_f32; 1000];
    let out = mix_crossfade(&outgoing, &incoming, 500);
    assert_eq!(out.len(), 1000);
    // At the midpoint, both gains are sqrt(2)/2 ~= 0.707.
    // Output should be 1.0 * 0.707 + 0.5 * 0.707 ~= 1.06.
    let mid = out[500];
    assert!((mid - 1.06).abs() < 0.1, "mid sample was {}", mid);
}
