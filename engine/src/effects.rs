//! Per-deck DSP effects: IIR echo delay and capture-and-replay loop buffer.
//!
//! NOTE: `EchoLine` is currently unused — both echo-based recipes (echo-out,
//! echo-throw) were dropped in the 2026-05-29 catalog consolidation after
//! listening feedback found the echo character not audibly perceptible. The
//! implementation is kept for future echo recipes once the audibility
//! is investigated. See:
//!   - `docs/recipe-feedback-2026-05-29.md`
//!   - `docs/superpowers/specs/2026-05-29-recipe-catalog-consolidation-design.md`

#[allow(dead_code)]
pub struct EchoLine {
    buffer_l: Vec<f32>,
    buffer_r: Vec<f32>,
    write_pos: usize,
    delay_samples: usize,
    feedback: f64,
}

impl EchoLine {
    pub fn new(capacity_samples: usize) -> Self {
        Self {
            buffer_l: vec![0.0; capacity_samples.max(1)],
            buffer_r: vec![0.0; capacity_samples.max(1)],
            write_pos: 0,
            delay_samples: 0,
            feedback: 0.0,
        }
    }

    pub fn set_params(&mut self, delay_samples: usize, feedback: f64) {
        self.delay_samples = delay_samples.min(self.buffer_l.len().saturating_sub(1));
        self.feedback = feedback.clamp(0.0, 0.95);
    }

    pub fn clear(&mut self) {
        for x in self.buffer_l.iter_mut() {
            *x = 0.0;
        }
        for x in self.buffer_r.iter_mut() {
            *x = 0.0;
        }
        self.write_pos = 0;
    }

    pub fn process(&mut self, l_in: f32, r_in: f32) -> (f32, f32) {
        if self.delay_samples == 0 {
            return (0.0, 0.0);
        }
        let cap = self.buffer_l.len();
        let read_pos = (self.write_pos + cap - self.delay_samples) % cap;
        let delayed_l = self.buffer_l[read_pos];
        let delayed_r = self.buffer_r[read_pos];
        let store_l = l_in + (delayed_l as f64 * self.feedback) as f32;
        let store_r = r_in + (delayed_r as f64 * self.feedback) as f32;
        self.buffer_l[self.write_pos] = store_l;
        self.buffer_r[self.write_pos] = store_r;
        self.write_pos = (self.write_pos + 1) % cap;
        (delayed_l, delayed_r)
    }
}

pub struct LoopBuffer {
    capture_l: Vec<f32>,
    capture_r: Vec<f32>,
    capture_pos: usize,
    is_locked: bool,
    locked_pos: usize,
    active_len: usize,
}

impl LoopBuffer {
    pub fn new(capacity_samples: usize) -> Self {
        Self {
            capture_l: vec![0.0; capacity_samples.max(1)],
            capture_r: vec![0.0; capacity_samples.max(1)],
            capture_pos: 0,
            is_locked: false,
            locked_pos: 0,
            active_len: 0,
        }
    }

    pub fn set_loop_samples(&mut self, samples: usize) {
        self.active_len = samples.min(self.capture_l.len());
    }

    pub fn write(&mut self, l: f32, r: f32) {
        let cap = self.capture_l.len();
        self.capture_l[self.capture_pos] = l;
        self.capture_r[self.capture_pos] = r;
        self.capture_pos = (self.capture_pos + 1) % cap;
    }

    pub fn lock(&mut self) {
        if self.active_len == 0 {
            return;
        }
        let cap = self.capture_l.len();
        self.locked_pos = (self.capture_pos + cap - self.active_len) % cap;
        self.is_locked = true;
    }

    pub fn unlock(&mut self) {
        self.is_locked = false;
    }

    pub fn is_locked(&self) -> bool {
        self.is_locked
    }

    pub fn read(&mut self) -> (f32, f32) {
        if self.active_len == 0 || !self.is_locked {
            return (0.0, 0.0);
        }
        let cap = self.capture_l.len();
        let l = self.capture_l[self.locked_pos];
        let r = self.capture_r[self.locked_pos];
        // Advance and wrap within the active loop window.
        let loop_start = (self.capture_pos + cap - self.active_len) % cap;
        let next = (self.locked_pos + 1) % cap;
        // If we've reached the boundary (capture_pos), wrap to loop_start.
        self.locked_pos = if next == self.capture_pos {
            loop_start
        } else {
            next
        };
        (l, r)
    }

    pub fn clear(&mut self) {
        for x in self.capture_l.iter_mut() {
            *x = 0.0;
        }
        for x in self.capture_r.iter_mut() {
            *x = 0.0;
        }
        self.capture_pos = 0;
        self.is_locked = false;
        self.locked_pos = 0;
    }
}
