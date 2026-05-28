//! Phase-composition DSL for transitions.
//!
//! A `TransitionScript` is a declarative description of a transition in musical
//! time (bars/beats). The `compile` function in this module turns a script into
//! a `crate::recipe::RecipeOutput` (frame-stamped automations) that the
//! scheduler consumes.
//!
//! The DSL is serde-derived so a future sub-project can deserialize externally-
//! generated (e.g., AI-generated) scripts and pipe them through `compile`
//! without scheduler changes.

use serde::{Deserialize, Serialize};

use crate::automation::Curve;
use crate::recipe::DeckInitConfig;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransitionScript {
    pub length_bars: usize,
    pub phases: Vec<Phase>,
    pub outgoing_init: Option<DeckInitConfig>,
    pub incoming_init: Option<DeckInitConfig>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeckRole {
    Outgoing,
    Incoming,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EqBand {
    Low,
    Mid,
    High,
}

/// Fractional bars from the transition window's start (bar 0).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BarPos {
    pub bars: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BarSpan {
    pub start: BarPos,
    pub end: BarPos,
}

/// Constructor helpers used by recipe `build_script` methods.
pub fn bar(n: f64) -> BarPos {
    BarPos { bars: n }
}

pub fn bars(start: f64, end: f64) -> BarSpan {
    BarSpan {
        start: BarPos { bars: start },
        end: BarPos { bars: end },
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Phase {
    Fader {
        deck: DeckRole,
        from: f64,
        to: f64,
        span: BarSpan,
        curve: Curve,
    },
    EqRamp {
        deck: DeckRole,
        band: EqBand,
        from_db: f64,
        to_db: f64,
        span: BarSpan,
        curve: Curve,
    },
    EqStep {
        deck: DeckRole,
        band: EqBand,
        value_db: f64,
        at: BarPos,
    },
    Filter {
        deck: DeckRole,
        from: f64,
        to: f64,
        span: BarSpan,
        curve: Curve,
    },
    EchoMix {
        deck: DeckRole,
        from: f64,
        to: f64,
        span: BarSpan,
        curve: Curve,
    },
    /// Set EchoMix to 1.0 at `at`, then back to 0.0 at `at + hold_bars`.
    EchoThrow {
        deck: DeckRole,
        at: BarPos,
        hold_bars: f64,
    },
    /// LoopGate on at `on_at`, off at `off_at`.
    LoopGate {
        deck: DeckRole,
        on_at: BarPos,
        off_at: BarPos,
    },
    /// Tempo Step at bar 0. Emitted once per script by `Incoming`.
    TempoAlign { deck: DeckRole, ratio: f64 },
}

use crate::automation::{Automation, AutomationTarget};
use crate::recipe::{RecipeOutput, TransitionContext};
use tracing::warn;

/// Compile a `TransitionScript` to a `RecipeOutput` (frame-stamped automations).
///
/// Clamps invalid inputs and logs a warn; never panics. Drops phases with
/// `BarSpan::end < BarSpan::start`. A span with `end == start` is emitted as a
/// 1-frame Step automation (the canonical "step at this BarPos" idiom).
pub fn compile(script: &TransitionScript, ctx: &TransitionContext<'_>) -> RecipeOutput {
    let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0).max(1.0);
    let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
    let frames_per_bar = bar_seconds * (ctx.sample_rate as f64);
    let length_bars_f = script.length_bars as f64;
    let t0 = ctx.master_clock_frames;

    let total_duration_frames = (length_bars_f * frames_per_bar) as u64;

    let bar_to_frame = |b: BarPos| -> u64 {
        let mut bars = b.bars;
        if !bars.is_finite() {
            warn!("compile: non-finite BarPos.bars={}, clamping to 0", bars);
            bars = 0.0;
        }
        if bars < 0.0 {
            warn!("compile: BarPos.bars={} < 0, clamping to 0", bars);
            bars = 0.0;
        }
        if bars > length_bars_f {
            warn!(
                "compile: BarPos.bars={} > length_bars={}, clamping",
                bars, length_bars_f
            );
            bars = length_bars_f;
        }
        t0 + (bars * frames_per_bar) as u64
    };

    let mut outgoing_automations: Vec<Automation> = Vec::new();
    let mut incoming_automations: Vec<Automation> = Vec::new();

    let eq_target = |band: EqBand| match band {
        EqBand::Low => AutomationTarget::EqLow,
        EqBand::Mid => AutomationTarget::EqMid,
        EqBand::High => AutomationTarget::EqHigh,
    };

    let clamp01 = |v: f64| v.clamp(0.0, 1.0);
    let clamp_filter = |v: f64| v.clamp(-1.0, 1.0);
    let clamp_db = |v: f64| v.clamp(-40.0, 6.0);

    macro_rules! push {
        ($deck:expr, $a:expr) => {
            match $deck {
                DeckRole::Outgoing => outgoing_automations.push($a),
                DeckRole::Incoming => incoming_automations.push($a),
            }
        };
    }

    macro_rules! emit_ramp {
        ($deck:expr, $target:expr, $from:expr, $to:expr, $span:expr, $curve:expr) => {{
            let span = $span;
            if !span.end.bars.is_finite() || !span.start.bars.is_finite() {
                warn!("compile: non-finite span, dropping phase");
            } else if span.end.bars < span.start.bars {
                warn!(
                    "compile: span end={} < start={}, dropping phase",
                    span.end.bars, span.start.bars
                );
            } else {
                let t_start = bar_to_frame(span.start);
                let t_end = if (span.end.bars - span.start.bars).abs() < f64::EPSILON {
                    t_start + 1
                } else {
                    let raw = bar_to_frame(span.end);
                    if raw <= t_start {
                        t_start + 1
                    } else {
                        raw
                    }
                };
                push!(
                    $deck,
                    Automation {
                        target: $target,
                        t_start_frames: t_start,
                        t_end_frames: t_end,
                        from: $from,
                        to: $to,
                        curve: $curve,
                    }
                );
            }
        }};
    }

    for phase in &script.phases {
        match *phase {
            Phase::Fader {
                deck,
                from,
                to,
                span,
                curve,
            } => emit_ramp!(
                deck,
                AutomationTarget::Fader,
                clamp01(from),
                clamp01(to),
                span,
                curve
            ),
            Phase::EqRamp {
                deck,
                band,
                from_db,
                to_db,
                span,
                curve,
            } => emit_ramp!(
                deck,
                eq_target(band),
                clamp_db(from_db),
                clamp_db(to_db),
                span,
                curve
            ),
            Phase::EqStep {
                deck,
                band,
                value_db,
                at,
            } => {
                let v = clamp_db(value_db);
                let t = bar_to_frame(at);
                push!(
                    deck,
                    Automation {
                        target: eq_target(band),
                        t_start_frames: t,
                        t_end_frames: t + 1,
                        from: v,
                        to: v,
                        curve: Curve::Step,
                    }
                );
            }
            Phase::Filter {
                deck,
                from,
                to,
                span,
                curve,
            } => emit_ramp!(
                deck,
                AutomationTarget::Filter,
                clamp_filter(from),
                clamp_filter(to),
                span,
                curve
            ),
            Phase::EchoMix {
                deck,
                from,
                to,
                span,
                curve,
            } => emit_ramp!(
                deck,
                AutomationTarget::EchoMix,
                clamp01(from),
                clamp01(to),
                span,
                curve
            ),
            Phase::EchoThrow {
                deck,
                at,
                hold_bars,
            } => {
                let t_on = bar_to_frame(at);
                let t_off = bar_to_frame(BarPos {
                    bars: at.bars + hold_bars.max(0.0),
                });
                push!(
                    deck,
                    Automation {
                        target: AutomationTarget::EchoMix,
                        t_start_frames: t_on,
                        t_end_frames: t_on + 1,
                        from: 1.0,
                        to: 1.0,
                        curve: Curve::Step,
                    }
                );
                push!(
                    deck,
                    Automation {
                        target: AutomationTarget::EchoMix,
                        t_start_frames: t_off,
                        t_end_frames: t_off + 1,
                        from: 0.0,
                        to: 0.0,
                        curve: Curve::Step,
                    }
                );
            }
            Phase::LoopGate {
                deck,
                on_at,
                off_at,
            } => {
                let t_on = bar_to_frame(on_at);
                let t_off = bar_to_frame(off_at);
                push!(
                    deck,
                    Automation {
                        target: AutomationTarget::LoopGate,
                        t_start_frames: t_on,
                        t_end_frames: t_on + 1,
                        from: 1.0,
                        to: 1.0,
                        curve: Curve::Step,
                    }
                );
                push!(
                    deck,
                    Automation {
                        target: AutomationTarget::LoopGate,
                        t_start_frames: t_off,
                        t_end_frames: t_off + 1,
                        from: 0.0,
                        to: 0.0,
                        curve: Curve::Step,
                    }
                );
            }
            Phase::TempoAlign { deck, ratio } => {
                push!(
                    deck,
                    Automation {
                        target: AutomationTarget::Tempo,
                        t_start_frames: t0,
                        t_end_frames: t0 + 1,
                        from: ratio,
                        to: ratio,
                        curve: Curve::Step,
                    }
                );
            }
        }
    }

    RecipeOutput {
        outgoing_automations,
        incoming_automations,
        outgoing_init: script.outgoing_init.clone(),
        incoming_init: script.incoming_init.clone(),
        total_duration_frames,
    }
}
