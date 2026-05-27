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
