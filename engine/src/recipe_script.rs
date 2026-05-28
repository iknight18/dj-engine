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
    TempoAlign {
        deck: DeckRole,
        ratio: f64,
    },
}
