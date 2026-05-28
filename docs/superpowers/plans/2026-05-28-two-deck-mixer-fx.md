# Echo + Loop FX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add per-deck `EchoLine` (IIR delay) and `LoopBuffer` (capture-and-replay) effects, two new `AutomationTarget`s to drive them (`EchoMix`, `LoopGate`), a `DeckInitConfig` mechanism on `RecipeOutput` for static effect params, two new recipes (`EchoOut`, `LoopOut`), and CLI routing through `--force-recipe`.

**Architecture:** New `engine/src/effects.rs` module owns the two DSP types. `Deck` carries one of each, allocated to a fixed worst-case capacity at `new()`. `Deck::pull` gains a per-tick "loop branch" (skip source-queue drain when locked, read from loop buffer instead) and a per-sample echo mix at the end of the signal chain. Recipes set the static effect parameters (delay seconds, feedback, loop bars) via two new `RecipeOutput` fields the scheduler applies after `cue()` and before `play()`.

**Tech Stack:** Rust 2021, existing patterns (Biquad-style stateful DSP, automation-driven state, scheduler-driven configuration).

**Spec:** `docs/superpowers/specs/2026-05-28-two-deck-mixer-fx-design.md`

**Working branch:** `research/two-deck-mixer` (continues from sub-project 3).

---

## File structure

| Path | Action | Responsibility |
|---|---|---|
| `engine/src/effects.rs` | CREATE | `EchoLine`, `LoopBuffer` |
| `engine/src/lib.rs` | MODIFY | `pub mod effects;` |
| `engine/src/automation.rs` | MODIFY | `EchoMix`, `LoopGate` variants + clamp in `apply_value` |
| `engine/src/deck.rs` | MODIFY | Fields (`echo_line`, `loop_buffer`), state (`echo_mix`, `loop_gate`), `set_echo_params`, `set_loop_bars`, `reset_effects`, pull integration |
| `engine/src/recipe.rs` | MODIFY | `DeckInitConfig`, extend `RecipeOutput`, `EchoOut`, `LoopOut`, `RecipeChoice` extension. Existing recipes default `*_init` to `None`. |
| `engine/src/scheduler.rs` | MODIFY | After cue/before play, apply `outgoing_init` and `incoming_init` |
| `engine/src/config.rs` | MODIFY | `ForceRecipe::EchoOut`, `ForceRecipe::LoopOut` |
| `engine/src/main.rs` | MODIFY | Two new arms in the recipe match |
| `engine/tests/effects_test.rs` | CREATE | `EchoLine` + `LoopBuffer` unit tests |
| `engine/tests/automation_test.rs` | MODIFY | `EchoMix` + `LoopGate` apply tests |
| `engine/tests/deck_test.rs` | MODIFY | `cue` resets effects; `set_echo_params` + EchoMix=1 produces echoes; `set_loop_bars` + LoopGate=1 freezes source_position |
| `engine/tests/recipe_test.rs` | MODIFY | `EchoOut::build`, `LoopOut::build` tests |
| `engine/tests/mixer_integration_test.rs` | MODIFY | EchoOut + LoopOut continuous-audio tests |

---

## Tasks

### Task 1: `EchoLine` + `LoopBuffer` in `effects.rs`

**Files:**
- Create: `engine/src/effects.rs`
- Modify: `engine/src/lib.rs` (`pub mod effects;`)
- Create: `engine/tests/effects_test.rs`

- [ ] **Step 1: Add module declaration**

Edit `engine/src/lib.rs`. Add `pub mod effects;` after `pub mod deck;` (alphabetical).

- [ ] **Step 2: Write failing tests for `EchoLine`**

Create `engine/tests/effects_test.rs`:

