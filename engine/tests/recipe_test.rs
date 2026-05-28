use dj_engine::automation::{AutomationTarget, Curve};
use dj_engine::library::LoadedTrack;
use dj_engine::recipe::{
    pick_recipe, DeckId, EqualPowerCrossfade, HardCut, Recipe, RecipeChoice, TransitionContext,
};
use dj_engine::sidecar::Sidecar;
use dj_engine::transition::TransitionPlan;
use std::path::PathBuf;

fn track_with_bpm(bpm: f64) -> LoadedTrack {
    LoadedTrack {
        sidecar_path: PathBuf::new(),
        sidecar: sidecar_with(bpm, 280.0),
    }
}

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
        stems: None,
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
        skip_tempo_align: false,
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
fn equal_power_diff_bpm_has_journey_ramp_only_when_in_band() {
    // 120→124 BPM: ratio ≈ 0.9677, delta ≈ 0.0323 — within the [0.005, 0.05)
    // journey band. build_and_compile should inject exactly one Tempo Linear
    // automation (the pitch-journey-back ramp) in addition to the Tempo Step.
    use dj_engine::recipe::build_and_compile;

    let outgoing = track(120.0);
    let incoming = track(124.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
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
    let r = build_and_compile(&EqualPowerCrossfade, &ctx);
    assert_eq!(r.outgoing_automations.len(), 1);
    // Incoming: Fader-in + Tempo Step + Tempo Linear (journey ramp) = 3 automations.
    assert_eq!(r.incoming_automations.len(), 3);
    let expected_ratio = 120.0_f64 / 124.0_f64;
    let tempo_step = r
        .incoming_automations
        .iter()
        .find(|a| a.target == AutomationTarget::Tempo && a.curve == Curve::Step)
        .expect("expected Tempo Step");
    assert!((tempo_step.from - expected_ratio).abs() < 1e-9);
    assert!((tempo_step.to - expected_ratio).abs() < 1e-9);
    let journey_ramp = r
        .incoming_automations
        .iter()
        .find(|a| a.target == AutomationTarget::Tempo && a.curve == Curve::Linear)
        .expect("expected Tempo Linear journey ramp");
    assert!((journey_ramp.from - expected_ratio).abs() < 1e-9);
    assert!((journey_ramp.to - 1.0).abs() < 1e-9);
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
        skip_tempo_align: false,
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
        skip_tempo_align: false,
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
        skip_tempo_align: false,
    };
    assert_eq!(
        dj_engine::recipe::pick_recipe_plan_only(&plan),
        RecipeChoice::HardCut
    );
}

#[test]
fn pick_recipe_chooses_equal_power_for_nonzero_bars() {
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.0,
        incoming_crossfade_start_seconds: 0.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
    };
    assert_eq!(
        dj_engine::recipe::pick_recipe_plan_only(&plan),
        RecipeChoice::EqualPower
    );
}

// ---------------------------------------------------------------------------
// BassSwap recipe tests (sub-project 2, task 5)
// ---------------------------------------------------------------------------

use dj_engine::recipe::BassSwap;
use dj_engine::sidecar::BandValues;

fn track_with_bass(bpm: f64, bass: f64) -> LoadedTrack {
    let mut sc = sidecar_with(bpm, 280.0);
    sc.band_energy_means = Some(BandValues {
        bass,
        mid: 0.5,
        high: 0.5,
    });
    LoadedTrack {
        sidecar_path: PathBuf::new(),
        sidecar: sc,
    }
}

fn standard_plan() -> TransitionPlan {
    TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: true,
        skip_tempo_align: false,
    }
}

fn ctx_for<'a>(
    outgoing: &'a LoadedTrack,
    incoming: &'a LoadedTrack,
    plan: &'a TransitionPlan,
) -> TransitionContext<'a> {
    TransitionContext {
        outgoing,
        incoming,
        plan,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    }
}

