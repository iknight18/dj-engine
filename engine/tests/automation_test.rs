use std::f64::consts::FRAC_1_SQRT_2;

use dj_engine::automation::{Automation, AutomationQueue, AutomationTarget, Curve};
use dj_engine::deck::DeckState;

const EPS: f64 = 1e-9;

fn default_state() -> DeckState {
    DeckState::default()
}

fn ramp(
    target: AutomationTarget,
    from: f64,
    to: f64,
    curve: Curve,
    t0: u64,
    t1: u64,
) -> Automation {
    Automation {
        target,
        t_start_frames: t0,
        t_end_frames: t1,
        from,
        to,
        curve,
    }
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
    assert!((mid - FRAC_1_SQRT_2).abs() < 1e-12);
}

#[test]
fn curve_equal_power_out() {
    // EqualPowerOut is a 0->1 progression with shape "1 - cos(t*π/2)".
    // When sample_value uses from=1.0 to=0.0, the resulting GAIN is cos(t·π/2).
    assert!((Curve::EqualPowerOut.eval(0.0) - 0.0).abs() < EPS);
    assert!((Curve::EqualPowerOut.eval(1.0) - 1.0).abs() < EPS);
    let mid = Curve::EqualPowerOut.eval(0.5);
    // 1 - cos(π/4) = 1 - sqrt(2)/2
    assert!((mid - (1.0 - FRAC_1_SQRT_2)).abs() < 1e-12);
}

#[test]
fn curve_step_jumps_at_one() {
    assert!((Curve::Step.eval(0.0) - 0.0).abs() < EPS);
    assert!((Curve::Step.eval(0.5) - 0.0).abs() < EPS);
    assert!((Curve::Step.eval(0.999) - 0.0).abs() < EPS);
    assert!((Curve::Step.eval(1.0) - 1.0).abs() < EPS);
}

#[test]
fn equal_power_pair_sums_to_constant_power_via_gains() {
    // Equal-power requires gain_in² + gain_out² = 1 across the crossfade. The
    // user-visible contract is the GAIN (what `sample_value` returns), not the
    // raw curve eval. Pair EqualPowerIn (0->1) with EqualPowerOut (1->0).
    let auto_in = Automation {
        target: AutomationTarget::Fader,
        t_start_frames: 0,
        t_end_frames: 100,
        from: 0.0,
        to: 1.0,
        curve: Curve::EqualPowerIn,
    };
    let auto_out = Automation {
        target: AutomationTarget::Fader,
        t_start_frames: 0,
        t_end_frames: 100,
        from: 1.0,
        to: 0.0,
        curve: Curve::EqualPowerOut,
    };
    for cursor in 0..100 {
        let g_in = auto_in.sample_value(cursor).unwrap();
        let g_out = auto_out.sample_value(cursor).unwrap();
        assert!(
            (g_in * g_in + g_out * g_out - 1.0).abs() < 1e-12,
            "cursor={cursor} g_in={g_in} g_out={g_out}"
        );
    }
}

#[test]
fn equal_power_out_gain_endpoints() {
    let auto_out = Automation {
        target: AutomationTarget::Fader,
        t_start_frames: 0,
        t_end_frames: 100,
        from: 1.0,
        to: 0.0,
        curve: Curve::EqualPowerOut,
    };
    let g0 = auto_out.sample_value(0).unwrap();
    assert!(
        (g0 - 1.0).abs() < EPS,
        "fade-out gain at t=0 should be 1.0, got {g0}"
    );
    let g_mid = auto_out.sample_value(50).unwrap();
    assert!(
        (g_mid - FRAC_1_SQRT_2).abs() < 1e-12,
        "fade-out gain at midpoint should be sqrt(2)/2"
    );
    // sample_value returns None at t=t_end; AutomationQueue::step applies `to`.
    assert!(auto_out.sample_value(100).is_none());
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
    let a = ramp(
        AutomationTarget::Fader,
        0.0,
        1.0,
        Curve::EqualPowerIn,
        0,
        100,
    );
    let v = a.sample_value(50).unwrap();
    assert!((v - FRAC_1_SQRT_2).abs() < 1e-12);
}