```rust
use dj_engine::effects::{EchoLine, LoopBuffer};

const EPS: f32 = 1e-6;

#[test]
fn echo_line_zero_delay_returns_silence() {
    let mut e = EchoLine::new(100);
    e.set_params(0, 0.5);
    let (l, r) = e.process(1.0, 1.0);
    assert!((l).abs() < EPS);
    assert!((r).abs() < EPS);
}

#[test]
fn echo_line_decaying_taps_at_delay_intervals() {
    // delay=4 samples, feedback=0.5. Push an impulse, then read 20 samples and
    // confirm decaying taps at offsets 4, 8, 12, 16 with amplitudes 1.0, 0.5, 0.25, 0.125.
    let mut e = EchoLine::new(100);
    e.set_params(4, 0.5);

    let (_l, _r) = e.process(1.0, 1.0);
    let mut left_samples = Vec::new();
    for _ in 0..20 {
        let (l, _r) = e.process(0.0, 0.0);
        left_samples.push(l);
    }

    // Expected taps at indices 3, 7, 11, 15 (since the first process consumed the
    // impulse and stored it; the next process reads delay=4 back).
    let expected = [(3, 1.0), (7, 0.5), (11, 0.25), (15, 0.125)];
    for (idx, amp) in expected {
        assert!(
            (left_samples[idx] - amp).abs() < 1e-3,
            "tap at index {idx} should be {amp}, got {}",
            left_samples[idx]
        );
    }
    // Confirm non-tap positions are near zero.
    for (i, s) in left_samples.iter().enumerate() {
        if ![3, 7, 11, 15].contains(&i) {
            assert!(s.abs() < 1e-3, "non-tap at {i}: {s}");
        }
    }
}

#[test]
fn echo_line_clear_zeroes_state() {
    let mut e = EchoLine::new(100);
    e.set_params(4, 0.5);
    e.process(1.0, 1.0);
    e.process(1.0, 1.0);
    e.clear();
    // After clear, processing zero input should yield zero output.
    for _ in 0..20 {
        let (l, r) = e.process(0.0, 0.0);
        assert!(l.abs() < EPS);
        assert!(r.abs() < EPS);
    }
}

#[test]
fn echo_line_set_params_preserves_buffer() {
    // Push energy, then change params; subsequent zero-input reads should still
    // produce non-zero output (buffer contents survived).
    let mut e = EchoLine::new(100);
    e.set_params(4, 0.5);
    for _ in 0..8 {
        e.process(1.0, 1.0);
    }
    e.set_params(8, 0.5);
    let (l, _r) = e.process(0.0, 0.0);
    // Some non-zero output expected (buffer has prior energy).
    assert!(l.abs() > 1e-3 || {
        // Try a few more reads
        let mut any_nonzero = false;
        for _ in 0..15 {
            let (l2, _) = e.process(0.0, 0.0);
            if l2.abs() > 1e-3 {
                any_nonzero = true;
                break;
            }
        }
        any_nonzero
    });
}

#[test]
fn loop_buffer_write_lock_read_replays_input() {
    let mut b = LoopBuffer::new(20);
    b.set_loop_samples(10);
    // Write a known ramp.
    for i in 0..10 {
        b.write(i as f32, (i + 100) as f32);
    }
    b.lock();
    // Read 10 samples — should match the written ramp in order.
    for i in 0..10 {
        let (l, r) = b.read();
        assert!((l - i as f32).abs() < EPS, "l at {i}: got {l}");
        assert!((r - (i + 100) as f32).abs() < EPS, "r at {i}: got {r}");
    }
    // Next read should wrap back to the start of the loop region.
    let (l, _r) = b.read();
    assert!((l - 0.0).abs() < EPS, "expected wrap to 0.0, got {l}");
}

#[test]
fn loop_buffer_unlock_then_relock_uses_fresh_capture() {
    let mut b = LoopBuffer::new(20);
    b.set_loop_samples(10);
    for i in 0..10 {
        b.write(i as f32, 0.0);
    }
    b.lock();
    b.read();
    b.unlock();
    // Write new audio.
    for i in 100..110 {
        b.write(i as f32, 0.0);
    }
    b.lock();
    // The locked region should now reflect the new writes (100..109).
    let mut firsts: Vec<f32> = (0..10).map(|_| b.read().0).collect();
    firsts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert!(firsts[0] >= 100.0 - EPS, "expected fresh writes, lowest was {}", firsts[0]);
}

#[test]
fn loop_buffer_clear_zeros_state() {
    let mut b = LoopBuffer::new(20);
    b.set_loop_samples(10);
    for i in 0..10 {
        b.write(i as f32, 0.0);
    }
    b.clear();
    assert!(!b.is_locked());
    // Lock and read — should be all zeros (capture was cleared).
    b.lock();
    for _ in 0..10 {
        let (l, _r) = b.read();
        assert!(l.abs() < EPS);
    }
}
```

- [ ] **Step 3: Verify failure**

Run: `cd engine && cargo test --test effects_test`
Expected: FAIL — `EchoLine` / `LoopBuffer` don't exist.

- [ ] **Step 4: Implement `effects.rs`**

Create `engine/src/effects.rs`:

```rust
//! Per-deck DSP effects: IIR echo delay and capture-and-replay loop buffer.

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
        self.locked_pos = if next == self.capture_pos { loop_start } else { next };
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
```

- [ ] **Step 5: Verify tests pass**

Run: `cd engine && cargo test --test effects_test`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```
git add engine/src/lib.rs engine/src/effects.rs engine/tests/effects_test.rs
git commit -m "feat(engine): EchoLine and LoopBuffer in effects.rs (sub-project 4, task 1)"
```

