//! Recipe layer: named transition styles compile to lists of Automation primitives.

use crate::automation::Automation;
use crate::library::LoadedTrack;
use crate::transition::TransitionPlan;

pub use crate::deck::DeckId;

pub struct TransitionContext<'a> {
    pub outgoing: &'a LoadedTrack,
    pub incoming: &'a LoadedTrack,
    pub plan: &'a TransitionPlan,
    pub master_clock_frames: u64,
    pub outgoing_deck: DeckId,
    pub incoming_deck: DeckId,
    pub sample_rate: u32,
}

/// Static per-deck effect configuration applied by the scheduler after `cue()`
/// and before `play()`. Fields are `Option` so a recipe can configure only the
/// effects it actually uses (`EchoOut` configures echo, `LoopOut` configures
/// the loop buffer). Existing recipes that don't use effects set both
/// `outgoing_init` and `incoming_init` to `None`.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct DeckInitConfig {
    pub echo_delay_seconds: Option<f64>,
    pub echo_feedback: Option<f64>,
    pub loop_bars: Option<usize>,
    pub loop_bar_seconds: Option<f64>,
}

pub struct RecipeOutput {
    pub outgoing_automations: Vec<Automation>,
    pub incoming_automations: Vec<Automation>,
    pub outgoing_init: Option<DeckInitConfig>,
    pub incoming_init: Option<DeckInitConfig>,
    pub total_duration_frames: u64,
}

pub trait Recipe {
    fn name(&self) -> &'static str;
    fn build_script(&self, ctx: &TransitionContext<'_>) -> crate::recipe_script::TransitionScript;
    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        crate::recipe_script::compile(&self.build_script(ctx), ctx)
    }
}

/// Build a script using the given recipe, inject any automatic post-processing
/// phases (currently: pitch-journey-back), then compile to a `RecipeOutput`.
pub fn build_and_compile(recipe: &dyn Recipe, ctx: &TransitionContext<'_>) -> RecipeOutput {
    let mut script = recipe.build_script(ctx);
    inject_pitch_journey(&mut script, ctx);
    crate::recipe_script::compile(&script, ctx)
}

/// Inject a pitch-journey-back `TempoRamp` into `script` when the pair has a
/// modest BPM shift (0.005 ≤ |ratio − 1.0| < 0.05) and `skip_tempo_align` is
/// false. The ramp runs from `ratio` back to 1.0 over 32 bars after the
/// transition window, so the incoming deck gradually returns to its natural BPM.
///
/// Calling this function multiple times on the same script is idempotent: if a
/// `TempoRamp` for `DeckRole::Incoming` already exists, the function returns
/// immediately without adding another.
pub fn inject_pitch_journey(
    script: &mut crate::recipe_script::TransitionScript,
    ctx: &TransitionContext<'_>,
) {
    use crate::automation::Curve;
    use crate::recipe_script::{bars, DeckRole, Phase};

    if ctx.plan.skip_tempo_align {
        return;
    }

    // Skip if a journey ramp already exists (idempotency).
    let already_injected = script.phases.iter().any(|p| {
        matches!(
            p,
            Phase::TempoRamp {
                deck: DeckRole::Incoming,
                ..
            }
        )
    });
    if already_injected {
        return;
    }

    // Find the TempoAlign on Incoming; its `ratio` is what we journey back from.
    let ratio = script.phases.iter().find_map(|p| match p {
        Phase::TempoAlign {
            deck: DeckRole::Incoming,
            ratio,
        } => Some(*ratio),
        _ => None,
    });
    let Some(ratio) = ratio else {
        return;
    };

    let delta = (ratio - 1.0).abs();
    if !(0.005..0.05).contains(&delta) {
        return;
    }

    let l = script.length_bars as f64;
    script.phases.push(Phase::TempoRamp {
        deck: DeckRole::Incoming,
        from: ratio,
        to: 1.0,
        span: bars(l, l + 32.0),
        curve: Curve::Linear,
    });
}

use crate::automation::Curve;
use crate::recipe_script::{bar, bars, DeckRole, EqBand, Phase, TransitionScript};

pub struct EqualPowerCrossfade;

impl Recipe for EqualPowerCrossfade {
    fn name(&self) -> &'static str {
        "equal_power_crossfade"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = ctx.plan.length_bars as f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

pub struct HardCut;

impl Recipe for HardCut {
    fn name(&self) -> &'static str {
        "hard_cut"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);

