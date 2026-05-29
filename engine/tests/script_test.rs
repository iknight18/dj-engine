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
        stems: None,
        local_bpm: vec![],
        beat_confidence: vec![],
        downbeat_confidence: vec![],
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
        skip_tempo_align: false,
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
    assert_eq!(script.phases.len(), 11, "phase count");

    let has_high_init = script.phases.iter().any(|p| {
        matches!(p,
        Phase::EqStep { deck: DeckRole::Incoming, band: EqBand::High, value_db, at }
            if (*value_db - (-40.0)).abs() < 1e-9 && (at.bars - 0.0).abs() < 1e-9)
    });
    assert!(has_high_init, "highs init step at bar 0");

    // Mids cross is now two Steps on downbeats (was a Linear ramp over [6, 10]):
    // outgoing mids kill at bar L/2 = 8, incoming mids release at bar L/2 + 1 = 9.
    let has_mids_outgoing_step = script.phases.iter().any(|p| {
        matches!(p,
        Phase::EqStep { deck: DeckRole::Outgoing, band: EqBand::Mid, value_db, at }
            if (*value_db - (-40.0)).abs() < 1e-9 && (at.bars - 8.0).abs() < 1e-9)
    });
    assert!(has_mids_outgoing_step, "outgoing mids Step kill at bar 8");

    let has_mids_incoming_release = script.phases.iter().any(|p| {
        matches!(p,
        Phase::EqStep { deck: DeckRole::Incoming, band: EqBand::Mid, value_db, at }
            if value_db.abs() < 1e-9 && (at.bars - 9.0).abs() < 1e-9)
    });
    assert!(has_mids_incoming_release, "incoming mids Step release at bar 9");

    let has_bass_swap_outgoing = script.phases.iter().any(|p| {
        matches!(p,
        Phase::EqStep { deck: DeckRole::Outgoing, band: EqBand::Low, value_db, at }
            if (*value_db - (-40.0)).abs() < 1e-9 && (at.bars - 14.0).abs() < 1e-9)
    });
    assert!(has_bass_swap_outgoing, "outgoing bass-kill at bar 14");

    let has_bass_swap_incoming = script.phases.iter().any(|p| {
        matches!(p,
        Phase::EqStep { deck: DeckRole::Incoming, band: EqBand::Low, value_db, at }
            if (*value_db - 0.0).abs() < 1e-9 && (at.bars - 14.0).abs() < 1e-9)
    });
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

    let has_bass_swap = script.phases.iter().any(|p| {
        matches!(p,
        Phase::EqStep { deck: DeckRole::Outgoing, band: EqBand::Low, value_db, at }
            if (*value_db - (-40.0)).abs() < 1e-9 && (at.bars - 7.0).abs() < 1e-9)
    });
    assert!(has_bass_swap, "outgoing bass-kill at bar 7 for L=8");
}

#[test]
fn echo_throw_16_bar_emits_triangle_echo_mix() {
    use dj_engine::recipe::EchoThrow;
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
    let script = EchoThrow.build_script(&ctx);
    assert_eq!(script.length_bars, 16);

    let init = script.outgoing_init.expect("outgoing_init for echo");
    assert!((init.echo_delay_seconds.unwrap() - 0.5).abs() < 1e-9);
    // Feedback tamed from 0.75 to 0.5.
    assert!((init.echo_feedback.unwrap() - 0.5).abs() < 1e-9);

    // EchoThrow now emits a triangle via two EchoMix ramps instead of a Phase::EchoThrow.
    // Swell: bar 14.0 → bar 14.25, 0.0 → 0.6
    let has_swell = script.phases.iter().any(|p| {
        matches!(p,
        Phase::EchoMix { deck: DeckRole::Outgoing, from, to, span, .. }
            if (*from - 0.0).abs() < 1e-9 && (*to - 0.6).abs() < 1e-9
               && (span.start.bars - 14.0).abs() < 1e-9 && (span.end.bars - 14.25).abs() < 1e-9)
    });
    assert!(has_swell, "triangle swell on [14.0, 14.25]");

    // Decay: bar 14.25 → bar 15.0, 0.6 → 0.0
    let has_decay = script.phases.iter().any(|p| {
        matches!(p,
        Phase::EchoMix { deck: DeckRole::Outgoing, from, to, span, .. }
            if (*from - 0.6).abs() < 1e-9 && (*to - 0.0).abs() < 1e-9
               && (span.start.bars - 14.25).abs() < 1e-9 && (span.end.bars - 15.0).abs() < 1e-9)
    });
    assert!(has_decay, "triangle decay on [14.25, 15.0]");
}