---

### Task 2: `AutomationTarget::EchoMix` and `LoopGate`

**Files:**
- Modify: `engine/src/automation.rs`
- Modify: `engine/tests/automation_test.rs`

- [ ] **Step 1: Failing tests**

Append to `engine/tests/automation_test.rs`:

```rust
#[test]
fn echo_mix_clamped_to_zero_one() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::EchoMix, 0.0, 2.0, Curve::Step, 0, 1));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.echo_mix - 1.0).abs() < 1e-9);

    let mut q2 = AutomationQueue::new();
    q2.schedule(ramp(AutomationTarget::EchoMix, 0.0, -0.5, Curve::Step, 0, 1));
    let mut s2 = default_state();
    q2.step(1, &mut s2);
    assert!(s2.echo_mix.abs() < 1e-9);
}

#[test]
fn loop_gate_clamped_and_installed() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::LoopGate, 0.0, 1.0, Curve::Step, 0, 1));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.loop_gate - 1.0).abs() < 1e-9);
}
```

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test automation_test`
Expected: FAIL — variants and fields don't exist.

- [ ] **Step 3: Add variants and apply branches**

Edit `engine/src/automation.rs`. Replace the `AutomationTarget` enum:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomationTarget {
    Fader,
    Tempo,
    EqLow,
    EqMid,
    EqHigh,
    Filter,
    EchoMix,
    LoopGate,
}
```

Add the new branches in `apply_value`:

```rust
AutomationTarget::EchoMix => {
    state.echo_mix = v.clamp(0.0, 1.0);
}
AutomationTarget::LoopGate => {
    state.loop_gate = v.clamp(0.0, 1.0);
}
```

(Will not compile until Task 3 adds the fields. Continue.)

- [ ] **Step 4: Commit with Task 3 — see Task 3's commit step.**

---

### Task 3: `DeckState` extension + `Deck` integration

**Files:**
- Modify: `engine/src/deck.rs`
- Modify: `engine/tests/deck_test.rs`

- [ ] **Step 1: Add fields and constants**

Edit `engine/src/deck.rs`. Add to `DeckState` (after `filter_knob`):

```rust
pub echo_mix: f64,
pub loop_gate: f64,
```

Update `DeckState::default`:

```rust
echo_mix: 0.0,
loop_gate: 0.0,
```

Add constants near the existing capacity / band constants:

```rust
const ECHO_CAPACITY_SAMPLES: usize = (SAMPLE_RATE as usize) * 2;   // 2 seconds
const LOOP_CAPACITY_SAMPLES: usize = (SAMPLE_RATE as usize) * 8;   // 8 seconds
```

Add to `Deck` (after the filter biquads):

```rust
echo_line: crate::effects::EchoLine,
loop_buffer: crate::effects::LoopBuffer,
```

Edit `Deck::new`. Construct the two effects:

```rust
let echo_line = crate::effects::EchoLine::new(ECHO_CAPACITY_SAMPLES);
let loop_buffer = crate::effects::LoopBuffer::new(LOOP_CAPACITY_SAMPLES);
```

Include `echo_line, loop_buffer` in the struct construction.

Add a `reset_effects` helper. Call it from both `cue()` and `unload()` alongside the existing `reset_eq` and `reset_filter`:

```rust
fn reset_effects(&mut self) {
    self.echo_line.clear();
    self.echo_line.set_params(0, 0.0);
    self.loop_buffer.clear();
    self.state.echo_mix = 0.0;
    self.state.loop_gate = 0.0;
}
```

Add two public methods for scheduler use:

```rust
impl Deck {
    pub fn set_echo_params(&mut self, delay_seconds: f64, feedback: f64) {
        let delay_samples = (delay_seconds * SAMPLE_RATE as f64) as usize;
        self.echo_line.set_params(delay_samples, feedback);
    }

    pub fn set_loop_bars(&mut self, bars: usize, bar_seconds: f64) {
        let samples = (bars as f64 * bar_seconds * SAMPLE_RATE as f64) as usize;
        self.loop_buffer.set_loop_samples(samples);
    }
}
```

- [ ] **Step 2: Write failing tests for cue resets and configuration**

Append to `engine/tests/deck_test.rs`:

```rust
#[tokio::test]
async fn cue_resets_echo_and_loop_state() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.set_echo_params(0.5, 0.5);
    d.set_loop_bars(4, 0.5);
    // Re-cue resets effect state.
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    assert!(d.state().echo_mix.abs() < f64::EPSILON);
    assert!(d.state().loop_gate.abs() < f64::EPSILON);
}
```

- [ ] **Step 3: Verify build + tests pass**

