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

    /// True iff this recipe needs both decks loaded in stems mode (4 separate
    /// audio buffers per deck). Default `false` — existing recipes use the
    /// mixed source. The scheduler reads this and dispatches `cue_stems`
    /// instead of `cue` for incoming, so per-stem fader automations have an
    /// effect.
    fn requires_stems(&self) -> bool {
        false
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

        // Both bass-light → no bass-swap gesture needed; plain equal-power
        // crossfade keeps the recipe usable on tracks with little low end.
        if outgoing_bass < 0.3 && incoming_bass < 0.3 {
            return TransitionScript {
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
                ],
                outgoing_init: None,
                incoming_init: None,
            };
        }

        // Real-DJ bass swap. Both decks playing at full volume for the first
        // half, incoming bass-killed via EqLow. At the midpoint downbeat: a
        // simultaneous Step swap — outgoing bass dies, incoming bass enters,
        // listener feels the gesture in a single beat. Then outgoing fades
        // out equal-power over the second half.
        let kill_bar = (l / 2.0).floor(); // L=8 → 4, L=16 → 8, L=32 → 16 — all downbeats

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                // Incoming Step to full volume at bar 0. Its bass is killed
                // via EqLow Step on the next phase, so listener hears the
                // mid+high content of incoming layered over outgoing's full
                // mix during the first half.
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 1.0,
                    to: 1.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::Low,
                    value_db: -40.0,
                    at: bar(0.0),
                },
                // THE MOMENT: simultaneous Step on the midpoint downbeat.
                Phase::EqStep {
                    deck: DeckRole::Outgoing,
                    band: EqBand::Low,
                    value_db: -40.0,
                    at: bar(kill_bar),
                },
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::Low,
                    value_db: 0.0,
                    at: bar(kill_bar),
                },
                // Outgoing fader: held at 1.0 by default until kill_bar
                // (no automation needed — deck arrives at this transition
                // with fader already at 1.0 from playing as master). Then
                // equal-power-out over the second half.
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(kill_bar, l),
                    curve: Curve::EqualPowerOut,
                },
            ],
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
                // Mids swap on integer downbeats: outgoing mids die at bar L/2,
                // incoming mids enter at bar L/2 + 1. The 1-bar gap creates a
                // brief "thin" moment between the two — a deliberate beat with
                // mostly highs and bass, then incoming mids re-fill the sound.
                Phase::EqStep {
                    deck: DeckRole::Outgoing,
                    band: EqBand::Mid,
                    value_db: -40.0,
                    at: bar(l / 2.0),
                },
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::Mid,
                    value_db: -40.0,
                    at: bar(0.0),
                },
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::Mid,
                    value_db: 0.0,
                    at: bar(l / 2.0 + 1.0),
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
                // Incoming mids enter as a Step on a downbeat (bar L/4) rather
                // than a gradual ramp — listener feels the mid-range fill in
                // as a deliberate "moment" instead of a slow reveal.
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::Mid,
                    value_db: 0.0,
                    at: bar(l / 4.0),
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

/// First index in `density[lo..hi]` where `density[i] >= threshold`. Returns
/// the last index of the slice when no bar passes. Returns `None` for empty
/// input or `lo >= hi`.
/// Returns `Some(i)` for the first bar in `[lo, hi)` whose `density[i] >= threshold`.
/// Returns `None` when no bar in the range passes the threshold (or the range is
/// empty / past the end of the array). Callers handle the no-peak case explicitly —
/// e.g., fall back to plain equal-power behavior rather than snapping to an
/// arbitrary bar at the end of the window.
fn first_bar_at_or_above(density: &[f64], lo: usize, hi: usize, threshold: f64) -> Option<usize> {
    let hi = hi.min(density.len());
    if lo >= hi {
        return None;
    }
    for (i, v) in density.iter().enumerate().take(hi).skip(lo) {
        if *v >= threshold {
            return Some(i);
        }
    }
    None
}

/// `VocalOverlay`: 16-bar layered transition.
/// - Bars [0, 4]: incoming's vocals stem fades in over outgoing's full mix.
///   Incoming non-vocal stems are kept muted; the mixed fader stays at 1.0
///   so the audible signal is purely vocals.
/// - Bars [4, 12]: hold — outgoing plays underneath the incoming vocal.
/// - Bars [12, 16]: incoming's drums/bass/other ramp in linearly while
///   outgoing equal-power-fades out.
///
/// Requires stems on both decks (`requires_stems = true`).
pub struct VocalOverlay;

