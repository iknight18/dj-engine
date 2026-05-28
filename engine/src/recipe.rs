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
#[derive(Clone, Debug, Default)]
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
    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput;
}

use crate::automation::{AutomationTarget, Curve};

pub struct EqualPowerCrossfade;

impl Recipe for EqualPowerCrossfade {
    fn name(&self) -> &'static str {
        "equal_power_crossfade"
    }

    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let sr = ctx.sample_rate as f64;

        let length_frames = (ctx.plan.length_bars as f64 * bar_seconds * sr) as u64;
        let t0 = ctx.master_clock_frames;
        let t1 = t0 + length_frames;

        let outgoing_fader = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t1,
            from: 1.0,
            to: 0.0,
            curve: Curve::EqualPowerOut,
        };

        let incoming_fader = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t1,
            from: 0.0,
            to: 1.0,
            curve: Curve::EqualPowerIn,
        };

        // Tempo Step is a no-op confirmation (cue() already set state.tempo = ratio).
        // Kept for clarity; sub-projects 2+ may use it to express explicit tempo
        // changes mid-set.
        let incoming_tempo_step = Automation {
            target: AutomationTarget::Tempo,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: ratio,
            to: ratio,
            curve: Curve::Step,
        };

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader],
            incoming_automations: vec![incoming_fader, incoming_tempo_step],
            outgoing_init: None,
            incoming_init: None,
            total_duration_frames: length_frames,
        }
    }
}

pub struct HardCut;

