//! Maintains a target energy that drifts across the set (rise-and-fall arc).

#[derive(Debug)]
pub struct EnergyTarget {
    value: f64,
    direction: f64, // +0.02 rising, -0.02 falling
    upper: f64,
    lower: f64,
}

impl EnergyTarget {
    pub fn new(initial: f64) -> Self {
        Self {
            value: initial,
            direction: 0.02,
            upper: 0.7,
            lower: 0.3,
        }
    }

    pub fn current(&self) -> f64 {
        self.value
    }

    pub fn advance(&mut self) {
        self.value += self.direction;
        if self.value >= self.upper {
            self.value = self.upper;
            self.direction = -0.02;
        } else if self.value <= self.lower {
            self.value = self.lower;
            self.direction = 0.02;
        }
    }
}