impl Recipe for VocalOverlay {
    fn name(&self) -> &'static str {
        "vocal_overlay"
    }

    fn requires_stems(&self) -> bool {
        true
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        use crate::deck::StemKind;
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);

        TransitionScript {
            length_bars: 16,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                // Outgoing equal-power fades out over the last 4 bars.
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(12.0, 16.0),
                    curve: Curve::EqualPowerOut,
                },
                // Incoming mixed fader stays at 1.0 from bar 0 — per-stem faders
                // gate everything below.
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 1.0,
                    to: 1.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                // Mute incoming non-vocal stems at bar 0 (Step at t0).
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Drums,
                    from: 0.0,
                    to: 0.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Bass,
                    from: 0.0,
                    to: 0.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Other,
                    from: 0.0,
                    to: 0.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Vocals,
                    from: 0.0,
                    to: 0.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                // Bars [0, 4]: incoming vocals fade in.
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Vocals,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, 4.0),
                    curve: Curve::Linear,
                },
                // Bars [12, 16]: incoming non-vocal stems fade in.
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Drums,
                    from: 0.0,
                    to: 1.0,
                    span: bars(12.0, 16.0),
                    curve: Curve::Linear,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Bass,
                    from: 0.0,
                    to: 1.0,
                    span: bars(12.0, 16.0),
                    curve: Curve::Linear,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Other,
                    from: 0.0,
                    to: 1.0,
                    span: bars(12.0, 16.0),
                    curve: Curve::Linear,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

/// `BassSwapStemAware`: 16-bar equal-power crossfade, with bass-stem kill/release
/// timed to actual bass-density structure from the analyzer's per-bar density
/// arrays. Outgoing's bass stem cuts off at the lowest-bass-density bar in
/// the last quarter of the window; incoming's bass stem comes in at the first
/// bar within the window where its bass_density >= 0.7 (with a fallback to
/// bar 2 if the bass is uniformly low). All other stems crossfade equal-power
/// across the full window on the mixed fader.
pub struct BassSwapStemAware;

