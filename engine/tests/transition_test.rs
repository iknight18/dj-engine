use dj_engine::sidecar::Sidecar;
use dj_engine::transition::{plan_transition, TransitionPlan};

fn sc(
    bpm: f64,
    key: &str,
    energy: f64,
    mixable: bool,
    downbeats: Vec<f64>,
    intro_end: f64,
    outro_start: f64,
) -> Sidecar {
    sc_v2(
        bpm,
        key,
        energy,
        mixable,
        true, // is_phrase_aligned defaults to true for old tests
        downbeats,
        intro_end,
        outro_start,
    )
}

fn sc_v2(
    bpm: f64,
    key: &str,
    energy: f64,
    beatmatchable: bool,
    phrase_aligned: bool,
    downbeats: Vec<f64>,
    intro_end: f64,
    outro_start: f64,
) -> Sidecar {
    Sidecar {
        schema_version: 2,
        analyzer_version: "0.2.0".into(),
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
        is_mixable: Some(beatmatchable),
        mixable_reasons: vec![],
        phrase_boundaries_seconds: None,
        phrase_confidence: None,
        band_energy_curves: None,
        band_energy_means: None,
        is_beatmatchable: Some(beatmatchable),
        is_phrase_aligned: Some(phrase_aligned),
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
fn plan_returns_transition_plan_struct() {
    let out = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    let _plan: TransitionPlan = plan_transition(&out, &inc);
}

#[test]
fn outgoing_start_snaps_to_nearest_phrase_boundary() {
    // Downbeats every 1s from 0 to 60. phrase_bars=16 → boundaries at indices 0, 16, 32, 48.
    // intro_end=0 → anchor at downbeat[0]. outro_start=42 → phrase boundaries [0, 16, 32].
    // Nearest to outro_start=42 is 32.
    let downbeats: Vec<f64> = (0..60).map(|i| i as f64).collect();
    let out = sc(120.0, "8A", 0.4, true, downbeats.clone(), 0.0, 42.0);
    let inc = sc(120.0, "8A", 0.4, true, downbeats, 0.0, 60.0);
    let plan = plan_transition(&out, &inc);
    assert!(
        (plan.outgoing_crossfade_start_seconds - 32.0).abs() < 1e-6,
        "expected 32.0 (nearest phrase boundary), got {}",
        plan.outgoing_crossfade_start_seconds
    );
}

#[test]
fn incoming_start_is_first_phrase_boundary() {
    let downbeats: Vec<f64> = (0..60).map(|i| i as f64).collect();
    let out = sc(120.0, "8A", 0.4, true, downbeats.clone(), 0.0, 40.0);
    // intro_end=5, phrase boundaries anchored at downbeat[5]=5 → first boundary is 5.0.
    let inc = sc(120.0, "8A", 0.4, true, downbeats, 5.0, 60.0);
    let plan = plan_transition(&out, &inc);
    assert!(
        (plan.incoming_crossfade_start_seconds - 5.0).abs() < 1e-6,
        "expected 5.0 (first phrase boundary), got {}",
        plan.incoming_crossfade_start_seconds
    );
}

#[test]
fn falls_back_to_raw_values_when_no_phrase_boundaries() {
    // Empty downbeats → phrase_boundaries returns empty → fall back to raw outro_start / intro_end.
    let out = sc(120.0, "8A", 0.4, true, vec![], 0.0, 42.0);
    let inc = sc(120.0, "8A", 0.4, true, vec![], 5.0, 60.0);
    let plan = plan_transition(&out, &inc);
    assert!((plan.outgoing_crossfade_start_seconds - 42.0).abs() < 1e-6);
    assert!((plan.incoming_crossfade_start_seconds - 5.0).abs() < 1e-6);
}

#[test]
fn non_beatmatchable_incoming_no_longer_hard_cuts() {
    let out = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc_v2(120.0, "8A", 0.4, false, false, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 16, "non-beatmatchable should give 16-bar fade, not hard cut");
    assert!(!plan.use_eq_swap, "non-beatmatchable should NOT use EQ swap");
}

#[test]
fn energy_delta_below_new_hard_cut_threshold_gets_quick_cut() {
    // delta = 0.40, between QUICK_CUT (0.20) and new HARD_CUT (0.50)
    let out = sc(120.0, "8A", 0.2, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc(120.0, "8A", 0.6, true, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 8, "delta 0.40 should be quick cut not hard cut");
}

#[test]
fn energy_delta_above_new_hard_cut_threshold_hard_cuts() {
    // delta = 0.55, above new HARD_CUT (0.50)
    let out = sc(120.0, "8A", 0.1, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc(120.0, "8A", 0.65, true, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 0);
}

#[test]
fn beatmatchable_but_not_phrase_aligned_skips_eq_swap() {
    let out = sc_v2(120.0, "8A", 0.4, true, false, dense_downbeats(), 0.0, 80.0);
    let inc = sc_v2(120.0, "8A", 0.4, true, false, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 32, "still gets long blend");
    assert!(!plan.use_eq_swap, "non-phrase-aligned should NOT EQ swap even on long blend");
}