#[test]
fn bass_swap_default_emits_fader_plus_eq_low_automations() {
    let out_track = track_with_bass(120.0, 0.4);
    let in_track = track_with_bass(120.0, 0.4);
    let plan = standard_plan();
    let ctx = ctx_for(&out_track, &in_track, &plan);
    let r = BassSwap.build(&ctx);
    assert_eq!(r.outgoing_automations.len(), 2);
    assert!(r
        .outgoing_automations
        .iter()
        .any(|a| a.target == AutomationTarget::Fader && a.curve == Curve::EqualPowerOut));
    assert!(r
        .outgoing_automations
        .iter()
        .any(|a| a.target == AutomationTarget::EqLow && a.curve == Curve::Linear && a.to == -40.0));

    assert_eq!(r.incoming_automations.len(), 4);
    let in_step = r
        .incoming_automations
        .iter()
        .find(|a| a.target == AutomationTarget::EqLow && a.curve == Curve::Step)
        .expect("incoming init kill");
    assert_eq!(in_step.from, -40.0);
    assert_eq!(in_step.to, -40.0);
    let in_release = r
        .incoming_automations
        .iter()
        .find(|a| a.target == AutomationTarget::EqLow && a.curve == Curve::Linear)
        .expect("incoming release");
    assert_eq!(in_release.from, -40.0);
    assert_eq!(in_release.to, 0.0);
}

#[test]
fn bass_swap_bass_light_pair_skips_eq() {
    let out_track = track_with_bass(120.0, 0.2);
    let in_track = track_with_bass(120.0, 0.25);
    let plan = standard_plan();
    let ctx = ctx_for(&out_track, &in_track, &plan);
    let r = BassSwap.build(&ctx);
    // No EqLow automations on either deck.
    assert!(!r
        .outgoing_automations
        .iter()
        .any(|a| a.target == AutomationTarget::EqLow));
    assert!(!r
        .incoming_automations
        .iter()
        .any(|a| a.target == AutomationTarget::EqLow));
    // Shape matches EqualPowerCrossfade: outgoing Fader; incoming Fader + Tempo Step.
    assert_eq!(r.outgoing_automations.len(), 1);
    assert_eq!(r.incoming_automations.len(), 2);
}

#[test]
fn bass_swap_bass_heavy_pair_uses_wider_window() {
    let out_track = track_with_bass(120.0, 0.7);
    let in_track = track_with_bass(120.0, 0.8);
    let plan = standard_plan();
    let ctx = ctx_for(&out_track, &in_track, &plan);
    let r = BassSwap.build(&ctx);
    let bar_seconds = (60.0_f64 / 120.0) * 4.0;
    let l_frames = (plan.length_bars as f64 * bar_seconds * SR as f64) as u64;
    let out_kill = r
        .outgoing_automations
        .iter()
        .find(|a| a.target == AutomationTarget::EqLow && a.curve == Curve::Linear)
        .expect("outgoing kill");
    let in_release = r
        .incoming_automations
        .iter()
        .find(|a| a.target == AutomationTarget::EqLow && a.curve == Curve::Linear)
        .expect("incoming release");
    // Wide window: outgoing kill [0.30 L .. 0.48 L], incoming release [0.52 L .. 0.70 L].
    assert!((out_kill.t_start_frames as f64 - 0.30 * l_frames as f64).abs() < 10.0);
    assert!((out_kill.t_end_frames as f64 - 0.48 * l_frames as f64).abs() < 10.0);
    assert!((in_release.t_start_frames as f64 - 0.52 * l_frames as f64).abs() < 10.0);
    assert!((in_release.t_end_frames as f64 - 0.70 * l_frames as f64).abs() < 10.0);
}

#[test]
fn bass_swap_missing_band_energy_falls_back_to_default() {
    // band_energy_means = None on either track → treat as 0.5 → default window.
    let mut out_track = track_with_bass(120.0, 0.4);
    out_track.sidecar.band_energy_means = None;
    let mut in_track = track_with_bass(120.0, 0.4);
    in_track.sidecar.band_energy_means = None;
    let plan = standard_plan();
    let ctx = ctx_for(&out_track, &in_track, &plan);
    let r = BassSwap.build(&ctx);
    // Has EqLow automations (not skipped) and uses default window.
    let bar_seconds = (60.0_f64 / 120.0) * 4.0;
    let l_frames = (plan.length_bars as f64 * bar_seconds * SR as f64) as u64;
    let out_kill = r
        .outgoing_automations
        .iter()
        .find(|a| a.target == AutomationTarget::EqLow && a.curve == Curve::Linear)
        .expect("outgoing kill");
    // Default window: outgoing kill [0.35 L .. 0.48 L].
    assert!((out_kill.t_start_frames as f64 - 0.35 * l_frames as f64).abs() < 10.0);
    assert!((out_kill.t_end_frames as f64 - 0.48 * l_frames as f64).abs() < 10.0);
}

