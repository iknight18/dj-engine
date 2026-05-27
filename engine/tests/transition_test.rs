use dj_engine::sidecar::Sidecar;
use dj_engine::transition::{plan_transition, TransitionPlan};

fn sc(bpm: f64, key: &str, energy: f64, mixable: bool, downbeats: Vec<f64>, intro_end: f64, outro_start: f64) -> Sidecar {
    Sidecar {
        schema_version: 1,
        analyzer_version: "0.1.0".into(),
        source_path: "x".into(),
        source_mtime: 0.0,
        source_size_bytes: 0,
        error: None,
        duration_seconds: Some(200.0),
        sample_rate: Some(44100),
        bpm: Some(bpm),
        bpm_confidence: Some(0.9),
        beats_seconds: vec![],
        downbeats_seconds: downbeats,
        key: Some(key.into()),
        key_label: Some("".into()),
        key_confidence: Some(0.8),
        energy_curve: vec![],
        energy_mean: Some(energy),
        cue_in_seconds: Some(0.0),
        cue_out_seconds: Some(200.0),
        intro_end_seconds: Some(intro_end),
        outro_start_seconds: Some(outro_start),
        is_mixable: Some(mixable),
        mixable_reasons: vec![],
    }
}

fn dense_downbeats() -> Vec<f64> {
    (0..100).map(|i| i as f64).collect()
}

#[test]
fn identical_tracks_get_long_blend() {
    let out = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 32);
    assert!(plan.use_eq_swap);
}

#[test]
fn big_energy_jump_triggers_hard_cut() {
    let out = sc(120.0, "8A", 0.1, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc(120.0, "8A", 0.8, true, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 0);
    assert!(!plan.use_eq_swap);
}

#[test]
fn non_mixable_incoming_triggers_hard_cut() {
    let out = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc(120.0, "8A", 0.4, false, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 0);
    assert!(!plan.use_eq_swap);
}

#[test]
fn moderate_energy_jump_triggers_quick_cut() {
    let out = sc(120.0, "8A", 0.3, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc(120.0, "8A", 0.55, true, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 8);
    assert!(plan.use_eq_swap);
}

#[test]
fn default_pair_gets_standard_blend() {
    let out = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    // Adjacent key, slightly different BPM, slight energy delta — none extreme.
    let inc = sc(123.0, "7A", 0.45, true, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 16);
    assert!(plan.use_eq_swap);
}

#[test]
fn relative_key_triggers_quick_cut() {
    let out = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    // 8A minor vs 8B major are relative keys.
    let inc = sc(120.0, "8B", 0.4, true, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 8);
    assert!(plan.use_eq_swap);
}

#[test]
fn plan_returns_TransitionPlan_struct() {
    let out = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    let _plan: TransitionPlan = plan_transition(&out, &inc);
}
