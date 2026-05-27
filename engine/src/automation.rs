//! Automation primitive: timed, curved change to one parameter on one deck.

use std::f64::consts::FRAC_PI_2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomationTarget {
    Fader,
    Tempo,
    // sub-project 2 adds EqLow, EqMid, EqHigh; sub-project 3 adds Filter; etc.
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    Linear,
    EqualPowerIn,
    EqualPowerOut,
    Step,
}

impl Curve {
    pub fn eval(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Curve::Linear => t,
            Curve::EqualPowerIn => (t * FRAC_PI_2).sin(),
            Curve::EqualPowerOut => (t * FRAC_PI_2).cos(),
            Curve::Step => {
                if t >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Automation {
    pub target: AutomationTarget,
    pub t_start_frames: u64,
    pub t_end_frames: u64,
    pub from: f64,
    pub to: f64,
    pub curve: Curve,
}

impl Automation {
    /// Evaluate at master-clock frame `cursor`. Returns `Some(value)` only when
    /// `cursor` is inside `[t_start_frames, t_end_frames)`.
    pub fn sample_value(&self, cursor: u64) -> Option<f64> {
        if cursor < self.t_start_frames || cursor >= self.t_end_frames {
            return None;
        }
        let span = (self.t_end_frames - self.t_start_frames).max(1) as f64;
        let t = (cursor - self.t_start_frames) as f64 / span;
        let frac = self.curve.eval(t);
        Some(self.from + (self.to - self.from) * frac)
    }
}

pub struct AutomationQueue {
    pending: Vec<Automation>,
}

impl Default for AutomationQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl AutomationQueue {
    pub fn new() -> Self {
        Self { pending: Vec::new() }
    }

    pub fn schedule(&mut self, a: Automation) {
        self.pending.push(a);
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Apply automations active at `cursor` to `state`. Automations whose
    /// `t_end_frames <= cursor` have their `to` value applied and are removed.
    /// Automations whose `t_start_frames > cursor` are left untouched.
    pub fn step(&mut self, cursor: u64, state: &mut crate::deck::DeckState) {
        let mut remaining = Vec::with_capacity(self.pending.len());
        for a in self.pending.drain(..) {
            if cursor >= a.t_end_frames {
                apply_value(a.target, a.to, state);
            } else if cursor >= a.t_start_frames {
                if let Some(v) = a.sample_value(cursor) {
                    apply_value(a.target, v, state);
                }
                remaining.push(a);
            } else {
                remaining.push(a);
            }
        }
        self.pending = remaining;
    }
}

fn apply_value(target: AutomationTarget, v: f64, state: &mut crate::deck::DeckState) {
    match target {
        AutomationTarget::Fader => {
            state.fader = v.clamp(0.0, 1.0) as f32;
        }
        AutomationTarget::Tempo => {
            state.tempo = v;
        }
    }
}