#[test]
fn highs_first_16_bar_bass_swap_at_bar_12() {
    use dj_engine::recipe::HighsFirst;
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
    let script = HighsFirst.build_script(&ctx);
    assert_eq!(script.length_bars, 16);

    let has_fast_fader = script.phases.iter().any(|p| {
        matches!(p,
        Phase::Fader { deck: DeckRole::Incoming, from, to, span, curve: Curve::EqualPowerIn }
            if (*from - 0.0).abs() < 1e-9 && (*to - 1.0).abs() < 1e-9
               && (span.start.bars - 0.0).abs() < 1e-9 && (span.end.bars - 2.0).abs() < 1e-9)
    });
    assert!(has_fast_fader, "incoming fader fast-rise on [0, 2]");

    let has_bass_swap_out = script.phases.iter().any(|p| {
        matches!(p,
        Phase::EqStep { deck: DeckRole::Outgoing, band: EqBand::Low, value_db, at }
            if (*value_db - (-40.0)).abs() < 1e-9 && (at.bars - 12.0).abs() < 1e-9)
    });
    let has_bass_swap_in = script.phases.iter().any(|p| {
        matches!(p,
        Phase::EqStep { deck: DeckRole::Incoming, band: EqBand::Low, value_db, at }
            if (*value_db - 0.0).abs() < 1e-9 && (at.bars - 12.0).abs() < 1e-9)
    });
    assert!(has_bass_swap_out && has_bass_swap_in, "bass swap at bar 12");

    // Mids reveal is now a Step on bar L/4 = 4 (was a Linear ramp over [4, 8]).
    let has_mids_step = script.phases.iter().any(|p| {
        matches!(p,
        Phase::EqStep { deck: DeckRole::Incoming, band: EqBand::Mid, value_db, at }
            if value_db.abs() < 1e-9 && (at.bars - 4.0).abs() < 1e-9)
    });
    assert!(has_mids_step, "incoming mids Step release at bar 4");
}

#[test]
fn drop_emits_filter_buildup_and_release() {
    use dj_engine::recipe::Drop;
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
    let script = Drop.build_script(&ctx);
    assert_eq!(script.length_bars, 4);

    // Filter sweep 0.85 -> 0.55 over [0, 4].
    let has_buildup = script.phases.iter().any(|ph| {
        matches!(ph,
        Phase::Filter { deck: DeckRole::Incoming, from, to, span, curve: Curve::Linear }
            if (*from - 0.85).abs() < 1e-9 && (*to - 0.55).abs() < 1e-9
               && (span.start.bars - 0.0).abs() < 1e-9 && (span.end.bars - 4.0).abs() < 1e-9)
    });
    assert!(has_buildup, "filter build-up sweep on [0, 4]");

    // Filter snap to 0.0 at bar 4.
    let has_release = script.phases.iter().any(|ph| {
        matches!(ph,
        Phase::Filter { deck: DeckRole::Incoming, from, to, span, curve: Curve::Step }
            if (*from - 0.0).abs() < 1e-9 && (*to - 0.0).abs() < 1e-9
               && (span.start.bars - 4.0).abs() < 1e-9 && (span.end.bars - 4.0).abs() < 1e-9)
    });
    assert!(has_release, "filter release step at bar 4");
}

#[test]
fn breakdown_bridge_kills_outgoing_mids_and_bass() {
    use dj_engine::recipe::BreakdownBridge;
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
    let script = BreakdownBridge.build_script(&ctx);
    assert_eq!(script.length_bars, 4);

    let has_low_kill = script.phases.iter().any(|ph| {
        matches!(ph,
        Phase::EqRamp { deck: DeckRole::Outgoing, band: EqBand::Low, from_db, to_db, .. }
            if (*from_db - 0.0).abs() < 1e-9 && (*to_db - (-40.0)).abs() < 1e-9)
    });
    assert!(has_low_kill, "outgoing low kill");

    let has_mid_kill = script.phases.iter().any(|ph| {
        matches!(ph,
        Phase::EqRamp { deck: DeckRole::Outgoing, band: EqBand::Mid, from_db, to_db, .. }
            if (*from_db - 0.0).abs() < 1e-9 && (*to_db - (-40.0)).abs() < 1e-9)
    });
    assert!(has_mid_kill, "outgoing mid kill");

    let has_incoming_drop = script.phases.iter().any(|ph| {
        matches!(ph,
        Phase::Fader { deck: DeckRole::Incoming, from, to, span, curve: Curve::Step }
            if (*from - 0.0).abs() < 1e-9 && (*to - 1.0).abs() < 1e-9
               && (span.start.bars - 4.0).abs() < 1e-9)
    });
    assert!(has_incoming_drop, "incoming hard-cut in at bar 4");
}