#[test]
fn empty_queue_no_change() {
    let mut q = AutomationQueue::new();
    let mut s = default_state();
    q.step(0, &mut s);
    assert_eq!(s.fader, 0.0);
    assert_eq!(s.tempo, 1.0);
}

#[test]
fn fader_linear_ramp_applies() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(
        AutomationTarget::Fader,
        0.0,
        1.0,
        Curve::Linear,
        0,
        100,
    ));
    let mut s = default_state();
    q.step(50, &mut s);
    assert!((s.fader - 0.5).abs() < 1e-6);
}

#[test]
fn fader_finished_leaves_to_installed_and_is_removed() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(
        AutomationTarget::Fader,
        0.0,
        1.0,
        Curve::Linear,
        0,
        100,
    ));
    let mut s = default_state();
    q.step(150, &mut s);
    assert!((s.fader - 1.0).abs() < 1e-6);
    assert!(q.is_empty());
}

#[test]
fn future_automation_not_yet_applied() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(
        AutomationTarget::Fader,
        0.0,
        1.0,
        Curve::Linear,
        100,
        200,
    ));
    let mut s = default_state();
    q.step(50, &mut s);
    assert_eq!(s.fader, 0.0);
    assert_eq!(q.len(), 1);
}

#[test]
fn tempo_step_installs_to_value() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::Tempo, 1.0, 1.07, Curve::Step, 0, 1));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.tempo - 1.07).abs() < 1e-9);
    assert!(q.is_empty());
}

#[test]
fn eq_low_step_installs_to_value() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::EqLow, 0.0, -40.0, Curve::Step, 0, 1));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.eq_low_db - (-40.0)).abs() < 1e-9);
    assert!(q.is_empty());
}

#[test]
fn eq_mid_clamped_below_minus_40() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(
        AutomationTarget::EqMid,
        0.0,
        -100.0,
        Curve::Step,
        0,
        1,
    ));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.eq_mid_db - (-40.0)).abs() < 1e-9);
}

#[test]
fn eq_high_clamped_above_plus_6() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::EqHigh, 0.0, 20.0, Curve::Step, 0, 1));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.eq_high_db - 6.0).abs() < 1e-9);
}

#[test]
fn eq_low_linear_midpoint() {
    let a = ramp(AutomationTarget::EqLow, 0.0, -40.0, Curve::Linear, 0, 100);
    let v = a.sample_value(50).unwrap();
    assert!((v - (-20.0)).abs() < 1e-9);
}

#[test]
fn filter_step_installs_to_value() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::Filter, 0.0, 1.0, Curve::Step, 0, 1));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.filter_knob - 1.0).abs() < 1e-9);
}

#[test]
fn filter_clamped_above_plus_1() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::Filter, 0.0, 5.0, Curve::Step, 0, 1));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.filter_knob - 1.0).abs() < 1e-9);
}

#[test]
fn filter_clamped_below_minus_1() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::Filter, 0.0, -5.0, Curve::Step, 0, 1));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.filter_knob - (-1.0)).abs() < 1e-9);
}

#[test]
fn filter_linear_midpoint_negative() {
    let a = ramp(AutomationTarget::Filter, 1.0, -1.0, Curve::Linear, 0, 100);
    let v = a.sample_value(50).unwrap();
    assert!((v - 0.0).abs() < 1e-9, "got {v}");
}

#[test]
fn echo_mix_clamped_to_zero_one() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::EchoMix, 0.0, 2.0, Curve::Step, 0, 1));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.echo_mix - 1.0).abs() < 1e-9);

    let mut q2 = AutomationQueue::new();
    q2.schedule(ramp(
        AutomationTarget::EchoMix,
        0.0,
        -0.5,
        Curve::Step,
        0,
        1,
    ));
    let mut s2 = default_state();
    q2.step(1, &mut s2);
    assert!(s2.echo_mix.abs() < 1e-9);
}

#[test]
fn loop_gate_clamped_and_installed() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::LoopGate, 0.0, 1.0, Curve::Step, 0, 1));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.loop_gate - 1.0).abs() < 1e-9);
}
