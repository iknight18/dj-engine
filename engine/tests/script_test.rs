use dj_engine::automation::Curve;
use dj_engine::library::LoadedTrack;
use dj_engine::recipe::{DeckId, Recipe, TransitionContext};
use dj_engine::recipe_script::{DeckRole, Phase};
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
        ratio: 1.0,
        ratio_global: 1.0,
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
fn rhythm_swap_emits_gradual_reveal_and_fast_handoff_phases() {
    use dj_engine::deck::StemKind;
    use dj_engine::recipe::{Recipe, RhythmSwap};

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
        ratio: 1.0,
        ratio_global: 1.0,
    };

    let script = RhythmSwap.build_script(&ctx);

    // Outgoing drums Linear ramp 1→0.
    let out_drums_ramp = script.phases.iter().any(|p| {
        matches!(
            p,
            Phase::StemFader {
                deck: DeckRole::Outgoing,
                stem: StemKind::Drums,
                from,
                to,
                ..
            } if (from - 1.0).abs() < 1e-9 && (to - 0.0).abs() < 1e-9
        )
    });
    assert!(out_drums_ramp, "outgoing drums Linear ramp missing");

    // Incoming drums Linear ramp 0→1.
    let in_drums_ramp = script.phases.iter().any(|p| {
        matches!(
            p,
            Phase::StemFader {
                deck: DeckRole::Incoming,
                stem: StemKind::Drums,
                from,
                to,
                ..
            } if (from - 0.0).abs() < 1e-9 && (to - 1.0).abs() < 1e-9
        )
    });
    assert!(in_drums_ramp, "incoming drums Linear ramp missing");

    // Incoming vocals Step mute at bar 0.
    let in_vocals_mute = script.phases.iter().any(|p| {
        matches!(
            p,
            Phase::StemFader {
                deck: DeckRole::Incoming,
                stem: StemKind::Vocals,
                from,
                to,
                span,
                ..
            } if (from - 0.0).abs() < 1e-9 && (to - 0.0).abs() < 1e-9
                  && (span.start.bars - 0.0).abs() < 1e-9
        )
    });
    assert!(in_vocals_mute, "incoming vocals mute Step at bar 0 missing");

    // Vocal handoff Steps at bar 8 (swap_bar = L/2 for L=16).
    let outgoing_vocals_off_at_8 = script.phases.iter().any(|p| {
        matches!(
            p,
            Phase::StemFader {
                deck: DeckRole::Outgoing,
                stem: StemKind::Vocals,
                from,
                to,
                span,
                ..
            } if (from - 0.0).abs() < 1e-9 && (to - 0.0).abs() < 1e-9
                  && (span.start.bars - 8.0).abs() < 1e-9
        )
    });
    assert!(
        outgoing_vocals_off_at_8,
        "outgoing vocals OFF Step at bar 8 missing"
    );

    let incoming_vocals_on_at_8 = script.phases.iter().any(|p| {
        matches!(
            p,
            Phase::StemFader {
                deck: DeckRole::Incoming,
                stem: StemKind::Vocals,
                from,
                to,
                span,
                ..
            } if (from - 1.0).abs() < 1e-9 && (to - 1.0).abs() < 1e-9
                  && (span.start.bars - 8.0).abs() < 1e-9
        )
    });
    assert!(
        incoming_vocals_on_at_8,
        "incoming vocals ON Step at bar 8 missing"
    );
}

#[test]
fn rhythm_swap_requires_stems_is_true() {
    use dj_engine::recipe::{Recipe, RhythmSwap};
    assert!(RhythmSwap.requires_stems());
}