Run: `cd engine && cargo build && cargo test --test automation_test --test deck_test`
Expected: clean build; new tests pass.

- [ ] **Step 4: Commit Tasks 2 + 3 together**

```
git add engine/src/automation.rs engine/src/deck.rs engine/tests/automation_test.rs engine/tests/deck_test.rs
git commit -m "feat(engine): EchoMix/LoopGate automation targets + per-deck EchoLine/LoopBuffer (sub-project 4, tasks 2-3)"
```

---

### Task 4: `Deck::pull` integration (loop branch + capture + echo mix)

**Files:**
- Modify: `engine/src/deck.rs`
- Modify: `engine/tests/deck_test.rs`

- [ ] **Step 1: Failing tests**

Append to `engine/tests/deck_test.rs`:

```rust
#[tokio::test]
async fn echo_mix_one_produces_audible_tail() {
    // With echo configured and EchoMix=1, the post-fader output should contain
    // non-zero energy after the fader hits 0 (i.e., echo tails ring through).
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.set_echo_params(0.05, 0.8);  // 50 ms delay, heavy feedback for audible tail
    d.play();
    d.test_set_fader(1.0);
    d.schedule(Automation {
        target: AutomationTarget::EchoMix,
        t_start_frames: 0,
        t_end_frames: 1,
        from: 1.0,
        to: 1.0,
        curve: Curve::Step,
    });
    // Pull ~250 ms of dry+wet, then drop the fader to 0 and pull 100 ms more —
    // expect the second block to have non-trivial RMS (the echo tail).
    let _ = d.pull(11025).await.unwrap(); // ~250ms
    d.test_set_fader(0.0);
    let tail = d.pull(4410).await.unwrap(); // ~100ms
    let rms = rms_stereo(&tail);
    // Echo line has the previous 250ms of energy in its buffer; the wet output
    // should be audibly non-zero even with the dry path silenced.
    assert!(rms > 0.001, "expected non-trivial tail, got rms {rms}");
}

#[tokio::test]
async fn loop_gate_freezes_source_position() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.set_loop_bars(1, 0.5); // 1 bar at 0.5 s/bar = 0.5 s loop
    d.play();
    d.test_set_fader(1.0);
    // Run for ~1 s to fill the capture buffer.
    let _ = d.pull(44100).await.unwrap();
    let pos_before_lock = d.source_position_seconds();
    // Lock the loop.
    d.schedule(Automation {
        target: AutomationTarget::LoopGate,
        t_start_frames: d.frame_cursor(),
        t_end_frames: d.frame_cursor() + 1,
        from: 1.0,
        to: 1.0,
        curve: Curve::Step,
    });
    // Pull another ~1 s — source_position_seconds should NOT advance.
    let _ = d.pull(44100).await.unwrap();
    let pos_after_lock = d.source_position_seconds();
    assert!(
        (pos_after_lock - pos_before_lock).abs() < 0.01,
        "source position should freeze; before={pos_before_lock} after={pos_after_lock}"
    );
}
```

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test deck_test`
Expected: FAIL — echo not applied, loop not freezing source.

- [ ] **Step 3: Integrate effects into `Deck::pull`**

Edit `engine/src/deck.rs`. Replace the body of `pull` with the integrated version. The order matters; here is the full replacement:

```rust
pub async fn pull(&mut self, n_frames: usize) -> Result<Vec<f32>> {
    let start_cursor = self.frame_cursor;
    self.maybe_prepare_tempo_ramp(start_cursor).await?;
    self.frame_cursor += n_frames as u64;

    if !matches!(self.state.play_state, PlayState::Playing) {
        self.automations.step(self.frame_cursor, &mut self.state);
        return Ok(vec![0.0; n_frames * (CHANNELS as usize)]);
    }

    // Pre-step: apply any automations at start_cursor so EQ / filter / loop
    // transitions reflect the current automation state before we decide.
    self.automations.step(start_cursor, &mut self.state);
    self.recompute_eq_if_dirty();
    self.recompute_filter_if_dirty();

    // Loop transitions (boundary-trigger semantics at tick start).
    if self.state.loop_gate >= 0.5 && !self.loop_buffer.is_locked() {
        self.loop_buffer.lock();
    } else if self.state.loop_gate < 0.5 && self.loop_buffer.is_locked() {
        self.loop_buffer.unlock();
    }
    let loop_active = self.loop_buffer.is_locked();
    let filter_active = self.state.filter_knob.abs() >= FILTER_BYPASS_EPSILON;

    let need_samples = n_frames * (CHANNELS as usize);
    let mut out: Vec<f32> = Vec::with_capacity(need_samples);

    if loop_active {
        // Fill `out` from the locked loop buffer; do NOT drain the source.
        for _ in 0..n_frames {
            let (l, r) = self.loop_buffer.read();
            out.push(l);
            out.push(r);
        }
    } else {
        // Drain source queue (existing logic).
        while out.len() < need_samples {
            let remaining_frames = (need_samples - out.len()) / (CHANNELS as usize);
            if remaining_frames == 0 {
                break;
            }
            let Some(source) = self.sources.front_mut() else {
                break;
            };
            let chunk = source.pull(remaining_frames).await?;
            if chunk.is_empty() {
                self.sources.pop_front();
                continue;
            }
            out.extend_from_slice(&chunk);
            if source.is_exhausted() {
                self.sources.pop_front();
            }
        }
        if out.len() < need_samples {
            out.resize(need_samples, 0.0);
        }
    }

    // Per-sample chain.
    for f in 0..n_frames {
        let cursor = start_cursor + f as u64;
        self.automations.step(cursor, &mut self.state);

        let li = f * 2;
        let ri = li + 1;
        let raw_l = out[li];
        let raw_r = out[ri];

        // Capture only while not locked (avoid self-feedback into the loop buffer).
        if !loop_active {
            self.loop_buffer.write(raw_l, raw_r);
        }

        // EQ
        let l = self.eq_high_l.process(self.eq_mid_l.process(self.eq_low_l.process(raw_l)));
        let r = self.eq_high_r.process(self.eq_mid_r.process(self.eq_low_r.process(raw_r)));

        // Filter (one-knob)
        let (l, r) = if filter_active {
            (self.filter_l.process(l), self.filter_r.process(r))
        } else {
            (l, r)
        };

        // Fader
        let g = self.state.fader;
        let dry_l = l * g;
        let dry_r = r * g;

        // Echo: pre-fader signal into the line; line's wet output added on top.
        let (wet_l, wet_r) = self.echo_line.process(l, r);
        let mix = self.state.echo_mix as f32;
        out[li] = dry_l + wet_l * mix;
        out[ri] = dry_r + wet_r * mix;
    }
    self.automations.step(self.frame_cursor, &mut self.state);

    // Advance source position only when NOT in loop mode. Looping freezes the
    // playhead so the recipe can rejoin from where the lock happened.
    if !loop_active {
        let emitted_seconds = n_frames as f64 / SAMPLE_RATE as f64;
        self.source_position_seconds += emitted_seconds * self.state.tempo;
    }
    Ok(out)
}
```

- [ ] **Step 4: Verify tests pass**

Run: `cd engine && cargo test --test deck_test`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/deck.rs engine/tests/deck_test.rs
git commit -m "feat(engine): Deck::pull integrates echo + loop (sub-project 4, task 4)"
```