#[test]
fn pick_recipe_routes_use_eq_swap_to_bass_swap() {
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.0,
        incoming_crossfade_start_seconds: 0.0,
        length_bars: 16,
        use_eq_swap: true,
        skip_tempo_align: false,
    };
    assert_eq!(
        dj_engine::recipe::pick_recipe_plan_only(&plan),
        RecipeChoice::BassSwap
    );
}

// ---------------------------------------------------------------------------
// FilterFade and FilterKill recipe tests (sub-project 3, task 3)
// ---------------------------------------------------------------------------

use dj_engine::recipe::{FilterFade, FilterKill};

#[test]
fn filter_fade_emits_fader_pair_plus_incoming_filter_sweep() {
    let outgoing = track(120.0);
    let incoming = track(120.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
    };
    let ctx = ctx_for(&outgoing, &incoming, &plan);
    let r = FilterFade.build(&ctx);
    // Outgoing: just the Fader (no filter on outgoing in FilterFade).
    assert_eq!(r.outgoing_automations.len(), 1);
    assert_eq!(r.outgoing_automations[0].target, AutomationTarget::Fader);
    assert_eq!(r.outgoing_automations[0].curve, Curve::EqualPowerOut);
    // Incoming: Fader, Tempo Step, Filter Step (init at +1), Filter Linear (+1->0).
    assert_eq!(r.incoming_automations.len(), 4);
    let filter_init = r
        .incoming_automations
        .iter()
        .find(|a| a.target == AutomationTarget::Filter && a.curve == Curve::Step)
        .expect("incoming filter init");
    assert_eq!(filter_init.from, 1.0);
    assert_eq!(filter_init.to, 1.0);
    let filter_sweep = r
        .incoming_automations
        .iter()
        .find(|a| a.target == AutomationTarget::Filter && a.curve == Curve::Linear)
        .expect("incoming filter sweep");
    assert_eq!(filter_sweep.from, 1.0);
    assert_eq!(filter_sweep.to, 0.0);
}

use dj_engine::recipe::{EchoOut, LoopOut};

#[test]
fn echo_out_emits_fader_pair_plus_outgoing_echo_ramp_and_init() {
    let outgoing = track(120.0);
    let incoming = track(120.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
    };
    let ctx = ctx_for(&outgoing, &incoming, &plan);
    let r = EchoOut.build(&ctx);

    // Outgoing: Fader + EchoMix ramp.
    assert_eq!(r.outgoing_automations.len(), 2);
    assert!(r
        .outgoing_automations
        .iter()
        .any(|a| a.target == AutomationTarget::Fader && a.curve == Curve::EqualPowerOut));
    let echo_ramp = r
        .outgoing_automations
        .iter()
        .find(|a| a.target == AutomationTarget::EchoMix)
        .expect("outgoing echo ramp");
    assert_eq!(echo_ramp.from, 0.0);
    assert!(
        (echo_ramp.to - 0.55).abs() < 1e-9,
        "EchoOut wet ramp peaks at 0.55"
    );

    // Incoming: Fader + Tempo Step.
    assert_eq!(r.incoming_automations.len(), 2);

    // Init: outgoing has echo configured; incoming has no init.
    let init = r.outgoing_init.as_ref().expect("outgoing_init for EchoOut");
    assert!(init.echo_delay_seconds.is_some());
    assert!(
        (init.echo_feedback.unwrap_or(0.0) - 0.35).abs() < 1e-9,
        "EchoOut feedback tamed to 0.35"
    );
    assert!(r.incoming_init.is_none());
}

