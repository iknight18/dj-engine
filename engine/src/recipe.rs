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

const RAMP_BARS: usize = 16;
const RAMP_IDENTITY_EPSILON: f64 = 0.005;

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

        let incoming_tempo_step = Automation {
            target: AutomationTarget::Tempo,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: ratio,
            to: ratio,
            curve: Curve::Step,
        };

        let mut incoming_automations = vec![incoming_fader, incoming_tempo_step];
        let mut total = length_frames;

        if (ratio - 1.0).abs() >= RAMP_IDENTITY_EPSILON {
            let ramp_seconds = RAMP_BARS as f64 * bar_seconds;
            let ramp_frames = (ramp_seconds * sr) as u64;
            let _ = ramp_frames;
            // Source-side headroom check: clamp ramp to cue_out remaining.
            let cue_out = ctx.incoming.sidecar.cue_out_seconds.unwrap_or(f64::INFINITY);
            // Source consumed by crossfade post-roll = length_frames / sr * ratio + small overhead.
            // We accept the same approximation as today's mixer.
            let source_after_crossfade = ctx.plan.incoming_crossfade_start_seconds
                + (length_frames as f64 / sr) * ratio;
            let max_ramp_source = (cue_out - source_after_crossfade).max(0.0);
            let max_ramp_output = max_ramp_source / ((ratio + 1.0) / 2.0).max(0.001);
            let effective_seconds = ramp_seconds.min(max_ramp_output);
            if effective_seconds >= bar_seconds {
                let effective_frames = (effective_seconds * sr) as u64;
                incoming_automations.push(Automation {
                    target: AutomationTarget::Tempo,
                    t_start_frames: t1,
                    t_end_frames: t1 + effective_frames,
                    from: ratio,
                    to: 1.0,
                    curve: Curve::Linear,
                });
                total = length_frames + effective_frames;
            }
        }

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader],
            incoming_automations,
            total_duration_frames: total,
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
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let sr = ctx.sample_rate as f64;
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

        let mut incoming_automations = vec![incoming_fader_on, incoming_tempo_step];
        let mut total = 1u64;

        if (ratio - 1.0).abs() >= RAMP_IDENTITY_EPSILON {
            let ramp_seconds = RAMP_BARS as f64 * bar_seconds;
            let cue_out = ctx.incoming.sidecar.cue_out_seconds.unwrap_or(f64::INFINITY);
            let source_after = ctx.plan.incoming_crossfade_start_seconds;
            let max_ramp_source = (cue_out - source_after).max(0.0);
            let max_ramp_output = max_ramp_source / ((ratio + 1.0) / 2.0).max(0.001);
            let effective_seconds = ramp_seconds.min(max_ramp_output);
            if effective_seconds >= bar_seconds {
                let effective_frames = (effective_seconds * sr) as u64;
                incoming_automations.push(Automation {
                    target: AutomationTarget::Tempo,
                    t_start_frames: t0 + 1,
                    t_end_frames: t0 + 1 + effective_frames,
                    from: ratio,
                    to: 1.0,
                    curve: Curve::Linear,
                });
                total = 1 + effective_frames;
            }
        }

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader_off],
            incoming_automations,
            total_duration_frames: total,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecipeChoice {
    EqualPower,
    HardCut,
}

pub fn pick_recipe(plan: &TransitionPlan) -> RecipeChoice {
    if plan.length_bars == 0 {
        RecipeChoice::HardCut
    } else {
        RecipeChoice::EqualPower
    }
}