---

### Task 5: `DeckInitConfig` and `RecipeOutput` extension + scheduler

**Files:**
- Modify: `engine/src/recipe.rs`
- Modify: `engine/src/scheduler.rs`

- [ ] **Step 1: Extend `RecipeOutput` with init configs**

Edit `engine/src/recipe.rs`. Add a struct near the top:

```rust
#[derive(Clone, Debug, Default)]
pub struct DeckInitConfig {
    pub echo_delay_seconds: Option<f64>,
    pub echo_feedback: Option<f64>,
    pub loop_bars: Option<usize>,
    pub loop_bar_seconds: Option<f64>,
}
```

Update `RecipeOutput`:

```rust
pub struct RecipeOutput {
    pub outgoing_automations: Vec<Automation>,
    pub incoming_automations: Vec<Automation>,
    pub outgoing_init: Option<DeckInitConfig>,
    pub incoming_init: Option<DeckInitConfig>,
    pub total_duration_frames: u64,
}
```

Update EVERY existing recipe (`EqualPowerCrossfade`, `HardCut`, `BassSwap`, `FilterFade`, `FilterKill`) to add `outgoing_init: None, incoming_init: None,` to their `RecipeOutput` construction. No other change to their behavior.

- [ ] **Step 2: Apply the init configs in `scheduler::arm`**

Edit `engine/src/scheduler.rs`. After cue/play and after `recipe.build(&ctx)`, add the init-application step:

```rust
let RecipeOutput {
    outgoing_automations,
    incoming_automations,
    outgoing_init,
    incoming_init,
    total_duration_frames,
} = recipe.build(&ctx);

if let Some(cfg) = outgoing_init {
    apply_deck_init(mixer.deck(outgoing_deck), &cfg);
}
if let Some(cfg) = incoming_init {
    apply_deck_init(mixer.deck(incoming_deck), &cfg);
}

// existing schedule-automations + return TransitionHandle
```