#[test]
fn vocal_overlay_emits_stem_fader_phases() {
    use dj_engine::deck::StemKind;
    use dj_engine::recipe::VocalOverlay;
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
    let script = VocalOverlay.build_script(&ctx);
    assert_eq!(script.length_bars, 16);

    // Incoming vocals ramp [0, 4].
    let has_vocal_intro = script.phases.iter().any(|ph| {
        matches!(ph,
        Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Vocals, from, to, span, curve: Curve::Linear }
            if (*from - 0.0).abs() < 1e-9 && (*to - 1.0).abs() < 1e-9
               && (span.start.bars - 0.0).abs() < 1e-9 && (span.end.bars - 4.0).abs() < 1e-9)
    });
    assert!(has_vocal_intro);

    // Incoming drums ramp [12, 16].
    let has_drums_intro = script.phases.iter().any(|ph| {
        matches!(ph,
        Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Drums, from, to, span, curve: Curve::Linear }
            if (*from - 0.0).abs() < 1e-9 && (*to - 1.0).abs() < 1e-9
               && (span.start.bars - 12.0).abs() < 1e-9 && (span.end.bars - 16.0).abs() < 1e-9)
    });
    assert!(has_drums_intro);
}

#[test]
fn bass_swap_stem_aware_emits_bass_stem_phases() {
    use dj_engine::deck::StemKind;
    use dj_engine::recipe::BassSwapStemAware;
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
    let script = BassSwapStemAware.build_script(&ctx);
    assert_eq!(script.length_bars, 16);

    // Outgoing bass stem killed somewhere — at least one bass-targeted StemFader on outgoing.
    let has_outgoing_bass_kill = script.phases.iter().any(|ph| {
        matches!(
            ph,
            Phase::StemFader {
                deck: DeckRole::Outgoing,
                stem: StemKind::Bass,
                ..
            }
        )
    });
    assert!(has_outgoing_bass_kill);

    // Incoming bass stem released somewhere.
    let has_incoming_bass_release = script.phases.iter().any(|ph| {
        matches!(
            ph,
            Phase::StemFader {
                deck: DeckRole::Incoming,
                stem: StemKind::Bass,
                ..
            }
        )
    });
    assert!(has_incoming_bass_release);
}

#[test]
fn breakdown_into_drop_emits_drum_breakdown_and_drop() {
    use dj_engine::deck::StemKind;
    use dj_engine::recipe::BreakdownIntoDrop;
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
    let script = BreakdownIntoDrop.build_script(&ctx);
    assert_eq!(script.length_bars, 8);

    // Outgoing drum stem ramps 1.0 -> 0.0.
    let has_drum_breakdown = script.phases.iter().any(|ph| {
        matches!(ph,
        Phase::StemFader { deck: DeckRole::Outgoing, stem: StemKind::Drums, from, to, .. }
            if (*from - 1.0).abs() < 1e-9 && (*to - 0.0).abs() < 1e-9)
    });
    assert!(has_drum_breakdown);

    // Incoming mixed fader hard-cuts 0 -> 1.
    let has_drop = script.phases.iter().any(|ph| {
        matches!(ph,
        Phase::Fader { deck: DeckRole::Incoming, from, to, curve: Curve::Step, .. }
            if (*from - 0.0).abs() < 1e-9 && (*to - 1.0).abs() < 1e-9)
    });
    assert!(has_drop);
}

#[test]
fn double_drop_holds_silence_then_both_play() {
    use dj_engine::recipe::DoubleDrop;
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
    let script = DoubleDrop.build_script(&ctx);
    assert_eq!(script.length_bars, 2);

    // Outgoing fades to silence over [0, 0.5].
    let has_initial_fadeout = script.phases.iter().any(|ph| {
        matches!(ph,
        Phase::Fader { deck: DeckRole::Outgoing, from, to, span, curve: Curve::Linear }
            if (*from - 1.0).abs() < 1e-9 && (*to - 0.0).abs() < 1e-9
               && (span.start.bars - 0.0).abs() < 1e-9 && (span.end.bars - 0.5).abs() < 1e-9)
    });
    assert!(has_initial_fadeout, "outgoing initial fade to silence");

    // Both decks come back at bar 1.
    let has_outgoing_return = script.phases.iter().any(|ph| {
        matches!(ph,
        Phase::Fader { deck: DeckRole::Outgoing, from, to, span, curve: Curve::Step }
            if (*from - 0.0).abs() < 1e-9 && (*to - 1.0).abs() < 1e-9
               && (span.start.bars - 1.0).abs() < 1e-9)
    });
    assert!(has_outgoing_return, "outgoing returns at bar 1");

    let has_incoming_return = script.phases.iter().any(|ph| {
        matches!(ph,
        Phase::Fader { deck: DeckRole::Incoming, from, to, span, curve: Curve::Step }
            if (*from - 0.0).abs() < 1e-9 && (*to - 1.0).abs() < 1e-9
               && (span.start.bars - 1.0).abs() < 1e-9)
    });
    assert!(has_incoming_return, "incoming hard-cuts in at bar 1");
}
