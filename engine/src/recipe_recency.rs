//! Ring buffer of recently-chosen `RecipeChoice` values. Used by the scored
//! `pick_recipe` to penalize recipes that have fired in the last N transitions
//! so we get variety across a set.

use std::collections::VecDeque;

use crate::recipe::RecipeChoice;

pub struct RecipeRecency {
    ring: VecDeque<RecipeChoice>,
    capacity: usize,
}

impl RecipeRecency {
    pub fn new(capacity: usize) -> Self {
        Self {
            ring: VecDeque::with_capacity(capacity.max(1)),
            capacity,
        }
    }

    pub fn record(&mut self, choice: RecipeChoice) {
        if self.capacity == 0 {
            return;
        }
        if self.ring.len() >= self.capacity {
            self.ring.pop_front();
        }
        self.ring.push_back(choice);
    }

    pub fn contains(&self, choice: RecipeChoice) -> bool {
        self.ring.iter().any(|&c| c == choice)
    }
}