Add a free function (or inline in arm):

```rust
fn apply_deck_init(deck: &mut crate::deck::Deck, cfg: &DeckInitConfig) {
    if let (Some(delay), Some(feedback)) = (cfg.echo_delay_seconds, cfg.echo_feedback) {
        deck.set_echo_params(delay, feedback);
    }
    if let (Some(bars), Some(bar_seconds)) = (cfg.loop_bars, cfg.loop_bar_seconds) {
        deck.set_loop_bars(bars, bar_seconds);
    }
}
```

(Import `DeckInitConfig` from `crate::recipe`.)

- [ ] **Step 3: Verify build + tests still pass**

Run: `cd engine && cargo build && cargo test`
Expected: existing tests still pass (every recipe now constructs a `RecipeOutput` with the new `None` fields).

- [ ] **Step 4: Commit**

```
git add engine/src/recipe.rs engine/src/scheduler.rs
git commit -m "feat(engine): DeckInitConfig in RecipeOutput, scheduler applies init (sub-project 4, task 5)"
```

---

### Task 6: `EchoOut` and `LoopOut` recipes + `RecipeChoice` extension

**Files:**
- Modify: `engine/src/recipe.rs`
- Modify: `engine/tests/recipe_test.rs`

- [ ] **Step 1: Failing tests**

Append to `engine/tests/recipe_test.rs`:

```rust
use dj_engine::recipe::{EchoOut, LoopOut};

#[test]
fn echo_out_emits_fader_pair_plus_outgoing_echo_ramp_and_init() {
    let outgoing = track(120.0);
    let incoming = track(120.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
    };
    let ctx = ctx_for(&outgoing, &incoming, &plan);
    let r = EchoOut.build(&ctx);

    // Outgoing: Fader + EchoMix ramp.
    assert_eq!(r.outgoing_automations.len(), 2);
    assert!(r.outgoing_automations.iter().any(|a|
        a.target == AutomationTarget::Fader && a.curve == Curve::EqualPowerOut));
    let echo_ramp = r.outgoing_automations.iter()
        .find(|a| a.target == AutomationTarget::EchoMix)
        .expect("outgoing echo ramp");
    assert_eq!(echo_ramp.from, 0.0);
    assert_eq!(echo_ramp.to, 1.0);

    // Incoming: Fader + Tempo Step.
    assert_eq!(r.incoming_automations.len(), 2);

    // Init: outgoing has echo configured; incoming has no init.
    let init = r.outgoing_init.as_ref().expect("outgoing_init for EchoOut");
    assert!(init.echo_delay_seconds.is_some());
    assert!((init.echo_feedback.unwrap_or(0.0) - 0.5).abs() < 1e-9);
    assert!(r.incoming_init.is_none());
}

#[test]
fn loop_out_emits_fader_pair_plus_outgoing_loop_gate_and_init() {
    let outgoing = track(120.0);
    let incoming = track(120.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
    };
    let ctx = ctx_for(&outgoing, &incoming, &plan);
    let r = LoopOut.build(&ctx);

    // Outgoing: Fader + LoopGate on + LoopGate off.
    assert_eq!(r.outgoing_automations.len(), 3);
    let loop_ons: Vec<_> = r.outgoing_automations.iter()
        .filter(|a| a.target == AutomationTarget::LoopGate)
        .collect();
    assert_eq!(loop_ons.len(), 2);

    // Init: outgoing has loop configured.
    let init = r.outgoing_init.as_ref().expect("outgoing_init for LoopOut");
    assert_eq!(init.loop_bars, Some(4));
    assert!(init.loop_bar_seconds.is_some());
}
```

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test recipe_test`
Expected: FAIL — recipes don't exist.

- [ ] **Step 3: Implement `EchoOut` and `LoopOut`**

Append to `engine/src/recipe.rs`:

```rust
pub struct EchoOut;

impl Recipe for EchoOut {
    fn name(&self) -> &'static str { "echo_out" }

    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let beat_seconds = bar_seconds / 4.0;
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
        let incoming_tempo_step = Automation {
            target: AutomationTarget::Tempo,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: ratio,
            to: ratio,
            curve: Curve::Step,
        };
        let outgoing_echo_ramp = Automation {
            target: AutomationTarget::EchoMix,
            t_start_frames: t0,
            t_end_frames: t0 + length_frames / 2,
            from: 0.0,
            to: 1.0,
            curve: Curve::Linear,
        };

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader, outgoing_echo_ramp],
            incoming_automations: vec![incoming_fader, incoming_tempo_step],
            outgoing_init: Some(DeckInitConfig {
                echo_delay_seconds: Some(beat_seconds),
                echo_feedback: Some(0.5),
                loop_bars: None,
                loop_bar_seconds: None,
            }),
            incoming_init: None,
            total_duration_frames: length_frames,
        }
    }
}