#[test]
fn loop_out_emits_fader_pair_plus_outgoing_loop_gate_and_init() {
    let outgoing = track(120.0);
    let incoming = track(120.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
    };
    let ctx = ctx_for(&outgoing, &incoming, &plan);
    let r = LoopOut.build(&ctx);

    // Outgoing: Fader + LoopGate on + LoopGate off.
    assert_eq!(r.outgoing_automations.len(), 3);
    let loop_ons: Vec<_> = r
        .outgoing_automations
        .iter()
        .filter(|a| a.target == AutomationTarget::LoopGate)
        .collect();
    assert_eq!(loop_ons.len(), 2);

    // Init: outgoing has loop configured.
    let init = r.outgoing_init.as_ref().expect("outgoing_init for LoopOut");
    assert_eq!(init.loop_bars, Some(4));
    assert!(init.loop_bar_seconds.is_some());
}

#[test]
fn filter_kill_emits_outgoing_filter_sweep_to_plus_1_over_60_percent() {
    let outgoing = track(120.0);
    let incoming = track(120.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
    };
    let ctx = ctx_for(&outgoing, &incoming, &plan);
    let r = FilterKill.build(&ctx);
    // Outgoing: Fader + Filter Linear 0->+1 over first 60% of L.
    assert_eq!(r.outgoing_automations.len(), 2);
    let out_filter = r
        .outgoing_automations
        .iter()
        .find(|a| a.target == AutomationTarget::Filter && a.curve == Curve::Linear)
        .expect("outgoing filter sweep");
    assert_eq!(out_filter.from, 0.0);
    assert_eq!(out_filter.to, 1.0);
    let bar_seconds = (60.0_f64 / 120.0) * 4.0;
    let l_frames = (plan.length_bars as f64 * bar_seconds * SR as f64) as u64;
    let expected_end = (0.6 * l_frames as f64) as u64;
    assert!(
        (out_filter.t_end_frames as i64 - expected_end as i64).abs() < 100,
        "expected end near {expected_end}, got {}",
        out_filter.t_end_frames
    );
    // Incoming: Fader + Tempo Step. No filter automation on incoming.
    assert_eq!(r.incoming_automations.len(), 2);
}

// ---------------------------------------------------------------------------
// score_recipe + PairScoreInputs tests (sub-project 5, task 2)
// ---------------------------------------------------------------------------

use dj_engine::recipe::{score_recipe, PairScoreInputs};

fn inputs_default() -> PairScoreInputs {
    PairScoreInputs {
        length_bars: 16,
        use_eq_swap: false,
        energy_delta: 0.0,
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
        stems_available: false,
    }
}

#[test]
fn score_hard_cut_only_when_length_zero() {
    let mut i = inputs_default();
    i.length_bars = 0;
    assert_eq!(score_recipe(RecipeChoice::HardCut, &i), 1.0);
    i.length_bars = 16;
    assert_eq!(score_recipe(RecipeChoice::HardCut, &i), 0.0);
}

#[test]
fn score_equal_power_is_baseline_half() {
    let i = inputs_default();
    // Deprioritized in Phase D (0.50 → 0.15) so stem-aware recipes can take
    // over for the bulk of phrase-aligned same-BPM cases.
    assert!((score_recipe(RecipeChoice::EqualPower, &i) - 0.15).abs() < 1e-9);
}

#[test]
fn score_bass_swap_high_when_both_heavy_and_phrase_aligned() {
    let mut i = inputs_default();
    i.outgoing_bass = 0.7;
    i.incoming_bass = 0.7;
    i.phrase_aligned = true;
    assert!(score_recipe(RecipeChoice::BassSwap, &i) > 0.8);
}

#[test]
fn score_bass_swap_low_when_not_phrase_aligned() {
    let mut i = inputs_default();
    i.outgoing_bass = 0.7;
    i.incoming_bass = 0.7;
    i.phrase_aligned = false;
    assert!(score_recipe(RecipeChoice::BassSwap, &i) < 0.2);
}

#[test]
fn score_filter_fade_rises_with_energy_boost() {
    let mut i = inputs_default();
    i.energy_delta = 0.3;
    assert!(score_recipe(RecipeChoice::FilterFade, &i) > 0.7);
    i.energy_delta = 0.10;
    assert!(score_recipe(RecipeChoice::FilterFade, &i) > 0.35);
    assert!(score_recipe(RecipeChoice::FilterFade, &i) < 0.45);
    i.energy_delta = -0.3;
    assert!(score_recipe(RecipeChoice::FilterFade, &i) < 0.01);
}

