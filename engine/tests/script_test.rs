use dj_engine::automation::Curve;
use dj_engine::library::LoadedTrack;
use dj_engine::recipe::{DeckId, Recipe, ThreeBandSwap, TransitionContext};
use dj_engine::recipe_script::{DeckRole, EqBand, Phase};
use dj_engine::sidecar::Sidecar;
use dj_engine::transition::TransitionPlan;
use std::path::PathBuf;

const SR: u32 = 44100;

fn sidecar() -> Sidecar {
    Sidecar {
        schema_version: 1,
        analyzer_version: "test".into(),
        source_path: "test.mp3".into(),
        source_mtime: 0.0,
        source_size_bytes: 0,
        duration_seconds: Some(300.0),
        sample_rate: Some(SR),
        bpm: Some(120.0),
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
        cue_out_seconds: Some(280.0),
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

fn track() -> LoadedTrack {
    LoadedTrack {
        sidecar_path: PathBuf::new(),
        sidecar: sidecar(),
    }
}

fn plan(length_bars: usize) -> TransitionPlan {
    TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars,
        use_eq_swap: false,
    }
}

#[test]
fn three_band_swap_16_bar_emits_expected_phase_shape() {
    let outgoing = track();
    let incoming = track();
    let p = plan(16);
    let ctx = TransitionContext {
        outgoing: &outgoing,
        incoming: &incoming,
        plan: &p,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let script = ThreeBandSwap.build_script(&ctx);
    assert_eq!(script.length_bars, 16);
    assert!(script.outgoing_init.is_none());
    assert!(script.incoming_init.is_none());
    assert_eq!(script.phases.len(), 10, "phase count");

    let has_high_init = script.phases.iter().any(|p| matches!(p,
        Phase::EqStep { deck: DeckRole::Incoming, band: EqBand::High, value_db, at }
            if (*value_db - (-40.0)).abs() < 1e-9 && (at.bars - 0.0).abs() < 1e-9));
    assert!(has_high_init, "highs init step at bar 0");

    let has_mids_outgoing = script.phases.iter().any(|p| matches!(p,
        Phase::EqRamp { deck: DeckRole::Outgoing, band: EqBand::Mid, from_db, to_db, span, .. }
            if (*from_db - 0.0).abs() < 1e-9 && (*to_db - (-40.0)).abs() < 1e-9
               && (span.start.bars - 6.0).abs() < 1e-9 && (span.end.bars - 10.0).abs() < 1e-9));
    assert!(has_mids_outgoing, "outgoing mids ramp on [6, 10]");

    let has_bass_swap_outgoing = script.phases.iter().any(|p| matches!(p,
        Phase::EqStep { deck: DeckRole::Outgoing, band: EqBand::Low, value_db, at }
            if (*value_db - (-40.0)).abs() < 1e-9 && (at.bars - 14.0).abs() < 1e-9));
    assert!(has_bass_swap_outgoing, "outgoing bass-kill at bar 14");

    let has_bass_swap_incoming = script.phases.iter().any(|p| matches!(p,
        Phase::EqStep { deck: DeckRole::Incoming, band: EqBand::Low, value_db, at }
            if (*value_db - 0.0).abs() < 1e-9 && (at.bars - 14.0).abs() < 1e-9));
    assert!(has_bass_swap_incoming, "incoming bass-release at bar 14");

    let _ = Curve::EqualPowerIn;
}

#[test]
fn three_band_swap_8_bar_swap_lands_at_bar_7() {
    let outgoing = track();
    let incoming = track();
    let p = plan(8);
    let ctx = TransitionContext {
        outgoing: &outgoing,
        incoming: &incoming,
        plan: &p,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let script = ThreeBandSwap.build_script(&ctx);

    let has_bass_swap = script.phases.iter().any(|p| matches!(p,
        Phase::EqStep { deck: DeckRole::Outgoing, band: EqBand::Low, value_db, at }
            if (*value_db - (-40.0)).abs() < 1e-9 && (at.bars - 7.0).abs() < 1e-9));
    assert!(has_bass_swap, "outgoing bass-kill at bar 7 for L=8");
}
