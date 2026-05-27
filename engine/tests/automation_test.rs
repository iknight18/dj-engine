use dj_engine::automation::Curve;

const EPS: f64 = 1e-9;

#[test]
fn curve_linear_endpoints_and_midpoint() {
    assert!((Curve::Linear.eval(0.0) - 0.0).abs() < EPS);
    assert!((Curve::Linear.eval(0.5) - 0.5).abs() < EPS);
    assert!((Curve::Linear.eval(1.0) - 1.0).abs() < EPS);
}

#[test]
fn curve_linear_clamps_out_of_range() {
    assert!((Curve::Linear.eval(-0.5) - 0.0).abs() < EPS);
    assert!((Curve::Linear.eval(1.5) - 1.0).abs() < EPS);
}

#[test]
fn curve_equal_power_in() {
    assert!((Curve::EqualPowerIn.eval(0.0) - 0.0).abs() < EPS);
    assert!((Curve::EqualPowerIn.eval(1.0) - 1.0).abs() < EPS);
    // sin(π/4) = sqrt(2)/2
    let mid = Curve::EqualPowerIn.eval(0.5);
    assert!((mid - 0.7071067811865476).abs() < 1e-12);
}

#[test]
fn curve_equal_power_out() {
    assert!((Curve::EqualPowerOut.eval(0.0) - 1.0).abs() < EPS);
    assert!((Curve::EqualPowerOut.eval(1.0) - 0.0).abs() < EPS);
    let mid = Curve::EqualPowerOut.eval(0.5);
    assert!((mid - 0.7071067811865476).abs() < 1e-12);
}

#[test]
fn curve_step_jumps_at_one() {
    assert!((Curve::Step.eval(0.0) - 0.0).abs() < EPS);
    assert!((Curve::Step.eval(0.5) - 0.0).abs() < EPS);
    assert!((Curve::Step.eval(0.999) - 0.0).abs() < EPS);
    assert!((Curve::Step.eval(1.0) - 1.0).abs() < EPS);
}

#[test]
fn curve_equal_power_sum_is_constant_power() {
    // sin^2 + cos^2 == 1
    for i in 0..=10 {
        let t = i as f64 / 10.0;
        let a = Curve::EqualPowerIn.eval(t);
        let b = Curve::EqualPowerOut.eval(t);
        assert!((a * a + b * b - 1.0).abs() < 1e-12, "t={t}");
    }
}
