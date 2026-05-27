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