        let mut phases = vec![
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
        ];
        if !ctx.plan.skip_tempo_align {
            phases.push(Phase::TempoAlign {
                deck: DeckRole::Incoming,
                ratio,
            });
        }

        TransitionScript {
            length_bars: 0,
            phases,
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

/// `BassSwap`: equal-power crossfade with EqLow automations on both decks that
/// kill the outgoing low end before bringing in the incoming one. Window timing
/// adapts to each track's `band_energy_means.bass`:
///
/// - Both bass-light (< 0.3): falls back to a plain equal-power crossfade.
/// - Both bass-heavy (> 0.5): widens the gap so big basslines clear cleanly.
/// - Otherwise: default window.
///
/// Missing `band_energy_means` is treated as 0.5 (default window).
pub struct BassSwap;

impl Recipe for BassSwap {
    fn name(&self) -> &'static str {
        "bass_swap_crossfade"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = ctx.plan.length_bars as f64;

        let mut phases = vec![
            Phase::Fader {
                deck: DeckRole::Outgoing,
                from: 1.0,
                to: 0.0,
                span: bars(0.0, l),
                curve: Curve::EqualPowerOut,
            },
            Phase::Fader {
                deck: DeckRole::Incoming,
                from: 0.0,
                to: 1.0,
                span: bars(0.0, l),
                curve: Curve::EqualPowerIn,
            },
            Phase::TempoAlign {
                deck: DeckRole::Incoming,
                ratio,
            },
        ];

        let outgoing_bass = ctx
            .outgoing
            .sidecar
            .band_energy_means
            .as_ref()
            .map(|b| b.bass)
            .unwrap_or(0.5);
        let incoming_bass = ctx
            .incoming
            .sidecar
            .band_energy_means
            .as_ref()
            .map(|b| b.bass)
            .unwrap_or(0.5);

        if outgoing_bass < 0.3 && incoming_bass < 0.3 {
            return TransitionScript {
                length_bars: ctx.plan.length_bars,
                phases,
                outgoing_init: None,
                incoming_init: None,
            };
        }

        let (kill_start, kill_end, rel_start, rel_end) =
            if outgoing_bass > 0.5 && incoming_bass > 0.5 {
                (0.30, 0.48, 0.52, 0.70)
            } else {
                (0.35, 0.48, 0.52, 0.65)
            };

        phases.push(Phase::EqStep {
            deck: DeckRole::Incoming,
            band: EqBand::Low,
            value_db: -40.0,
            at: bar(0.0),
        });
        phases.push(Phase::EqRamp {
            deck: DeckRole::Outgoing,
            band: EqBand::Low,
            from_db: 0.0,
            to_db: -40.0,
            span: bars(kill_start * l, kill_end * l),
            curve: Curve::Linear,
        });
        phases.push(Phase::EqRamp {
            deck: DeckRole::Incoming,
            band: EqBand::Low,
            from_db: -40.0,
            to_db: 0.0,
            span: bars(rel_start * l, rel_end * l),
            curve: Curve::Linear,
        });

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases,
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

/// `FilterFade`: incoming track filters in from the top (treble first).
/// outgoing.Fader equal-power fade-out; incoming.Fader equal-power fade-in;
/// incoming.Filter sweeps from +1.0 (full HP, treble only) to 0.0 (bypass)
/// across the full crossfade window. Outgoing has no filter automation.
pub struct FilterFade;

impl Recipe for FilterFade {
    fn name(&self) -> &'static str {
        "filter_fade"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = ctx.plan.length_bars as f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 1.0,
                    to: 1.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::Linear,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

/// `FilterKill`: outgoing filters out to silence (HP sweep) before fading.
/// outgoing.Filter sweeps 0 -> +1 over the first 60% of the crossfade;
/// outgoing.Fader equal-power fade-out over the full crossfade;
/// incoming.Fader equal-power fade-in over the full crossfade (no filter).
pub struct FilterKill;

impl Recipe for FilterKill {
    fn name(&self) -> &'static str {
        "filter_kill"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = ctx.plan.length_bars as f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Filter {
                    deck: DeckRole::Outgoing,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, 0.6 * l),
                    curve: Curve::Linear,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

/// `EchoOut`: outgoing fades out under an echo wash. While the equal-power
/// crossfade does its thing, the outgoing deck's echo wet-mix ramps from 0 to
/// 1 over the first half of the window, leaving the echo tail audible as the
/// dry signal closes. Echo is configured for a one-beat delay with moderate
/// feedback (0.5).
pub struct EchoOut;

impl Recipe for EchoOut {
    fn name(&self) -> &'static str {
        "echo_out"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let beat_seconds = bar_seconds / 4.0;
        let l = ctx.plan.length_bars as f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::EchoMix {
                    deck: DeckRole::Outgoing,
                    from: 0.0,
                    to: 0.55,
                    span: bars(0.0, l / 2.0),
                    curve: Curve::Linear,
                },
            ],
            outgoing_init: Some(DeckInitConfig {
                echo_delay_seconds: Some(beat_seconds),
                echo_feedback: Some(0.35),
                loop_bars: None,
                loop_bar_seconds: None,
            }),
            incoming_init: None,
        }
    }
}

/// `LoopOut`: outgoing locks its last few bars into a loop and rides that loop
/// to the end while the incoming track equal-power fades in over the top. A
/// LoopGate Step pins the loop at t0; another Step releases it at t1 so the
/// deck unloads cleanly on the post-transition swap. Loop length is 4 bars at
/// the outgoing track's tempo.
pub struct LoopOut;

impl Recipe for LoopOut {
    fn name(&self) -> &'static str {
        "loop_out"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let l = ctx.plan.length_bars as f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::LoopGate {
                    deck: DeckRole::Outgoing,
                    on_at: bar(0.0),
                    off_at: bar(l),
                },
            ],
            outgoing_init: Some(DeckInitConfig {
                echo_delay_seconds: None,
                echo_feedback: None,
                loop_bars: Some(4),
                loop_bar_seconds: Some(bar_seconds),
            }),
            incoming_init: None,
        }
    }
}

