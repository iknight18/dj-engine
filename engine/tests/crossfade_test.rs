use dj_engine::crossfade::equal_power_curve;

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