#[test]
fn score_filter_kill_rises_with_energy_drop() {
    let mut i = inputs_default();
    i.energy_delta = -0.3;
    assert!(score_recipe(RecipeChoice::FilterKill, &i) > 0.6);
    i.energy_delta = 0.3;
    assert!(score_recipe(RecipeChoice::FilterKill, &i) < 0.01);
}

#[test]
fn score_echo_out_rises_on_wind_down() {
    let mut i = inputs_default();
    // Deprioritized in Phase D (0.65 → 0.45 wind-down, 0.35 → 0.20 baseline).
    i.energy_delta = -0.5;
    assert!(score_recipe(RecipeChoice::EchoOut, &i) > 0.4);
    i.energy_delta = 0.0;
    let baseline = score_recipe(RecipeChoice::EchoOut, &i);
    assert!(baseline > 0.15 && baseline < 0.25);
}

#[test]
fn score_loop_out_is_low_baseline() {
    let i = inputs_default();
    assert!(score_recipe(RecipeChoice::LoopOut, &i) < 0.35);
}

// ---------------------------------------------------------------------------
// scored pick_recipe tests (sub-project 5, task 3)
// ---------------------------------------------------------------------------

use dj_engine::recipe::pick_recipe_plan_only;
use dj_engine::recipe_recency::RecipeRecency;

fn bass_heavy_phrase_aligned_pair() -> (LoadedTrack, LoadedTrack) {
    let mut a = track_with_bass(120.0, 0.7);
    let mut b = track_with_bass(120.0, 0.7);
    a.sidecar.is_phrase_aligned = Some(true);
    b.sidecar.is_phrase_aligned = Some(true);
    (a, b)
}

fn standard_blend_plan() -> TransitionPlan {
    TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
    }
}

#[test]
fn scored_picks_hard_cut_when_length_zero() {
    let (a, b) = bass_heavy_phrase_aligned_pair();
    let mut plan = standard_blend_plan();
    plan.length_bars = 0;
    let recency = RecipeRecency::new(5);
    assert_eq!(pick_recipe(&plan, &a, &b, &recency), RecipeChoice::HardCut);
}

#[test]
fn scored_picks_bass_swap_for_heavy_aligned_pair_no_recency() {
    let (a, b) = bass_heavy_phrase_aligned_pair();
    let plan = standard_blend_plan();
    let recency = RecipeRecency::new(5);
    // ThreeBandSwap (0.90) now outscores BassSwap (0.85) for bass-heavy
    // phrase-aligned pairs with length_bars >= 16; accept either.
    let pick = pick_recipe(&plan, &a, &b, &recency);
    assert!(
        pick == RecipeChoice::BassSwap || pick == RecipeChoice::ThreeBandSwap,
        "expected BassSwap or ThreeBandSwap, got {pick:?}"
    );
}

#[test]
fn scored_skips_recent_bass_swap() {
    let (a, b) = bass_heavy_phrase_aligned_pair();
    let plan = standard_blend_plan();
    let mut recency = RecipeRecency::new(5);
    recency.record(RecipeChoice::BassSwap);
    // Penalized 0.85 * 0.5 = 0.425, below EqualPower's 0.5. Picker should
    // switch to EqualPower (or EchoOut at 0.35, but EqualPower is higher).
    let pick = pick_recipe(&plan, &a, &b, &recency);
    assert_ne!(
        pick,
        RecipeChoice::BassSwap,
        "should skip recently-used BassSwap"
    );
}

#[test]
fn scored_picks_filter_fade_for_big_energy_boost() {
    let mut a = track_with_bass(120.0, 0.5);
    let mut b = track_with_bass(120.0, 0.5);
    a.sidecar.energy_mean = Some(0.3);
    b.sidecar.energy_mean = Some(0.7); // +0.4 boost
    a.sidecar.is_phrase_aligned = Some(false);
    b.sidecar.is_phrase_aligned = Some(false);
    let plan = standard_blend_plan();
    let recency = RecipeRecency::new(5);
    assert_eq!(
        pick_recipe(&plan, &a, &b, &recency),
        RecipeChoice::FilterFade
    );
}