/// `ThreeBandSwap`: staged EQ swap on integer bar positions.
/// - Incoming highs ramp in over bars [0, L/8].
/// - Mids cross in the middle quarter (bars [3L/8, 5L/8]).
/// - Bass swaps on a single downbeat at bar 7L/8.
///
/// Equal-power fader across the whole window. For L=16 the bass swap
/// lands on bar 14; for L=32 on bar 28; for L=8 on bar 7.
pub struct ThreeBandSwap;

impl Recipe for ThreeBandSwap {
    fn name(&self) -> &'static str {
        "three_band_swap"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = ctx.plan.length_bars as f64;
        let swap_at = 7.0 * l / 8.0;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::High,
                    value_db: -40.0,
                    at: bar(0.0),
                },
                Phase::EqRamp {
                    deck: DeckRole::Incoming,
                    band: EqBand::High,
                    from_db: -40.0,
                    to_db: 0.0,
                    span: bars(0.0, l / 8.0),
                    curve: Curve::Linear,
                },
                Phase::EqRamp {
                    deck: DeckRole::Outgoing,
                    band: EqBand::Mid,
                    from_db: 0.0,
                    to_db: -40.0,
                    span: bars(3.0 * l / 8.0, 5.0 * l / 8.0),
                    curve: Curve::Linear,
                },
                Phase::EqRamp {
                    deck: DeckRole::Incoming,
                    band: EqBand::Mid,
                    from_db: -40.0,
                    to_db: 0.0,
                    span: bars(3.0 * l / 8.0, 5.0 * l / 8.0),
                    curve: Curve::Linear,
                },
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::Low,
                    value_db: -40.0,
                    at: bar(0.0),
                },
                Phase::EqStep {
                    deck: DeckRole::Outgoing,
                    band: EqBand::Low,
                    value_db: -40.0,
                    at: bar(swap_at),
                },
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::Low,
                    value_db: 0.0,
                    at: bar(swap_at),
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

/// `EchoThrow`: equal-power crossfade with a 1-bar one-shot echo flourish
/// on the outgoing deck at bar L-2. Echo delay is 1 beat at the outgoing
/// tempo; feedback is 0.75. The throw fires when the outgoing fader is
/// already near -6 dB, so the tail rings into the incoming.
pub struct EchoThrow;

