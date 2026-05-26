//! Recently-played ring buffer. Tracks the last N track IDs.

use std::collections::VecDeque;

#[derive(Debug)]
pub struct RecencyRing {
    capacity: usize,
    items: VecDeque<String>,
}

impl RecencyRing {
    pub fn new(capacity: usize) -> Self {
        Self { capacity, items: VecDeque::with_capacity(capacity.max(1)) }
    }

    pub fn record(&mut self, id: String) {
        if self.capacity == 0 {
            return;
        }
        if self.items.len() == self.capacity {
            self.items.pop_front();
        }
        self.items.push_back(id);
    }

    pub fn was_recent(&self, id: &str) -> bool {
        self.items.iter().any(|s| s == id)
    }
}
