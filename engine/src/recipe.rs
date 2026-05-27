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