impl Recipe for EchoThrow {
    fn name(&self) -> &'static str {
        "echo_throw"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let beat_seconds = 60.0 / outgoing_bpm;
        let l = ctx.plan.length_bars as f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                // Triangle wet envelope over the last 2 bars: quick swell (0.25 bar) to 0.6,
                // then linear decay (0.75 bar) back to 0.
                Phase::EchoMix {
                    deck: DeckRole::Outgoing,
                    from: 0.0,
                    to: 0.6,
                    span: bars((l - 2.0).max(0.0), (l - 1.75).max(0.0)),
                    curve: Curve::Linear,
                },
                Phase::EchoMix {
                    deck: DeckRole::Outgoing,
                    from: 0.6,
                    to: 0.0,
                    span: bars((l - 1.75).max(0.0), (l - 1.0).max(0.0)),
                    curve: Curve::Linear,
                },
            ],
            outgoing_init: Some(DeckInitConfig {
                echo_delay_seconds: Some(beat_seconds),
                echo_feedback: Some(0.5),
                loop_bars: None,
                loop_bar_seconds: None,
            }),
            incoming_init: None,
        }
    }
}

/// `HighsFirst`: incoming intro with bass and mids killed at start.
/// - Incoming fader rises fast (equal-power over bars [0, L/8]).
/// - Mids fade in over bars [L/4, L/2].
/// - Synchronized bass swap on bar 3L/4 (outgoing low → -40dB, incoming low → 0dB).
///
/// Used when incoming is markedly higher energy than outgoing.
pub struct HighsFirst;

impl Recipe for HighsFirst {
    fn name(&self) -> &'static str {
        "highs_first"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = ctx.plan.length_bars as f64;
        let swap_at = 3.0 * l / 4.0;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l / 8.0),
                    curve: Curve::EqualPowerIn,
                },
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::Low,
                    value_db: -40.0,
                    at: bar(0.0),
                },
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::Mid,
                    value_db: -40.0,
                    at: bar(0.0),
                },
                Phase::EqRamp {
                    deck: DeckRole::Incoming,
                    band: EqBand::Mid,
                    from_db: -40.0,
                    to_db: 0.0,
                    span: bars(l / 4.0, l / 2.0),
                    curve: Curve::Linear,
                },
                Phase::EqStep {
                    deck: DeckRole::Outgoing,
                    band: EqBand::Low,
                    value_db: -40.0,
                    at: bar(swap_at),
                },
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::Low,
                    value_db: 0.0,
                    at: bar(swap_at),
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

/// `Drop`: incoming plays for 4 bars under a high-pass filter (treble-only
/// build-up), then hard-cuts to full bandwidth on the next downbeat while
/// outgoing fades to silence. The HP-filter sweeps from 0.85 → 0.55 over the
/// build, giving the listener a "riser" — gradually revealing the mid-band —
/// followed by a hard release. Outgoing plays untouched until bar 4.
pub struct Drop;

impl Recipe for Drop {
    fn name(&self) -> &'static str {
        "drop"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = 4.0;

        TransitionScript {
            length_bars: 4,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(l, l),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.7,
                    to: 0.7,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.7,
                    to: 1.0,
                    span: bars(l, l),
                    curve: Curve::Step,
                },
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 0.85,
                    to: 0.85,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 0.85,
                    to: 0.55,
                    span: bars(0.0, l),
                    curve: Curve::Linear,
                },
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 0.0,
                    span: bars(l, l),
                    curve: Curve::Step,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

/// `BreakdownBridge`: outgoing's mids + bass are killed over the first 3 bars
/// (the breakdown — only outgoing's highs ring through), then at bar 4 the
/// outgoing hard-cuts to silence and the incoming hard-cuts in at full volume.
/// The contrast between thin-outgoing and full-incoming is the gesture.
pub struct BreakdownBridge;

impl Recipe for BreakdownBridge {
    fn name(&self) -> &'static str {
        "breakdown_bridge"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = 4.0;