#[test]
fn scored_picks_filter_kill_or_echo_out_for_big_energy_drop() {
    let mut a = track_with_bass(120.0, 0.5);
    let mut b = track_with_bass(120.0, 0.5);
    a.sidecar.energy_mean = Some(0.8);
    b.sidecar.energy_mean = Some(0.4); // -0.4 drop
    a.sidecar.is_phrase_aligned = Some(false);
    b.sidecar.is_phrase_aligned = Some(false);
    let plan = standard_blend_plan();
    let recency = RecipeRecency::new(5);
    let pick = pick_recipe(&plan, &a, &b, &recency);
    assert!(
        pick == RecipeChoice::FilterKill || pick == RecipeChoice::EchoOut,
        "expected FilterKill or EchoOut, got {pick:?}"
    );
}

#[test]
fn pick_recipe_plan_only_preserved_for_legacy_tests() {
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.0,
        incoming_crossfade_start_seconds: 0.0,
        length_bars: 16,
        use_eq_swap: true,
        skip_tempo_align: false,
    };
    assert_eq!(pick_recipe_plan_only(&plan), RecipeChoice::BassSwap);
}

#[test]
fn score_three_band_swap_both_heavy_phrase_aligned_long() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.0,
        outgoing_bass: 0.7,
        incoming_bass: 0.6,
        phrase_aligned: true,
        stems_available: false,
    };
    assert!((score_recipe(RecipeChoice::ThreeBandSwap, &inputs) - 0.90).abs() < 1e-9);
}

#[test]
fn score_three_band_swap_both_heavy_short_window() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 8,
        use_eq_swap: true,
        energy_delta: 0.0,
        outgoing_bass: 0.7,
        incoming_bass: 0.6,
        phrase_aligned: true,
        stems_available: false,
    };
    assert!((score_recipe(RecipeChoice::ThreeBandSwap, &inputs) - 0.70).abs() < 1e-9);
}

#[test]
fn score_echo_throw_smooth_long() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: false,
        energy_delta: 0.05,
        outgoing_bass: 0.4,
        incoming_bass: 0.4,
        phrase_aligned: true,
        stems_available: false,
    };
    assert!((score_recipe(RecipeChoice::EchoThrow, &inputs) - 0.65).abs() < 1e-9);
}

#[test]
fn score_highs_first_big_ramp_long() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: false,
        energy_delta: 0.30,
        outgoing_bass: 0.4,
        incoming_bass: 0.4,
        phrase_aligned: true,
        stems_available: false,
    };
    assert!((score_recipe(RecipeChoice::HighsFirst, &inputs) - 0.85).abs() < 1e-9);
}

#[test]
fn score_highs_first_no_ramp_low_score() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: false,
        energy_delta: 0.0,
        outgoing_bass: 0.4,
        incoming_bass: 0.4,
        phrase_aligned: true,
        stems_available: false,
    };
    assert!((score_recipe(RecipeChoice::HighsFirst, &inputs) - 0.10).abs() < 1e-9);
}

// ---------------------------------------------------------------------------
// inject_pitch_journey + build_and_compile tests (pitch-journey-back feature)
// ---------------------------------------------------------------------------