pub struct LoopOut;

impl Recipe for LoopOut {
    fn name(&self) -> &'static str { "loop_out" }

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
        let incoming_tempo_step = Automation {
            target: AutomationTarget::Tempo,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: ratio,
            to: ratio,
            curve: Curve::Step,
        };
        let outgoing_loop_on = Automation {
            target: AutomationTarget::LoopGate,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: 1.0,
            to: 1.0,
            curve: Curve::Step,
        };
        let outgoing_loop_off = Automation {
            target: AutomationTarget::LoopGate,
            t_start_frames: t1,
            t_end_frames: t1 + 1,
            from: 0.0,
            to: 0.0,
            curve: Curve::Step,
        };

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader, outgoing_loop_on, outgoing_loop_off],
            incoming_automations: vec![incoming_fader, incoming_tempo_step],
            outgoing_init: Some(DeckInitConfig {
                echo_delay_seconds: None,
                echo_feedback: None,
                loop_bars: Some(4),
                loop_bar_seconds: Some(bar_seconds),
            }),
            incoming_init: None,
            total_duration_frames: length_frames,
        }
    }
}
```

Extend `RecipeChoice`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecipeChoice {
    EqualPower,
    HardCut,
    BassSwap,
    FilterFade,
    FilterKill,
    EchoOut,
    LoopOut,
}
```

- [ ] **Step 4: Verify tests pass**

Run: `cd engine && cargo test --test recipe_test`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/recipe.rs engine/tests/recipe_test.rs
git commit -m "feat(engine): EchoOut and LoopOut recipes (sub-project 4, task 6)"
```

---

### Task 7: CLI + `main.rs` wiring

**Files:**
- Modify: `engine/src/config.rs`
- Modify: `engine/src/main.rs`

- [ ] **Step 1: Extend `ForceRecipe`**

Edit `engine/src/config.rs`. Add the two variants:

```rust
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ForceRecipe {
    EqualPower,
    HardCut,
    BassSwap,
    FilterFade,
    FilterKill,
    EchoOut,
    LoopOut,
}