        TransitionScript {
            length_bars: 4,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::EqRamp {
                    deck: DeckRole::Outgoing,
                    band: EqBand::Low,
                    from_db: 0.0,
                    to_db: -40.0,
                    span: bars(0.0, 3.0),
                    curve: Curve::Linear,
                },
                Phase::EqRamp {
                    deck: DeckRole::Outgoing,
                    band: EqBand::Mid,
                    from_db: 0.0,
                    to_db: -40.0,
                    span: bars(0.0, 3.0),
                    curve: Curve::Linear,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(l, l),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 0.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(l, l),
                    curve: Curve::Step,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

/// `DoubleDrop`: outgoing fades to silence over half a bar; both decks held
/// silent for half a bar (anticipation gap); both hard-cut to full volume on
/// the next downbeat; both play together for 1 bar; outgoing hard-cuts out
/// on bar 2 leaving incoming as the new master.
pub struct DoubleDrop;

impl Recipe for DoubleDrop {
    fn name(&self) -> &'static str {
        "double_drop"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = 2.0;

        TransitionScript {
            length_bars: 2,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, 0.5),
                    curve: Curve::Linear,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 0.0,
                    to: 1.0,
                    span: bars(1.0, 1.0),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(l, l),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 0.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(1.0, 1.0),
                    curve: Curve::Step,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecipeChoice {
    EqualPower,
    HardCut,
    BassSwap,
    FilterFade,
    FilterKill,
    EchoOut,
    LoopOut,
    ThreeBandSwap,
    EchoThrow,
    HighsFirst,
    Drop,
    BreakdownBridge,
    DoubleDrop,
}

/// Legacy plan-only picker. Kept for tests and as a reference baseline.
pub fn pick_recipe_plan_only(plan: &TransitionPlan) -> RecipeChoice {
    if plan.length_bars == 0 {
        RecipeChoice::HardCut
    } else if plan.use_eq_swap {
        RecipeChoice::BassSwap
    } else {
        RecipeChoice::EqualPower
    }
}

/// Like `pick_recipe`, but also returns the score for every candidate (after
/// recency penalty and jitter). For logging at transition-arming time.
pub fn pick_recipe_with_scores(
    plan: &TransitionPlan,
    outgoing: &LoadedTrack,
    incoming: &LoadedTrack,
    recency: &crate::recipe_recency::RecipeRecency,
) -> (RecipeChoice, Vec<(RecipeChoice, f64)>) {
    if plan.length_bars == 0 {
        return (RecipeChoice::HardCut, vec![(RecipeChoice::HardCut, 1.0)]);
    }
    let inputs = PairScoreInputs::from_pair(plan, outgoing, incoming);
    let candidates = [
        RecipeChoice::EqualPower,
        RecipeChoice::BassSwap,
        RecipeChoice::FilterFade,
        RecipeChoice::FilterKill,
        RecipeChoice::EchoOut,
        RecipeChoice::LoopOut,
        RecipeChoice::ThreeBandSwap,
        RecipeChoice::EchoThrow,
        RecipeChoice::HighsFirst,
        RecipeChoice::Drop,
        RecipeChoice::BreakdownBridge,
        RecipeChoice::DoubleDrop,
    ];
    let jitter_range = 0.04;
    let mut scored: Vec<(RecipeChoice, f64)> = Vec::with_capacity(candidates.len());
    let mut best = (RecipeChoice::EqualPower, f64::MIN);
    for (idx, choice) in candidates.iter().enumerate() {
        let mut score = score_recipe(*choice, &inputs);
        if recency.contains(*choice) {
            score *= 0.5;
        }
        let salt = (idx as u64)
            .wrapping_mul(2654435761)
            .wrapping_add(recency_hash(recency));
        let jitter = (salt % 1000) as f64 / 1000.0 * jitter_range;
        score += jitter;
        scored.push((*choice, score));
        if score > best.1 {
            best = (*choice, score);
        }
    }
    (best.0, scored)
}

/// Scored picker: ranks all 7 recipes against the pair's properties and a
/// recency penalty. Hard-cut shortcuts when `plan.length_bars == 0`.
pub fn pick_recipe(
    plan: &TransitionPlan,
    outgoing: &LoadedTrack,
    incoming: &LoadedTrack,
    recency: &crate::recipe_recency::RecipeRecency,
) -> RecipeChoice {
    if plan.length_bars == 0 {
        return RecipeChoice::HardCut;
    }
    let inputs = PairScoreInputs::from_pair(plan, outgoing, incoming);
    let candidates = [
        RecipeChoice::EqualPower,
        RecipeChoice::BassSwap,
        RecipeChoice::FilterFade,
        RecipeChoice::FilterKill,
        RecipeChoice::EchoOut,
        RecipeChoice::LoopOut,
        RecipeChoice::ThreeBandSwap,
        RecipeChoice::EchoThrow,
        RecipeChoice::HighsFirst,
        RecipeChoice::Drop,
        RecipeChoice::BreakdownBridge,
        RecipeChoice::DoubleDrop,
    ];

    let mut best = (RecipeChoice::EqualPower, f64::MIN);
    let jitter_range = 0.04;
    for (idx, choice) in candidates.iter().enumerate() {
        let mut score = score_recipe(*choice, &inputs);
        if recency.contains(*choice) {
            score *= 0.5;
        }
        // Deterministic jitter that varies per call: hash the recency state
        // mixed with the candidate index. Stable for tests with a fixed
        // recency state.
        let salt = (idx as u64)
            .wrapping_mul(2654435761)
            .wrapping_add(recency_hash(recency));
        let jitter = (salt % 1000) as f64 / 1000.0 * jitter_range;
        score += jitter;
        if score > best.1 {
            best = (*choice, score);
        }
    }
    best.0
}

fn recipe_choice_id(c: RecipeChoice) -> u64 {
    // Stable, ordered id used in the deterministic-jitter mix below.
    match c {
        RecipeChoice::EqualPower => 1,
        RecipeChoice::HardCut => 2,
        RecipeChoice::BassSwap => 3,
        RecipeChoice::FilterFade => 4,
        RecipeChoice::FilterKill => 5,
        RecipeChoice::EchoOut => 6,
        RecipeChoice::LoopOut => 7,
        RecipeChoice::ThreeBandSwap => 8,
        RecipeChoice::EchoThrow => 9,
        RecipeChoice::HighsFirst => 10,
        RecipeChoice::Drop => 11,
        RecipeChoice::BreakdownBridge => 12,
        RecipeChoice::DoubleDrop => 13,
    }
}

fn recency_hash(recency: &crate::recipe_recency::RecipeRecency) -> u64 {
    // Fold the recency's current contents into a u64. Since RecipeRecency
    // doesn't expose iteration, mix in the "contains" result for each choice.
    let mut h: u64 = 0x9e3779b97f4a7c15;
    for c in [
        RecipeChoice::EqualPower,
        RecipeChoice::HardCut,
        RecipeChoice::BassSwap,
        RecipeChoice::FilterFade,
        RecipeChoice::FilterKill,
        RecipeChoice::EchoOut,
        RecipeChoice::LoopOut,
        RecipeChoice::ThreeBandSwap,
        RecipeChoice::EchoThrow,
        RecipeChoice::HighsFirst,
        RecipeChoice::Drop,
        RecipeChoice::BreakdownBridge,
        RecipeChoice::DoubleDrop,
    ] {
        let bit = if recency.contains(c) { 1 } else { 0 };
        h = h
            .wrapping_mul(0x100000001b3)
            .wrapping_add(recipe_choice_id(c) * 31 + bit);
    }
    h
}

/// Pre-computed inputs the recipe scorer needs. Factoring out the sidecar
/// dereference makes the scorer trivially unit-testable.
#[derive(Clone, Debug)]
pub struct PairScoreInputs {
    pub length_bars: usize,
    pub use_eq_swap: bool,
    /// `incoming.energy_mean - outgoing.energy_mean` (positive = ramp up).
    pub energy_delta: f64,
    /// `band_energy_means.bass` from each sidecar; defaults to 0.5 when absent.
    pub outgoing_bass: f64,
    pub incoming_bass: f64,
    /// True iff BOTH tracks are marked `is_phrase_aligned`.
    pub phrase_aligned: bool,
}

impl PairScoreInputs {
    pub fn from_pair(
        plan: &TransitionPlan,
        outgoing: &LoadedTrack,
        incoming: &LoadedTrack,
    ) -> Self {
        let outgoing_energy = outgoing.sidecar.energy_mean.unwrap_or(0.5);
        let incoming_energy = incoming.sidecar.energy_mean.unwrap_or(0.5);
        let outgoing_bass = outgoing
            .sidecar
            .band_energy_means
            .as_ref()
            .map(|b| b.bass)
            .unwrap_or(0.5);
        let incoming_bass = incoming
            .sidecar
            .band_energy_means
            .as_ref()
            .map(|b| b.bass)
            .unwrap_or(0.5);
        let phrase_aligned = outgoing.sidecar.phrase_aligned() && incoming.sidecar.phrase_aligned();
        Self {
            length_bars: plan.length_bars,
            use_eq_swap: plan.use_eq_swap,
            energy_delta: incoming_energy - outgoing_energy,
            outgoing_bass,
            incoming_bass,
            phrase_aligned,
        }
    }
}

/// Score a recipe's fit for the pair. Higher = better. Score range [0, 1].
pub fn score_recipe(choice: RecipeChoice, inputs: &PairScoreInputs) -> f64 {
    match choice {
        RecipeChoice::HardCut => {
            if inputs.length_bars == 0 {
                1.0
            } else {
                0.0
            }
        }
        RecipeChoice::EqualPower => 0.5,
        RecipeChoice::BassSwap => {
            let both_heavy = inputs.outgoing_bass > 0.5 && inputs.incoming_bass > 0.5;
            let any_heavy = inputs.outgoing_bass > 0.5 || inputs.incoming_bass > 0.5;
            if inputs.phrase_aligned && both_heavy {
                0.85
            } else if inputs.phrase_aligned && any_heavy {
                0.45
            } else {
                0.10
            }
        }
        RecipeChoice::FilterFade => {
            if inputs.energy_delta > 0.20 {
                0.75
            } else if inputs.energy_delta > 0.05 {
                0.40
            } else {
                0.0
            }
        }
        RecipeChoice::FilterKill => {
            if inputs.energy_delta < -0.20 {
                0.70
            } else if inputs.energy_delta < -0.05 {
                0.40
            } else {
                0.0
            }
        }
        RecipeChoice::EchoOut => {
            if inputs.energy_delta < -0.15 {
                0.65
            } else {
                0.35
            }
        }
        RecipeChoice::LoopOut => 0.30,
        RecipeChoice::ThreeBandSwap => {
            let both_heavy = inputs.outgoing_bass > 0.5 && inputs.incoming_bass > 0.5;
            let any_heavy = inputs.outgoing_bass > 0.5 || inputs.incoming_bass > 0.5;
            let long_enough = inputs.length_bars >= 16;
            if inputs.phrase_aligned && both_heavy && long_enough {
                0.90
            } else if inputs.phrase_aligned && both_heavy {
                0.70
            } else if inputs.phrase_aligned && any_heavy {
                0.50
            } else {
                0.20
            }
        }
        RecipeChoice::EchoThrow => {
            // L >= 16 only: on shorter windows the echo line accumulates ~13s of
            // feedback-laden material that dumps audibly when the wet gate opens.
            let smooth = inputs.energy_delta.abs() < 0.10 && inputs.length_bars >= 16;
            let mild = inputs.energy_delta.abs() < 0.15 && inputs.length_bars >= 16;
            if smooth {
                0.65
            } else if mild {
                0.55
            } else {
                0.20
            }
        }
        RecipeChoice::HighsFirst => {
            let big_ramp =
                inputs.energy_delta > 0.20 && inputs.phrase_aligned && inputs.length_bars >= 16;
            let med_ramp = inputs.energy_delta > 0.10 && inputs.phrase_aligned;
            let any_ramp = inputs.energy_delta > 0.05;
            if big_ramp {
                0.85
            } else if med_ramp {
                0.55
            } else if any_ramp {
                0.30
            } else {
                0.10
            }
        }
        RecipeChoice::Drop => {
            let big_ramp = inputs.energy_delta > 0.20 && inputs.phrase_aligned;
            let modest_ramp = inputs.energy_delta > 0.10 && inputs.phrase_aligned;
            if big_ramp {
                0.75
            } else if modest_ramp {
                0.40
            } else {
                0.10
            }
        }
        RecipeChoice::BreakdownBridge => {
            let big_winddown = inputs.energy_delta < -0.20 && inputs.phrase_aligned;
            let modest_winddown = inputs.energy_delta < -0.10 && inputs.phrase_aligned;
            if big_winddown {
                0.70
            } else if modest_winddown {
                0.40
            } else {
                0.10
            }
        }
        RecipeChoice::DoubleDrop => {
            let jackpot = inputs.length_bars >= 16
                && inputs.energy_delta.abs() < 0.05
                && inputs.phrase_aligned;
            if jackpot {
                0.80
            } else {
                0.05
            }
        }
    }
}
