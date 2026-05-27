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

pub struct RecipeOutput {
    pub outgoing_automations: Vec<Automation>,
    pub incoming_automations: Vec<Automation>,
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
}

pub fn pick_recipe(plan: &TransitionPlan) -> RecipeChoice {
    if plan.length_bars == 0 {
        RecipeChoice::HardCut
    } else if plan.use_eq_swap {
        RecipeChoice::BassSwap
    } else {
        RecipeChoice::EqualPower
    }
}