impl Recipe for BassSwapStemAware {
    fn name(&self) -> &'static str {
        "bass_swap_stem_aware"
    }

    fn requires_stems(&self) -> bool {
        true
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        use crate::deck::StemKind;
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = 16.0_f64;

        // Synchronized swap: pick a single bar for both outgoing's kill and
        // incoming's release. This is the "real DJ" handoff — bass dies on
        // deck A at the same beat it enters on deck B, no dead-zone gap.
        //
        // Choose the bar by finding incoming's first non-trivial bass-density
        // entry (0.35 catches gradual buildups; 0.7 was too peak-level and
        // produced multi-bar gaps on tracks with quiet intros). If incoming
        // never crosses the threshold within the search window, fall back to
        // bar 8 (mid-window) — a neutral default that still gives both decks
        // half a window of "with bass" and half of "after bass swap".
        let incoming_bar_seconds = (60.0 / incoming_bpm) * 4.0;
        let incoming_intro_bar =
            (ctx.plan.incoming_crossfade_start_seconds / incoming_bar_seconds) as usize;
        let incoming_bass_density = ctx
            .incoming
            .sidecar
            .stems
            .as_ref()
            .map(|s| s.bass_density.clone())
            .unwrap_or_default();

        let incoming_release_bar_track = first_bar_at_or_above(
            &incoming_bass_density,
            incoming_intro_bar,
            incoming_intro_bar + 16,
            0.35,
        );
        let swap_bar_window = incoming_release_bar_track
            .map(|b| (b.saturating_sub(incoming_intro_bar)) as f64)
            .unwrap_or(8.0)
            .clamp(2.0, l - 1.0); // never bar 0 (no time to crossfade) or last (instant cut)

        // outgoing_kill_bar = incoming_release_bar = swap_bar_window. Both
        // step automations fire at the same bar of the transition window.
        let outgoing_kill_bar_window = swap_bar_window;
        let incoming_release_bar_window = swap_bar_window;

        TransitionScript {
            length_bars: 16,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                // Equal-power crossfade on the mixed fader across the full window.
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
                // Incoming bass stem: muted from bar 0 until release.
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Bass,
                    from: 0.0,
                    to: 0.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Bass,
                    from: 1.0,
                    to: 1.0,
                    span: bars(incoming_release_bar_window, incoming_release_bar_window),
                    curve: Curve::Step,
                },
                // Outgoing bass stem: killed at the valley bar.
                Phase::StemFader {
                    deck: DeckRole::Outgoing,
                    stem: StemKind::Bass,
                    from: 0.0,
                    to: 0.0,
                    span: bars(outgoing_kill_bar_window, outgoing_kill_bar_window),
                    curve: Curve::Step,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}

/// `BreakdownIntoDrop`: 8-bar transition timed to outgoing's drum-density
/// valley and incoming's drum-density peak. During the window, outgoing's
/// drum stem progressively drops (the breakdown). At the first incoming-drum-
/// peak bar within the window (drum_density >= 0.85; fallback bar 4),
/// hard-cut: outgoing fader -> 0, incoming fader -> 1.
pub struct BreakdownIntoDrop;

impl Recipe for BreakdownIntoDrop {
    fn name(&self) -> &'static str {
        "breakdown_into_drop"
    }

    fn requires_stems(&self) -> bool {
        true
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        use crate::deck::StemKind;
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = 8.0_f64;

        let incoming_bar_seconds = (60.0 / incoming_bpm) * 4.0;
        let incoming_intro_bar =
            (ctx.plan.incoming_crossfade_start_seconds / incoming_bar_seconds) as usize;
        let incoming_drum_density = ctx
            .incoming
            .sidecar
            .stems
            .as_ref()
            .map(|s| s.drum_density.clone())
            .unwrap_or_default();

        // First incoming-drum-peak bar in [intro, intro + 8]; fallback bar 4.
        let drop_bar_track = first_bar_at_or_above(
            &incoming_drum_density,
            incoming_intro_bar,
            incoming_intro_bar + 8,
            0.85,
        );
        let drop_bar_window = drop_bar_track
            .map(|b| (b.saturating_sub(incoming_intro_bar)) as f64)
            .unwrap_or(4.0)
            .clamp(0.0, l);
        // Breakdown ramp duration: at least 1 bar (avoid degenerate 0-span).
        let breakdown_end = drop_bar_window.max(1.0);

        TransitionScript {
            length_bars: 8,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                // Outgoing's drum stem progressively drops over the breakdown.
                Phase::StemFader {
                    deck: DeckRole::Outgoing,
                    stem: StemKind::Drums,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, breakdown_end),
                    curve: Curve::Linear,
                },
                // Outgoing mixed fader holds at 1.0 then hard-cuts at the drop bar.
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(drop_bar_window, drop_bar_window),
                    curve: Curve::Step,
                },
                // Incoming silent until the drop, then hard-cuts in.
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
                    span: bars(drop_bar_window, drop_bar_window),
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
    VocalOverlay,
    BassSwapStemAware,
    BreakdownIntoDrop,
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
        RecipeChoice::VocalOverlay,
        RecipeChoice::BassSwapStemAware,
        RecipeChoice::BreakdownIntoDrop,
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
        RecipeChoice::VocalOverlay,
        RecipeChoice::BassSwapStemAware,
        RecipeChoice::BreakdownIntoDrop,
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
        RecipeChoice::VocalOverlay => 14,
        RecipeChoice::BassSwapStemAware => 15,
        RecipeChoice::BreakdownIntoDrop => 16,
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
        RecipeChoice::VocalOverlay,
        RecipeChoice::BassSwapStemAware,
        RecipeChoice::BreakdownIntoDrop,
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
    /// True iff BOTH sidecars carry a `stems` block (drum/bass/vocal density,
    /// vocal segments). Stem-aware recipes refuse to fire without this.
    pub stems_available: bool,
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
        let stems_available = outgoing.sidecar.stems.is_some() && incoming.sidecar.stems.is_some();
        Self {
            length_bars: plan.length_bars,
            use_eq_swap: plan.use_eq_swap,
            energy_delta: incoming_energy - outgoing_energy,
            outgoing_bass,
            incoming_bass,
            phrase_aligned,
            stems_available,
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
        RecipeChoice::EqualPower => 0.15,
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
                0.45
            } else {
                0.20
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
        RecipeChoice::VocalOverlay => {
            // The planner only promotes to length_bars >= 16 when bpm_delta
            // <= 0.02, so requiring length_bars >= 16 here proxies the
            // ratio ~= 1.0 gate from the spec.
            let identical_bpm_band = inputs.energy_delta.abs() < 0.20 && inputs.stems_available;
            let strong = identical_bpm_band && inputs.phrase_aligned && inputs.length_bars >= 16;
            if strong {
                0.85
            } else if identical_bpm_band {
                0.30
            } else {
                0.05
            }
        }
        RecipeChoice::BassSwapStemAware => {
            let strong = inputs.stems_available
                && inputs.phrase_aligned
                && inputs.length_bars >= 16
                && (inputs.outgoing_bass > 0.5 || inputs.incoming_bass > 0.5);
            if strong {
                0.92
            } else if inputs.stems_available {
                0.30
            } else {
                0.05
            }
        }
        RecipeChoice::BreakdownIntoDrop => {
            let strong =
                inputs.stems_available && inputs.phrase_aligned && inputs.energy_delta > 0.10;
            if strong {
                0.80
            } else if inputs.stems_available {
                0.20
            } else {
                0.05
            }
        }
    }
}
