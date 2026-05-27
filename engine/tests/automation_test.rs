use dj_engine::automation::{Automation, AutomationTarget, Curve};

const EPS: f64 = 1e-9;

fn ramp(target: AutomationTarget, from: f64, to: f64, curve: Curve, t0: u64, t1: u64) -> Automation {
    Automation { target, t_start_frames: t0, t_end_frames: t1, from, to, curve }
}

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

#[test]
fn sample_value_before_start_is_none() {
    let a = ramp(AutomationTarget::Fader, 0.0, 1.0, Curve::Linear, 100, 200);
    assert_eq!(a.sample_value(50), None);
    assert_eq!(a.sample_value(99), None);
}

#[test]
fn sample_value_at_start_is_from() {
    let a = ramp(AutomationTarget::Fader, 0.2, 0.8, Curve::Linear, 100, 200);
    let v = a.sample_value(100).unwrap();
    assert!((v - 0.2).abs() < 1e-9, "got {v}");
}

#[test]
fn sample_value_at_midpoint_linear() {
    let a = ramp(AutomationTarget::Fader, 0.0, 1.0, Curve::Linear, 0, 100);
    let v = a.sample_value(50).unwrap();
    assert!((v - 0.5).abs() < 1e-9, "got {v}");
}

#[test]
fn sample_value_at_or_after_end_is_none() {
    let a = ramp(AutomationTarget::Fader, 0.0, 1.0, Curve::Linear, 0, 100);
    assert_eq!(a.sample_value(100), None);
    assert_eq!(a.sample_value(150), None);
}

#[test]
fn sample_value_equal_power_in_midpoint() {
    let a = ramp(AutomationTarget::Fader, 0.0, 1.0, Curve::EqualPowerIn, 0, 100);
    let v = a.sample_value(50).unwrap();
    assert!((v - 0.7071067811865476).abs() < 1e-12);
}