impl Recipe for HardCut {
    fn name(&self) -> &'static str {
        "hard_cut"
    }

    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let t0 = ctx.master_clock_frames;

        let outgoing_fader_off = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: 1.0,
            to: 0.0,
            curve: Curve::Step,
        };
        let incoming_fader_on = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: 0.0,
            to: 1.0,
            curve: Curve::Step,
        };
        let incoming_tempo_step = Automation {
            target: AutomationTarget::Tempo,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: ratio,
            to: ratio,
            curve: Curve::Step,
        };

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader_off],
            incoming_automations: vec![incoming_fader_on, incoming_tempo_step],
            outgoing_init: None,
            incoming_init: None,
            total_duration_frames: 1,
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

    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let sr = ctx.sample_rate as f64;
        let length_frames = (ctx.plan.length_bars as f64 * bar_seconds * sr) as u64;
        let t0 = ctx.master_clock_frames;
        let t1 = t0 + length_frames;

        let outgoing_fader = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t1,
            from: 1.0,
            to: 0.0,
            curve: Curve::EqualPowerOut,
        };
        let incoming_fader = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t1,
            from: 0.0,
            to: 1.0,
            curve: Curve::EqualPowerIn,
        };
        let incoming_tempo_step = Automation {
            target: AutomationTarget::Tempo,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: ratio,
            to: ratio,
            curve: Curve::Step,
        };

        // Bass-content adaptation.
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

        // Both bass-light → just an equal-power crossfade, no EQ shenanigans.
        if outgoing_bass < 0.3 && incoming_bass < 0.3 {
            return RecipeOutput {
                outgoing_automations: vec![outgoing_fader],
                incoming_automations: vec![incoming_fader, incoming_tempo_step],
                outgoing_init: None,
                incoming_init: None,
                total_duration_frames: length_frames,
            };
        }

        // Both bass-heavy → wider gap so big basslines clear before the new one enters.
        let (kill_start, kill_end, rel_start, rel_end) =
            if outgoing_bass > 0.5 && incoming_bass > 0.5 {
                (0.30, 0.48, 0.52, 0.70)
            } else {
                (0.35, 0.48, 0.52, 0.65)
            };

        let l = length_frames as f64;
        let outgoing_eq_low_kill = Automation {
            target: AutomationTarget::EqLow,
            t_start_frames: t0 + (kill_start * l) as u64,
            t_end_frames: t0 + (kill_end * l) as u64,
            from: 0.0,
            to: -40.0,
            curve: Curve::Linear,
        };
        let incoming_eq_low_init = Automation {
            target: AutomationTarget::EqLow,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: -40.0,
            to: -40.0,
            curve: Curve::Step,
        };
        let incoming_eq_low_release = Automation {
            target: AutomationTarget::EqLow,
            t_start_frames: t0 + (rel_start * l) as u64,
            t_end_frames: t0 + (rel_end * l) as u64,
            from: -40.0,
            to: 0.0,
            curve: Curve::Linear,
        };

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader, outgoing_eq_low_kill],
            incoming_automations: vec![
                incoming_fader,
                incoming_tempo_step,
                incoming_eq_low_init,
                incoming_eq_low_release,
            ],
            outgoing_init: None,
            incoming_init: None,
            total_duration_frames: length_frames,
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

    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let sr = ctx.sample_rate as f64;
        let length_frames = (ctx.plan.length_bars as f64 * bar_seconds * sr) as u64;
        let t0 = ctx.master_clock_frames;
        let t1 = t0 + length_frames;

        let outgoing_fader = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t1,
            from: 1.0,
            to: 0.0,
            curve: Curve::EqualPowerOut,
        };
        let incoming_fader = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t1,
            from: 0.0,
            to: 1.0,
            curve: Curve::EqualPowerIn,
        };
        let incoming_tempo_step = Automation {
            target: AutomationTarget::Tempo,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: ratio,
            to: ratio,
            curve: Curve::Step,
        };
        let incoming_filter_init = Automation {
            target: AutomationTarget::Filter,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: 1.0,
            to: 1.0,
            curve: Curve::Step,
        };
        let incoming_filter_sweep = Automation {
            target: AutomationTarget::Filter,
            t_start_frames: t0,
            t_end_frames: t1,
            from: 1.0,
            to: 0.0,
            curve: Curve::Linear,
        };

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader],
            incoming_automations: vec![
                incoming_fader,
                incoming_tempo_step,
                incoming_filter_init,
                incoming_filter_sweep,
            ],
            outgoing_init: None,
            incoming_init: None,
            total_duration_frames: length_frames,
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

    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let sr = ctx.sample_rate as f64;
        let length_frames = (ctx.plan.length_bars as f64 * bar_seconds * sr) as u64;
        let t0 = ctx.master_clock_frames;
        let t1 = t0 + length_frames;
        let filter_end = t0 + ((length_frames as f64) * 0.6) as u64;

        let outgoing_fader = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t1,
            from: 1.0,
            to: 0.0,
            curve: Curve::EqualPowerOut,
        };
        let outgoing_filter_sweep = Automation {
            target: AutomationTarget::Filter,
            t_start_frames: t0,
            t_end_frames: filter_end,
            from: 0.0,
            to: 1.0,
            curve: Curve::Linear,
        };
        let incoming_fader = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t1,
            from: 0.0,
            to: 1.0,
            curve: Curve::EqualPowerIn,
        };
        let incoming_tempo_step = Automation {
            target: AutomationTarget::Tempo,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: ratio,
            to: ratio,
            curve: Curve::Step,
        };

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader, outgoing_filter_sweep],
            incoming_automations: vec![incoming_fader, incoming_tempo_step],
            outgoing_init: None,
            incoming_init: None,
            total_duration_frames: length_frames,
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

    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let beat_seconds = bar_seconds / 4.0;
        let sr = ctx.sample_rate as f64;
        let length_frames = (ctx.plan.length_bars as f64 * bar_seconds * sr) as u64;
        let t0 = ctx.master_clock_frames;
        let t1 = t0 + length_frames;

        let outgoing_fader = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t1,
            from: 1.0,
            to: 0.0,
            curve: Curve::EqualPowerOut,
        };
        let incoming_fader = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t1,
            from: 0.0,
            to: 1.0,
            curve: Curve::EqualPowerIn,
        };
        let incoming_tempo_step = Automation {
            target: AutomationTarget::Tempo,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: ratio,
            to: ratio,
            curve: Curve::Step,
        };
        let outgoing_echo_ramp = Automation {
            target: AutomationTarget::EchoMix,
            t_start_frames: t0,
            t_end_frames: t0 + length_frames / 2,
            from: 0.0,
            to: 1.0,
            curve: Curve::Linear,
        };

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader, outgoing_echo_ramp],
            incoming_automations: vec![incoming_fader, incoming_tempo_step],
            outgoing_init: Some(DeckInitConfig {
                echo_delay_seconds: Some(beat_seconds),
                echo_feedback: Some(0.5),
                loop_bars: None,
                loop_bar_seconds: None,
            }),
            incoming_init: None,
            total_duration_frames: length_frames,
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

    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let sr = ctx.sample_rate as f64;
        let length_frames = (ctx.plan.length_bars as f64 * bar_seconds * sr) as u64;
        let t0 = ctx.master_clock_frames;
        let t1 = t0 + length_frames;

        let outgoing_fader = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t1,
            from: 1.0,
            to: 0.0,
            curve: Curve::EqualPowerOut,
        };
        let incoming_fader = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t1,
            from: 0.0,
            to: 1.0,
            curve: Curve::EqualPowerIn,
        };
        let incoming_tempo_step = Automation {
            target: AutomationTarget::Tempo,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: ratio,
            to: ratio,
            curve: Curve::Step,
        };
        let outgoing_loop_on = Automation {
            target: AutomationTarget::LoopGate,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: 1.0,
            to: 1.0,
            curve: Curve::Step,
        };
        let outgoing_loop_off = Automation {
            target: AutomationTarget::LoopGate,
            t_start_frames: t1,
            t_end_frames: t1 + 1,
            from: 0.0,
            to: 0.0,
            curve: Curve::Step,
        };

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader, outgoing_loop_on, outgoing_loop_off],
            incoming_automations: vec![incoming_fader, incoming_tempo_step],
            outgoing_init: Some(DeckInitConfig {
                echo_delay_seconds: None,
                echo_feedback: None,
                loop_bars: Some(4),
                loop_bar_seconds: Some(bar_seconds),
            }),
            incoming_init: None,
            total_duration_frames: length_frames,
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
    }
}