impl ForceRecipe {
    pub fn to_recipe_choice(self) -> crate::recipe::RecipeChoice {
        match self {
            ForceRecipe::EqualPower => crate::recipe::RecipeChoice::EqualPower,
            ForceRecipe::HardCut => crate::recipe::RecipeChoice::HardCut,
            ForceRecipe::BassSwap => crate::recipe::RecipeChoice::BassSwap,
            ForceRecipe::FilterFade => crate::recipe::RecipeChoice::FilterFade,
            ForceRecipe::FilterKill => crate::recipe::RecipeChoice::FilterKill,
            ForceRecipe::EchoOut => crate::recipe::RecipeChoice::EchoOut,
            ForceRecipe::LoopOut => crate::recipe::RecipeChoice::LoopOut,
        }
    }
}
```

- [ ] **Step 2: Wire new arms in `main.rs`**

Edit `engine/src/main.rs`. Extend `recipe_name`:

```rust
dj_engine::recipe::RecipeChoice::EchoOut => "echo_out",
dj_engine::recipe::RecipeChoice::LoopOut => "loop_out",
```

Add two new arms to the `let handle = match choice { ... }` block, mirroring the existing arms (using `&dj_engine::recipe::EchoOut` and `&dj_engine::recipe::LoopOut`).

- [ ] **Step 3: Verify build + tests + clippy**

Run sequentially:
```
cd engine
cargo build --release
cargo test
cargo clippy --all-targets -- -D warnings
```
Expected: all clean.

- [ ] **Step 4: Commit**

```
git add engine/src/config.rs engine/src/main.rs
git commit -m "feat(engine): CLI + main.rs route EchoOut and LoopOut (sub-project 4, task 7)"
```

---

### Task 8: Integration tests

**Files:**
- Modify: `engine/tests/mixer_integration_test.rs`

- [ ] **Step 1: Add EchoOut and LoopOut integration tests**

Append to `engine/tests/mixer_integration_test.rs`:

```rust
use dj_engine::recipe::{EchoOut, LoopOut};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn echo_out_transition_outputs_continuous_audio() {
    let cap = (SAMPLE_RATE as usize) * 2 * 10;
    let (producer, consumer) = make_ring(cap);
    let mut mixer = Mixer::new(producer);
    let outgoing = track("sine_440_6s.wav", 6.0);
    let incoming = track("sine_660_6s.wav", 6.0);
    {
        let d = mixer.deck(DeckId::A);
        d.cue(outgoing.clone(), 0.0, 1.0).await.unwrap();
        d.play();
        d.test_set_fader(1.0);
    }
    let drain = drain_to_vec(consumer);
    tick_until(&mut mixer, 22_050).await.unwrap();
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.5,
        incoming_crossfade_start_seconds: 1.0,
        length_bars: 2,
        use_eq_swap: false,
    };
    let handle = TransitionScheduler::arm(
        &mut mixer, DeckId::A, DeckId::B,
        &outgoing, &incoming, &plan, &EchoOut,
    ).await.unwrap();
    tick_until(&mut mixer, handle.end_master_clock_frames).await.unwrap();
    let all = drain.await.unwrap();
    assert!(!all.is_empty());
    let mut zero_run = 0usize;
    let mut max_zero_run = 0usize;
    for s in &all {
        if *s == 0.0 { zero_run += 1; max_zero_run = max_zero_run.max(zero_run); }
        else { zero_run = 0; }
    }
    assert!(max_zero_run < 4096, "long silence: {max_zero_run} samples");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn loop_out_transition_outputs_continuous_audio() {
    let cap = (SAMPLE_RATE as usize) * 2 * 10;
    let (producer, consumer) = make_ring(cap);
    let mut mixer = Mixer::new(producer);
    let outgoing = track("sine_440_6s.wav", 6.0);
    let incoming = track("sine_660_6s.wav", 6.0);
    {
        let d = mixer.deck(DeckId::A);
        d.cue(outgoing.clone(), 0.0, 1.0).await.unwrap();
        d.play();
        d.test_set_fader(1.0);
    }
    let drain = drain_to_vec(consumer);
    tick_until(&mut mixer, 22_050).await.unwrap();
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.5,
        incoming_crossfade_start_seconds: 1.0,
        length_bars: 2,
        use_eq_swap: false,
    };
    let handle = TransitionScheduler::arm(
        &mut mixer, DeckId::A, DeckId::B,
        &outgoing, &incoming, &plan, &LoopOut,
    ).await.unwrap();
    tick_until(&mut mixer, handle.end_master_clock_frames).await.unwrap();
    let all = drain.await.unwrap();
    assert!(!all.is_empty());
    let mut zero_run = 0usize;
    let mut max_zero_run = 0usize;
    for s in &all {
        if *s == 0.0 { zero_run += 1; max_zero_run = max_zero_run.max(zero_run); }
        else { zero_run = 0; }
    }
    assert!(max_zero_run < 4096, "long silence: {max_zero_run} samples");
}
```

(If `drain_to_vec` isn't already a named helper in this file, copy whatever drain pattern the existing tests use.)

- [ ] **Step 2: Verify**

Run: `cd engine && cargo test --test mixer_integration_test`
Expected: all integration tests pass (6 total now: original + BassSwap + FilterFade + FilterKill + EchoOut + LoopOut).

- [ ] **Step 3: Commit**

```
git add engine/tests/mixer_integration_test.rs
git commit -m "test(engine): integration tests for EchoOut and LoopOut (sub-project 4, task 8)"
```

---

### Task 9: Final verification

**Files:** none.

- [ ] **Step 1: `cargo fmt`**

Run: `cd engine && cargo fmt --check`. If output: `cargo fmt && git add -A && git commit -m "chore(engine): fmt after sub-project 4"`.

- [ ] **Step 2: `cargo clippy`**

Run: `cd engine && cargo clippy --all-targets -- -D warnings`. Expected: clean.

- [ ] **Step 3: `cargo test`**

Run: `cd engine && cargo test`. Expected: every test passes.

- [ ] **Step 4: `cargo build --release`**

Run: `cd engine && cargo build --release`. Expected: success.

---

## Self-Review notes

Coverage check:
- Spec Component 1 (`EchoLine`) → Task 1.
- Spec Component 2 (`LoopBuffer`) → Task 1.
- Spec Component 3 (`AutomationTarget` extension) → Task 2.
- Spec Component 4 (`DeckState` extension) → Task 3.
- Spec Component 5 (`Deck` integration + setters + reset) → Task 3.
- Spec Component 6 (`Deck::pull` ordering) → Task 4.
- Spec Component 7 (`DeckInitConfig` + `RecipeOutput`) → Task 5.
- Spec Component 8 (scheduler applies init) → Task 5.
- Spec Component 9 (`EchoOut` recipe) → Task 6.
- Spec Component 10 (`LoopOut` recipe) → Task 6.
- Spec Component 11 (`RecipeChoice` + CLI + main arms) → Tasks 6+7.

No placeholders. Every code step is complete.