#[test]
fn loop_bridge_emits_three_act_phases() {
    use dj_engine::deck::StemKind;
    use dj_engine::recipe::{LoopBridge, Recipe};
    use dj_engine::recipe_script::{DeckRole, Phase};

    let mut outgoing = track();
    outgoing.sidecar.bpm = Some(120.0);
    outgoing.sidecar.outro_start_seconds = Some(60.0);
    outgoing.sidecar.phrase_boundaries_seconds = Some(vec![16.0, 32.0, 48.0, 64.0]);
    outgoing.sidecar.key = Some("8A".to_string());

    let mut incoming = track();
    incoming.sidecar.bpm = Some(120.0);
    incoming.sidecar.downbeats_seconds = (0..32).map(|i| i as f64 * 2.0).collect();
    incoming.sidecar.key = Some("8A".to_string());
    incoming.sidecar.stems = Some(dj_engine::sidecar::Stems {
        drum_density: vec![],
        bass_density: vec![],
        vocal_density: vec![],
        vocal_segments: vec![[16.0, 24.0]],
        energy_per_stem: Default::default(),
        demucs_version: None,
        model: None,
    });

    // Custom plan whose outgoing/incoming crossfade-start anchors align with
    // the sidecar test data so compute_loop_bridge_anchors yields a positive
    // phrase_end_bar.
    let p = TransitionPlan {
        outgoing_crossfade_start_seconds: 16.0,
        incoming_crossfade_start_seconds: 0.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
    };
    let ctx = TransitionContext {
        outgoing: &outgoing,
        incoming: &incoming,
        plan: &p,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
        ratio: 1.0,
        ratio_global: 1.0,
    };

    let script = LoopBridge.build_script(&ctx);

    // Act 1 — incoming stems isolated to rhythm-only at bar 0.
    let in_vocals_mute = script.phases.iter().any(|p| matches!(
        p,
        Phase::StemFader {
            deck: DeckRole::Incoming, stem: StemKind::Vocals,
            from, to, span, ..
        } if (from - 0.0).abs() < 1e-9 && (to - 0.0).abs() < 1e-9
              && (span.start.bars - 0.0).abs() < 1e-9
    ));
    assert!(in_vocals_mute, "incoming vocals mute at bar 0 missing");

    // LoopGate on incoming.
    let loopgate_on = script.phases.iter().any(|p| matches!(
        p,
        Phase::LoopGate { deck: DeckRole::Incoming, .. }
    ));
    assert!(loopgate_on, "LoopGate on incoming missing");

    // Act 2 — incoming filter sweep 0.85 → 0.
    let incoming_filter = script.phases.iter().any(|p| matches!(
        p,
        Phase::Filter {
            deck: DeckRole::Incoming, from, to, ..
        } if (from - 0.85).abs() < 1e-9 && (to - 0.0).abs() < 1e-9
    ));
    assert!(incoming_filter, "incoming filter 0.85->0 sweep missing");

    // Act 2 — outgoing filter sweep 0 → 0.6.
    let outgoing_filter = script.phases.iter().any(|p| matches!(
        p,
        Phase::Filter {
            deck: DeckRole::Outgoing, from, to, ..
        } if (from - 0.0).abs() < 1e-9 && (to - 0.6).abs() < 1e-9
    ));
    assert!(outgoing_filter, "outgoing filter 0->0.6 sweep missing");

    // Act 3 — incoming vocals restored Step at a later bar.
    let in_vocals_restored = script.phases.iter().any(|p| matches!(
        p,
        Phase::StemFader {
            deck: DeckRole::Incoming, stem: StemKind::Vocals,
            from, to, span, ..
        } if (from - 1.0).abs() < 1e-9 && (to - 1.0).abs() < 1e-9
              && span.start.bars > 0.0
    ));
    assert!(in_vocals_restored, "incoming vocals restored Step missing");

    // outgoing_init is None; incoming_init carries loop_bars=4.
    assert!(script.outgoing_init.is_none());
    let inc_init = script.incoming_init.as_ref().expect("incoming_init must be set");
    assert_eq!(inc_init.loop_bars, Some(4));
}

#[test]
fn loop_bridge_requires_stems_is_true() {
    use dj_engine::recipe::{LoopBridge, Recipe};
    assert!(LoopBridge.requires_stems());
}