#[test]
fn build_and_compile_injects_tempo_ramp_for_modest_ratio() {
    use dj_engine::automation::{AutomationTarget, Curve};
    use dj_engine::recipe::{build_and_compile, EqualPowerCrossfade};

    // 120 BPM outgoing, 126 BPM incoming → ratio ≈ 0.9524 — in the journey band.
    let outgoing = track_with_bpm(120.0);
    let incoming = track_with_bpm(126.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
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
    let out = build_and_compile(&EqualPowerCrossfade, &ctx);

    let tempo_ramps: Vec<_> = out
        .incoming_automations
        .iter()
        .filter(|a| a.target == AutomationTarget::Tempo && a.curve == Curve::Linear)
        .collect();
    assert_eq!(
        tempo_ramps.len(),
        1,
        "expected exactly one Tempo Linear automation"
    );
    let ramp = tempo_ramps[0];
    assert!((ramp.from - (120.0 / 126.0)).abs() < 1e-6);
    assert!((ramp.to - 1.0).abs() < 1e-9);
}

#[test]
fn build_and_compile_does_not_inject_for_identical_bpm() {
    use dj_engine::automation::{AutomationTarget, Curve};
    use dj_engine::recipe::{build_and_compile, EqualPowerCrossfade};

    let outgoing = track_with_bpm(120.0);
    let incoming = track_with_bpm(120.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
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
    let out = build_and_compile(&EqualPowerCrossfade, &ctx);

    let tempo_ramps: Vec<_> = out
        .incoming_automations
        .iter()
        .filter(|a| a.target == AutomationTarget::Tempo && a.curve == Curve::Linear)
        .collect();
    assert_eq!(tempo_ramps.len(), 0, "should not inject when ratio == 1.0");
}

#[test]
fn build_and_compile_does_not_inject_for_large_ratio() {
    use dj_engine::automation::{AutomationTarget, Curve};
    use dj_engine::recipe::{build_and_compile, EqualPowerCrossfade};

    // 120 BPM → 152 BPM = ratio 0.789 — well outside the journey band.
    let outgoing = track_with_bpm(120.0);
    let incoming = track_with_bpm(152.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
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
    let out = build_and_compile(&EqualPowerCrossfade, &ctx);

    let tempo_ramps: Vec<_> = out
        .incoming_automations
        .iter()
        .filter(|a| a.target == AutomationTarget::Tempo && a.curve == Curve::Linear)
        .collect();
    assert_eq!(
        tempo_ramps.len(),
        0,
        "should not inject when ratio is too far from 1.0"
    );
}

#[test]
fn build_and_compile_does_not_inject_when_skip_tempo_align() {
    use dj_engine::automation::{AutomationTarget, Curve};
    use dj_engine::recipe::build_and_compile;

    let outgoing = track_with_bpm(120.0);
    let incoming = track_with_bpm(125.0); // modest ratio in normal circumstances
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 0,
        use_eq_swap: false,
        skip_tempo_align: true, // the planner decided no-sync
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
    let out = build_and_compile(&dj_engine::recipe::HardCut, &ctx);

    let tempo_ramps: Vec<_> = out
        .incoming_automations
        .iter()
        .filter(|a| a.target == AutomationTarget::Tempo && a.curve == Curve::Linear)
        .collect();
    assert_eq!(
        tempo_ramps.len(),
        0,
        "skip_tempo_align must suppress the journey"
    );
}

#[test]
fn inject_pitch_journey_is_idempotent() {
    use dj_engine::recipe::{inject_pitch_journey, EqualPowerCrossfade, Recipe};
    use dj_engine::recipe_script::{DeckRole, Phase};

    let outgoing = track_with_bpm(120.0);
    let incoming = track_with_bpm(126.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
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
    let mut script = EqualPowerCrossfade.build_script(&ctx);
    inject_pitch_journey(&mut script, &ctx);
    inject_pitch_journey(&mut script, &ctx); // second call should be a no-op

    let ramps: Vec<_> = script
        .phases
        .iter()
        .filter(|p| {
            matches!(
                p,
                Phase::TempoRamp {
                    deck: DeckRole::Incoming,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(
        ramps.len(),
        1,
        "second injection should not duplicate the ramp"
    );
}

#[test]
fn score_drop_big_ramp_up() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 8,
        use_eq_swap: false,
        energy_delta: 0.30,
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
        stems_available: false,
    };
    assert!((score_recipe(RecipeChoice::Drop, &inputs) - 0.75).abs() < 1e-9);
}

#[test]
fn score_breakdown_bridge_winddown() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 8,
        use_eq_swap: false,
        energy_delta: -0.25,
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
        stems_available: false,
    };
    assert!((score_recipe(RecipeChoice::BreakdownBridge, &inputs) - 0.70).abs() < 1e-9);
}

#[test]
fn score_double_drop_jackpot() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.02,
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
        stems_available: false,
    };
    assert!((score_recipe(RecipeChoice::DoubleDrop, &inputs) - 0.80).abs() < 1e-9);
}

#[test]
fn score_double_drop_misses_when_energy_too_far() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.20, // outside the < 0.05 gate
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
        stems_available: false,
    };
    assert!((score_recipe(RecipeChoice::DoubleDrop, &inputs) - 0.05).abs() < 1e-9);
}
