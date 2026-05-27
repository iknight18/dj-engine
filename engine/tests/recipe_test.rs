use dj_engine::automation::{AutomationTarget, Curve};
use dj_engine::library::LoadedTrack;
use dj_engine::recipe::{
    pick_recipe, DeckId, EqualPowerCrossfade, HardCut, Recipe, RecipeChoice, TransitionContext,
};
use dj_engine::sidecar::Sidecar;
use dj_engine::transition::TransitionPlan;
use std::path::PathBuf;

const SR: u32 = 44100;

fn sidecar_with(bpm: f64, cue_out: f64) -> Sidecar {
    Sidecar {
        schema_version: 1,
        analyzer_version: "test".into(),
        source_path: "test.mp3".into(),
        source_mtime: 0.0,
        source_size_bytes: 0,
        duration_seconds: Some(300.0),
        sample_rate: Some(SR),
        bpm: Some(bpm),
        bpm_confidence: Some(1.0),
        beats_seconds: vec![],
        downbeats_seconds: vec![],
        key: None,
        key_label: None,
        key_confidence: None,
        energy_curve: vec![],
        energy_mean: Some(0.5),
        band_energy_means: None,
        cue_in_seconds: Some(0.0),
        cue_out_seconds: Some(cue_out),
        intro_end_seconds: Some(16.0),
        outro_start_seconds: Some(200.0),
        is_mixable: Some(true),
        mixable_reasons: vec![],
        error: None,
        phrase_boundaries_seconds: None,
        phrase_confidence: None,
        band_energy_curves: None,
        is_beatmatchable: None,
        is_phrase_aligned: None,
    }
}

fn track(bpm: f64) -> LoadedTrack {
    LoadedTrack {
        sidecar_path: PathBuf::new(),
        sidecar: sidecar_with(bpm, 280.0),
    }
}

#[test]
fn equal_power_same_bpm_emits_one_out_two_in_no_ramp() {
    let outgoing = track(120.0);
    let incoming = track(120.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
    };
    let ctx = TransitionContext {
        outgoing: &outgoing,
        incoming: &incoming,
        plan: &plan,
        master_clock_frames: 1_000_000,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let r = EqualPowerCrossfade.build(&ctx);
    assert_eq!(r.outgoing_automations.len(), 1);
    assert_eq!(r.outgoing_automations[0].target, AutomationTarget::Fader);
    assert_eq!(r.outgoing_automations[0].curve, Curve::EqualPowerOut);
    assert_eq!(r.outgoing_automations[0].from, 1.0);
    assert_eq!(r.outgoing_automations[0].to, 0.0);

    // Same BPM → no pitch-journey ramp; only Fader-in + Tempo-Step (no-op).
    assert_eq!(r.incoming_automations.len(), 2);
    assert!(r
        .incoming_automations
        .iter()
        .any(|a| a.target == AutomationTarget::Fader && a.curve == Curve::EqualPowerIn));
    assert!(r
        .incoming_automations
        .iter()
        .any(|a| a.target == AutomationTarget::Tempo && a.curve == Curve::Step));
}

#[test]
fn equal_power_diff_bpm_no_ramp_only_step() {
    // Pitch-journey ramp was removed in favor of leaving incoming at the cued
    // (BPM-matched) tempo for the rest of its playback. Confirm no Tempo Linear
    // automation is emitted, only the Tempo Step at t0 confirming the cue tempo.
    let outgoing = track(120.0);
    let incoming = track(124.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
    };
    let ctx = TransitionContext {
        outgoing: &outgoing,
        incoming: &incoming,
        plan: &plan,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let r = EqualPowerCrossfade.build(&ctx);
    assert_eq!(r.outgoing_automations.len(), 1);
    assert_eq!(r.incoming_automations.len(), 2);
    assert!(!r
        .incoming_automations
        .iter()
        .any(|a| a.target == AutomationTarget::Tempo && a.curve == Curve::Linear));
    let expected_ratio = 120.0_f64 / 124.0_f64;
    let tempo_step = r
        .incoming_automations
        .iter()
        .find(|a| a.target == AutomationTarget::Tempo)
        .expect("expected Tempo Step");
    assert_eq!(tempo_step.curve, Curve::Step);
    assert!((tempo_step.from - expected_ratio).abs() < 1e-9);
    assert!((tempo_step.to - expected_ratio).abs() < 1e-9);
}

#[test]
fn equal_power_crossfade_length_matches_plan() {
    let outgoing = track(120.0);
    let incoming = track(120.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
    };
    let ctx = TransitionContext {
        outgoing: &outgoing,
        incoming: &incoming,
        plan: &plan,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let r = EqualPowerCrossfade.build(&ctx);
    let bar_seconds = (60.0_f64 / 120.0) * 4.0;
    let expected_frames = (plan.length_bars as f64 * bar_seconds * SR as f64) as u64;
    let out_fader = &r.outgoing_automations[0];
    assert_eq!(
        out_fader.t_end_frames - out_fader.t_start_frames,
        expected_frames
    );
}

#[test]
fn hard_cut_emits_step_automations() {
    let outgoing = track(120.0);
    let incoming = track(120.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 0,
        use_eq_swap: false,
    };
    let ctx = TransitionContext {
        outgoing: &outgoing,
        incoming: &incoming,
        plan: &plan,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let r = HardCut.build(&ctx);
    assert_eq!(r.outgoing_automations.len(), 1);
    assert_eq!(r.outgoing_automations[0].curve, Curve::Step);
    assert!(r
        .incoming_automations
        .iter()
        .any(|a| a.target == AutomationTarget::Fader && a.curve == Curve::Step));
}

#[test]
fn pick_recipe_chooses_hard_cut_for_zero_bars() {
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.0,
        incoming_crossfade_start_seconds: 0.0,
        length_bars: 0,
        use_eq_swap: false,
    };
    assert_eq!(pick_recipe(&plan), RecipeChoice::HardCut);
}

#[test]
fn pick_recipe_chooses_equal_power_for_nonzero_bars() {
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.0,
        incoming_crossfade_start_seconds: 0.0,
        length_bars: 16,
        use_eq_swap: false,
    };
    assert_eq!(pick_recipe(&plan), RecipeChoice::EqualPower);

    let plan_eq = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.0,
        incoming_crossfade_start_seconds: 0.0,
        length_bars: 16,
        use_eq_swap: true, // sub-project 1 still picks EqualPower; EQ swap added later
    };
    assert_eq!(pick_recipe(&plan_eq), RecipeChoice::EqualPower);
}
