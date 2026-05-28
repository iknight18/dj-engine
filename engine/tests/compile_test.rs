use dj_engine::automation::{AutomationTarget, Curve};
use dj_engine::library::LoadedTrack;
use dj_engine::recipe::{DeckId, TransitionContext};
use dj_engine::recipe_script::{
    bar, bars, compile, BarPos, BarSpan, DeckRole, EqBand, Phase, TransitionScript,
};
use dj_engine::sidecar::Sidecar;
use dj_engine::transition::TransitionPlan;
use std::path::PathBuf;

const SR: u32 = 44100;

fn sidecar_120() -> Sidecar {
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
        sidecar: sidecar_120(),
    }
}

fn make_ctx<'a>(
    outgoing: &'a LoadedTrack,
    incoming: &'a LoadedTrack,
    plan: &'a TransitionPlan,
    t0: u64,
) -> TransitionContext<'a> {
    TransitionContext {
        outgoing,
        incoming,
        plan,
        master_clock_frames: t0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    }
}

fn plan_16() -> TransitionPlan {
    TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
    }
}

#[test]
fn bar_to_frame_basic_math() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 1_000_000);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::Fader {
            deck: DeckRole::Outgoing,
            from: 1.0,
            to: 0.0,
            span: bars(0.0, 4.0),
            curve: Curve::Linear,
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.outgoing_automations.len(), 1);
    let a = &out.outgoing_automations[0];
    assert_eq!(a.target, AutomationTarget::Fader);
    assert_eq!(a.t_start_frames, 1_000_000);
    assert_eq!(a.t_end_frames, 1_352_800);
    assert!((a.from - 1.0).abs() < 1e-12);
    assert!((a.to - 0.0).abs() < 1e-12);
}

#[test]
fn eq_step_emits_one_frame_step() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::EqStep {
            deck: DeckRole::Incoming,
            band: EqBand::Low,
            value_db: -40.0,
            at: bar(8.0),
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.incoming_automations.len(), 1);
    let a = &out.incoming_automations[0];
    assert_eq!(a.target, AutomationTarget::EqLow);
    assert_eq!(a.curve, Curve::Step);
    assert_eq!(a.t_end_frames, a.t_start_frames + 1);
    assert!((a.from - (-40.0)).abs() < 1e-12);
    assert!((a.to - (-40.0)).abs() < 1e-12);
    assert_eq!(a.t_start_frames, 8 * 88200);
}

#[test]
fn echo_throw_expands_to_two_step_automations() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::EchoThrow {
            deck: DeckRole::Outgoing,
            at: bar(14.0),
            hold_bars: 1.0,
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.outgoing_automations.len(), 2);
    let on = &out.outgoing_automations[0];
    let off = &out.outgoing_automations[1];
    assert_eq!(on.target, AutomationTarget::EchoMix);
    assert_eq!(on.curve, Curve::Step);
    assert!((on.from - 1.0).abs() < 1e-12);
    assert!((on.to - 1.0).abs() < 1e-12);
    assert_eq!(on.t_start_frames, 14 * 88200);
    assert_eq!(on.t_end_frames, 14 * 88200 + 1);
    assert_eq!(off.target, AutomationTarget::EchoMix);
    assert_eq!(off.curve, Curve::Step);
    assert!((off.from - 0.0).abs() < 1e-12);
    assert!((off.to - 0.0).abs() < 1e-12);
    assert_eq!(off.t_start_frames, 15 * 88200);
    assert_eq!(off.t_end_frames, 15 * 88200 + 1);
}

#[test]
fn loop_gate_expands_to_two_step_automations() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::LoopGate {
            deck: DeckRole::Outgoing,
            on_at: bar(0.0),
            off_at: bar(16.0),
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.outgoing_automations.len(), 2);
    assert_eq!(
        out.outgoing_automations[0].target,
        AutomationTarget::LoopGate
    );
    assert_eq!(out.outgoing_automations[0].curve, Curve::Step);
    assert_eq!(out.outgoing_automations[1].curve, Curve::Step);
    assert_eq!(out.outgoing_automations[1].t_start_frames, 16 * 88200);
}

#[test]
fn tempo_align_emits_one_frame_step_at_t0() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 500_000);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::TempoAlign {
            deck: DeckRole::Incoming,
            ratio: 0.9677,
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.incoming_automations.len(), 1);
    let a = &out.incoming_automations[0];
    assert_eq!(a.target, AutomationTarget::Tempo);
    assert_eq!(a.curve, Curve::Step);
    assert_eq!(a.t_start_frames, 500_000);
    assert_eq!(a.t_end_frames, 500_001);
    assert!((a.from - 0.9677).abs() < 1e-12);
    assert!((a.to - 0.9677).abs() < 1e-12);
}

#[test]
fn fader_span_with_equal_start_end_emits_one_frame_step() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 0,
        phases: vec![
            Phase::Fader {
                deck: DeckRole::Outgoing,
                from: 1.0,
                to: 0.0,
                span: bars(0.0, 0.0),
                curve: Curve::Step,
            },
            Phase::Fader {
                deck: DeckRole::Incoming,
                from: 0.0,
                to: 1.0,
                span: bars(0.0, 0.0),
                curve: Curve::Step,
            },
        ],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.outgoing_automations.len(), 1);
    assert_eq!(out.outgoing_automations[0].t_end_frames, 1);
    assert_eq!(out.outgoing_automations[0].curve, Curve::Step);
    assert_eq!(out.incoming_automations.len(), 1);
    assert_eq!(out.incoming_automations[0].t_end_frames, 1);
}

#[test]
fn out_of_range_barpos_is_clamped() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::EqStep {
            deck: DeckRole::Outgoing,
            band: EqBand::Mid,
            value_db: -10.0,
            at: bar(47.0),
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.outgoing_automations.len(), 1);
    assert_eq!(out.outgoing_automations[0].t_start_frames, 1_411_200);
}

#[test]
fn eq_db_value_is_clamped_to_min_40() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::EqStep {
            deck: DeckRole::Outgoing,
            band: EqBand::Low,
            value_db: -55.0,
            at: bar(2.0),
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.outgoing_automations.len(), 1);
    assert!((out.outgoing_automations[0].from - (-40.0)).abs() < 1e-12);
    assert!((out.outgoing_automations[0].to - (-40.0)).abs() < 1e-12);
}

#[test]
fn inverted_span_is_dropped() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::Fader {
            deck: DeckRole::Outgoing,
            from: 1.0,
            to: 0.0,
            span: BarSpan {
                start: BarPos { bars: 5.0 },
                end: BarPos { bars: 3.0 },
            },
            curve: Curve::Linear,
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.outgoing_automations.len(), 0);
}

#[test]
fn total_duration_matches_length_bars() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.total_duration_frames, 1_411_200);
}

#[test]
fn hardcut_zero_length_total_duration_zero() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 0,
        phases: vec![],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.total_duration_frames, 0);
}

#[test]
fn init_configs_pass_through() {
    use dj_engine::recipe::DeckInitConfig;
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let init = DeckInitConfig {
        echo_delay_seconds: Some(0.5),
        echo_feedback: Some(0.75),
        loop_bars: None,
        loop_bar_seconds: None,
    };
    let script = TransitionScript {
        length_bars: 8,
        phases: vec![],
        outgoing_init: Some(init.clone()),
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    let out_init = out
        .outgoing_init
        .expect("outgoing_init should pass through");
    assert_eq!(out_init.echo_delay_seconds, Some(0.5));
    assert_eq!(out_init.echo_feedback, Some(0.75));
    assert!(out.incoming_init.is_none());
}
