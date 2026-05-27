# Per-Deck EQ + BassSwap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add per-deck 3-band EQ (low-shelf 200 Hz, peaking-mid 1 kHz Q=1, high-shelf 5 kHz) to each `Deck`, driven by new `EqLow/EqMid/EqHigh` automation targets. Add a `BassSwap` recipe that uses `EqLow` automations to swap basses across a crossfade. Route `pick_recipe` so `use_eq_swap=true` plans get `BassSwap`.

**Architecture:** Each `Deck` holds six biquad filters (3 bands × stereo). `Deck::pull` applies them per sample, between the decoder and the fader. Biquad coefficients are recomputed at the start of each tick when any `eq_*_db` differs from the last-used value, so coefficient updates are O(1) when state hasn't changed and smooth-stepped when it has. The `BassSwap` recipe adapts its swap timing to the sidecar's `band_energy_means.bass` (skip EQ entirely for bass-light pairs; widen the window for bass-heavy pairs).

**Tech Stack:** Rust 2021 edition, `tokio` async, existing `Biquad` in `engine/src/eq.rs` (RBJ cookbook formulas).

**Spec:** `docs/superpowers/specs/2026-05-28-two-deck-mixer-eq-design.md`

**Working branch:** `research/two-deck-mixer` (continues from sub-project 1).

---

## File structure

| Path | Action | Responsibility |
|---|---|---|
| `engine/src/eq.rs` | MODIFY | Add `low_shelf` / `peaking` / `high_shelf` constructors + `set_*` mutators on `Biquad`. |
| `engine/src/automation.rs` | MODIFY | Add `EqLow / EqMid / EqHigh` variants to `AutomationTarget`. Clamp `[-40, +6]` in `apply_value`. |
| `engine/src/deck.rs` | MODIFY | Add 3 dB fields to `DeckState`, 6 biquads + 3 cache fields to `Deck`, init in `new/cue/unload`, EQ chain in `pull`. |
| `engine/src/recipe.rs` | MODIFY | Add `BassSwap` recipe with bass-content adaptation. Extend `RecipeChoice` and `pick_recipe`. |
| `engine/src/main.rs` | MODIFY | Add `BassSwap` arm in the recipe match; update `transition_type_label` labels. |
| `engine/tests/eq_test.rs` | MODIFY | Add sanity tests for new constructors. |
| `engine/tests/automation_test.rs` | MODIFY | Add tests for `EqLow/EqMid/EqHigh` apply. |
| `engine/tests/recipe_test.rs` | MODIFY | Add `BassSwap` build tests (3 bass-content cases) + `pick_recipe` test. |
| `engine/tests/deck_test.rs` | MODIFY | Add `cue` resets EQ; `EqLow Step -40` attenuates 100 Hz vs 1 kHz. |

---

## Tasks

### Task 1: Shelf + peaking constructors on `Biquad`

**Files:**
- Modify: `engine/src/eq.rs`
- Modify: `engine/tests/eq_test.rs`

- [ ] **Step 1: Write the failing tests**

Append to `engine/tests/eq_test.rs`:

```rust
use dj_engine::eq::Biquad;

const SR: f64 = 44100.0;

fn process_stream(bq: &mut Biquad, samples: &[f32]) -> Vec<f32> {
    samples.iter().map(|s| bq.process(*s)).collect()
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() { return 0.0; }
    let s: f32 = samples.iter().map(|x| x * x).sum();
    (s / samples.len() as f32).sqrt()
}

fn sine(freq_hz: f64, seconds: f64) -> Vec<f32> {
    let n = (seconds * SR) as usize;
    (0..n)
        .map(|i| (2.0 * std::f64::consts::PI * freq_hz * (i as f64) / SR).sin() as f32)
        .collect()
}

#[test]
fn low_shelf_at_zero_db_is_passthrough() {
    let mut bq = Biquad::low_shelf(SR, 200.0, 0.0);
    let s = sine(100.0, 0.2);
    let out = process_stream(&mut bq, &s);
    // 0 dB gain → output RMS approximately equals input RMS.
    let r_in = rms(&s);
    let r_out = rms(&out);
    assert!((r_in - r_out).abs() / r_in < 0.05, "in {r_in} out {r_out}");
}

#[test]
fn low_shelf_minus40_db_attenuates_below_freq() {
    let mut bq = Biquad::low_shelf(SR, 200.0, -40.0);
    let low = sine(100.0, 0.5);
    let low_out = process_stream(&mut bq, &low);
    // After a brief transient settle, the steady-state RMS should be much smaller.
    // Skip the first 0.1 s to let the IIR state settle.
    let skip = (0.1 * SR) as usize;
    let r_in = rms(&low[skip..]);
    let r_out = rms(&low_out[skip..]);
    assert!(r_out / r_in < 0.05, "expected strong attenuation, got ratio {}", r_out / r_in);
}

#[test]
fn low_shelf_minus40_db_passes_through_high_freq() {
    let mut bq = Biquad::low_shelf(SR, 200.0, -40.0);
    let high = sine(5000.0, 0.5);
    let high_out = process_stream(&mut bq, &high);
    let skip = (0.1 * SR) as usize;
    let r_in = rms(&high[skip..]);
    let r_out = rms(&high_out[skip..]);
    assert!((r_in - r_out).abs() / r_in < 0.1, "high freq should be near-passthrough, got ratio {}", r_out / r_in);
}

#[test]
fn peaking_zero_db_is_passthrough() {
    let mut bq = Biquad::peaking(SR, 1000.0, 1.0, 0.0);
    let s = sine(1000.0, 0.2);
    let out = process_stream(&mut bq, &s);
    let r_in = rms(&s);
    let r_out = rms(&out);
    assert!((r_in - r_out).abs() / r_in < 0.05, "in {r_in} out {r_out}");
}

#[test]
fn high_shelf_minus40_db_attenuates_above_freq() {
    let mut bq = Biquad::high_shelf(SR, 5000.0, -40.0);
    let high = sine(8000.0, 0.5);
    let high_out = process_stream(&mut bq, &high);
    let skip = (0.1 * SR) as usize;
    let r_in = rms(&high[skip..]);
    let r_out = rms(&high_out[skip..]);
    assert!(r_out / r_in < 0.1, "expected attenuation, got ratio {}", r_out / r_in);
}

#[test]
fn set_low_shelf_does_not_reset_state() {
    // Start with a -40 dB low shelf, drive it with a low-freq sine until the
    // state is non-trivial, then re-set to a different gain — the output
    // should NOT jump (the next sample should be close to the previous one).
    let mut bq = Biquad::low_shelf(SR, 200.0, -40.0);
    let s = sine(100.0, 0.1);
    let _ = process_stream(&mut bq, &s);
    let last = bq.process(0.5);
    bq.set_low_shelf(SR, 200.0, 0.0);
    let next = bq.process(0.5);
    assert!((last - next).abs() < 0.5, "smoothness check: |{last} - {next}|");
}
```

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test eq_test`
Expected: FAIL — `low_shelf` / `peaking` / `high_shelf` / `set_low_shelf` / `set_peaking` / `set_high_shelf` not defined.

- [ ] **Step 3: Implement the constructors and setters**

Append to `engine/src/eq.rs`:

```rust
impl Biquad {
    /// Low-shelf, RBJ cookbook. Shelf slope S = 1.0 (max flat, no overshoot).
    pub fn low_shelf(sample_rate: f64, freq_hz: f64, gain_db: f64) -> Self {
        let (b0, b1, b2, a1, a2) = low_shelf_coeffs(sample_rate, freq_hz, gain_db);
        Self {
            b0,
            b1,
            b2,
            a1,
            a2,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    pub fn set_low_shelf(&mut self, sample_rate: f64, freq_hz: f64, gain_db: f64) {
        let (b0, b1, b2, a1, a2) = low_shelf_coeffs(sample_rate, freq_hz, gain_db);
        self.b0 = b0;
        self.b1 = b1;
        self.b2 = b2;
        self.a1 = a1;
        self.a2 = a2;
    }

    /// Peaking EQ band. RBJ cookbook.
    pub fn peaking(sample_rate: f64, freq_hz: f64, q: f64, gain_db: f64) -> Self {
        let (b0, b1, b2, a1, a2) = peaking_coeffs(sample_rate, freq_hz, q, gain_db);
        Self {
            b0,
            b1,
            b2,
            a1,
            a2,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    pub fn set_peaking(&mut self, sample_rate: f64, freq_hz: f64, q: f64, gain_db: f64) {
        let (b0, b1, b2, a1, a2) = peaking_coeffs(sample_rate, freq_hz, q, gain_db);
        self.b0 = b0;
        self.b1 = b1;
        self.b2 = b2;
        self.a1 = a1;
        self.a2 = a2;
    }

    /// High-shelf, RBJ cookbook. Shelf slope S = 1.0.
    pub fn high_shelf(sample_rate: f64, freq_hz: f64, gain_db: f64) -> Self {
        let (b0, b1, b2, a1, a2) = high_shelf_coeffs(sample_rate, freq_hz, gain_db);
        Self {
            b0,
            b1,
            b2,
            a1,
            a2,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    pub fn set_high_shelf(&mut self, sample_rate: f64, freq_hz: f64, gain_db: f64) {
        let (b0, b1, b2, a1, a2) = high_shelf_coeffs(sample_rate, freq_hz, gain_db);
        self.b0 = b0;
        self.b1 = b1;
        self.b2 = b2;
        self.a1 = a1;
        self.a2 = a2;
    }
}

fn low_shelf_coeffs(sample_rate: f64, freq_hz: f64, gain_db: f64) -> (f64, f64, f64, f64, f64) {
    let freq = freq_hz.clamp(1.0, sample_rate * 0.49);
    let w0 = 2.0 * std::f64::consts::PI * freq / sample_rate;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let a_lin = 10.0_f64.powf(gain_db / 40.0);
    // Shelf slope S = 1.0
    let alpha = sin_w0 / 2.0 * ((a_lin + 1.0 / a_lin) * (1.0 / 1.0 - 1.0) + 2.0).sqrt();
    let a0 = (a_lin + 1.0) + (a_lin - 1.0) * cos_w0 + 2.0 * a_lin.sqrt() * alpha;
    let b0 = a_lin * ((a_lin + 1.0) - (a_lin - 1.0) * cos_w0 + 2.0 * a_lin.sqrt() * alpha) / a0;
    let b1 = 2.0 * a_lin * ((a_lin - 1.0) - (a_lin + 1.0) * cos_w0) / a0;
    let b2 = a_lin * ((a_lin + 1.0) - (a_lin - 1.0) * cos_w0 - 2.0 * a_lin.sqrt() * alpha) / a0;
    let a1 = -2.0 * ((a_lin - 1.0) + (a_lin + 1.0) * cos_w0) / a0;
    let a2 = ((a_lin + 1.0) + (a_lin - 1.0) * cos_w0 - 2.0 * a_lin.sqrt() * alpha) / a0;
    (b0, b1, b2, a1, a2)
}

fn peaking_coeffs(sample_rate: f64, freq_hz: f64, q: f64, gain_db: f64) -> (f64, f64, f64, f64, f64) {
    let freq = freq_hz.clamp(1.0, sample_rate * 0.49);
    let w0 = 2.0 * std::f64::consts::PI * freq / sample_rate;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let a_lin = 10.0_f64.powf(gain_db / 40.0);
    let alpha = sin_w0 / (2.0 * q.max(0.001));
    let a0 = 1.0 + alpha / a_lin;
    let b0 = (1.0 + alpha * a_lin) / a0;
    let b1 = (-2.0 * cos_w0) / a0;
    let b2 = (1.0 - alpha * a_lin) / a0;
    let a1 = (-2.0 * cos_w0) / a0;
    let a2 = (1.0 - alpha / a_lin) / a0;
    (b0, b1, b2, a1, a2)
}

fn high_shelf_coeffs(sample_rate: f64, freq_hz: f64, gain_db: f64) -> (f64, f64, f64, f64, f64) {
    let freq = freq_hz.clamp(1.0, sample_rate * 0.49);
    let w0 = 2.0 * std::f64::consts::PI * freq / sample_rate;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let a_lin = 10.0_f64.powf(gain_db / 40.0);
    let alpha = sin_w0 / 2.0 * ((a_lin + 1.0 / a_lin) * (1.0 / 1.0 - 1.0) + 2.0).sqrt();
    let a0 = (a_lin + 1.0) - (a_lin - 1.0) * cos_w0 + 2.0 * a_lin.sqrt() * alpha;
    let b0 = a_lin * ((a_lin + 1.0) + (a_lin - 1.0) * cos_w0 + 2.0 * a_lin.sqrt() * alpha) / a0;
    let b1 = -2.0 * a_lin * ((a_lin - 1.0) + (a_lin + 1.0) * cos_w0) / a0;
    let b2 = a_lin * ((a_lin + 1.0) + (a_lin - 1.0) * cos_w0 - 2.0 * a_lin.sqrt() * alpha) / a0;
    let a1 = 2.0 * ((a_lin - 1.0) - (a_lin + 1.0) * cos_w0) / a0;
    let a2 = ((a_lin + 1.0) - (a_lin - 1.0) * cos_w0 - 2.0 * a_lin.sqrt() * alpha) / a0;
    (b0, b1, b2, a1, a2)
}
```

(Note for the implementer: the `alpha` formula above uses `S = 1.0` for shelves: `alpha = sin(w0)/2 * sqrt((A + 1/A)*(1/S - 1) + 2)` with `S = 1.0` simplifying to `alpha = sin(w0)/2 * sqrt(2)`. The expression `((a_lin + 1.0 / a_lin) * (1.0 / 1.0 - 1.0) + 2.0).sqrt()` collapses to `sqrt(2)`. Kept literal to make the formula traceable to the cookbook.)

- [ ] **Step 4: Verify pass**

Run: `cd engine && cargo test --test eq_test`
Expected: existing tests still pass; new tests all pass.

- [ ] **Step 5: Commit**

```
git add engine/src/eq.rs engine/tests/eq_test.rs
git commit -m "feat(engine): low_shelf/peaking/high_shelf biquad constructors + set_* (sub-project 2, task 1)"
```

---

### Task 2: `AutomationTarget::Eq{Low,Mid,High}`

**Files:**
- Modify: `engine/src/automation.rs`
- Modify: `engine/tests/automation_test.rs`

- [ ] **Step 1: Write the failing tests**

Append to `engine/tests/automation_test.rs`:

```rust
#[test]
fn eq_low_step_installs_to_value() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::EqLow, 0.0, -40.0, Curve::Step, 0, 1));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.eq_low_db - (-40.0)).abs() < 1e-9);
    assert!(q.is_empty());
}

#[test]
fn eq_mid_clamped_below_minus_40() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::EqMid, 0.0, -100.0, Curve::Step, 0, 1));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.eq_mid_db - (-40.0)).abs() < 1e-9);
}

#[test]
fn eq_high_clamped_above_plus_6() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::EqHigh, 0.0, 20.0, Curve::Step, 0, 1));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.eq_high_db - 6.0).abs() < 1e-9);
}

#[test]
fn eq_low_linear_midpoint() {
    let a = ramp(AutomationTarget::EqLow, 0.0, -40.0, Curve::Linear, 0, 100);
    let v = a.sample_value(50).unwrap();
    assert!((v - (-20.0)).abs() < 1e-9);
}
```

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test automation_test`
Expected: FAIL — `EqLow`/`EqMid`/`EqHigh` variants and `DeckState.eq_*_db` fields don't exist.

- [ ] **Step 3: Add the variants and apply branches**

Edit `engine/src/automation.rs`. Replace the `AutomationTarget` enum:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomationTarget {
    Fader,
    Tempo,
    EqLow,
    EqMid,
    EqHigh,
}
```

Replace the `apply_value` function:

```rust
fn apply_value(target: AutomationTarget, v: f64, state: &mut crate::deck::DeckState) {
    match target {
        AutomationTarget::Fader => {
            state.fader = v.clamp(0.0, 1.0) as f32;
        }
        AutomationTarget::Tempo => {
            state.tempo = v;
        }
        AutomationTarget::EqLow => {
            state.eq_low_db = v.clamp(-40.0, 6.0);
        }
        AutomationTarget::EqMid => {
            state.eq_mid_db = v.clamp(-40.0, 6.0);
        }
        AutomationTarget::EqHigh => {
            state.eq_high_db = v.clamp(-40.0, 6.0);
        }
    }
}
```

(Compilation will fail until Task 3 adds the fields to `DeckState`. That's expected — continue.)

- [ ] **Step 4: Commit (compiles + tests deferred to Task 3 completion)**

This task and Task 3 land together because the test file needs both. Commit Task 3's content alongside this one — see Task 3's final commit step.

---

### Task 3: `DeckState` EQ fields + `Deck` biquads + `cue/unload` reset

**Files:**
- Modify: `engine/src/deck.rs`
- Modify: `engine/tests/deck_test.rs`

- [ ] **Step 1: Add the dB fields and band constants**

Edit `engine/src/deck.rs`. Replace the `DeckState` struct:

```rust
#[derive(Clone, Debug)]
pub struct DeckState {
    pub fader: f32,
    pub tempo: f64,
    pub eq_low_db: f64,
    pub eq_mid_db: f64,
    pub eq_high_db: f64,
    pub source_path: Option<String>,
    pub play_state: PlayState,
}

impl Default for DeckState {
    fn default() -> Self {
        Self {
            fader: 0.0,
            tempo: 1.0,
            eq_low_db: 0.0,
            eq_mid_db: 0.0,
            eq_high_db: 0.0,
            source_path: None,
            play_state: PlayState::Stopped,
        }
    }
}
```

Add module-private constants near the top of the impl block (above `Deck`):

```rust
const EQ_LOW_FREQ: f64 = 200.0;
const EQ_MID_FREQ: f64 = 1000.0;
const EQ_MID_Q: f64 = 1.0;
const EQ_HIGH_FREQ: f64 = 5000.0;
```

- [ ] **Step 2: Add the biquads to `Deck`**

Edit `Deck`'s field list. Add (after `ramp_materialized_for`):

```rust
eq_low_l: crate::eq::Biquad,
eq_low_r: crate::eq::Biquad,
eq_mid_l: crate::eq::Biquad,
eq_mid_r: crate::eq::Biquad,
eq_high_l: crate::eq::Biquad,
eq_high_r: crate::eq::Biquad,
last_eq_low_db: f64,
last_eq_mid_db: f64,
last_eq_high_db: f64,
```

Edit `Deck::new`. After initializing the existing fields, add the biquads at 0 dB (passthrough) and the last-applied cache:

```rust
let sr = SAMPLE_RATE as f64;
let eq_low_l = crate::eq::Biquad::low_shelf(sr, EQ_LOW_FREQ, 0.0);
let eq_low_r = crate::eq::Biquad::low_shelf(sr, EQ_LOW_FREQ, 0.0);
let eq_mid_l = crate::eq::Biquad::peaking(sr, EQ_MID_FREQ, EQ_MID_Q, 0.0);
let eq_mid_r = crate::eq::Biquad::peaking(sr, EQ_MID_FREQ, EQ_MID_Q, 0.0);
let eq_high_l = crate::eq::Biquad::high_shelf(sr, EQ_HIGH_FREQ, 0.0);
let eq_high_r = crate::eq::Biquad::high_shelf(sr, EQ_HIGH_FREQ, 0.0);
Self {
    // ... existing fields ...
    ramp_materialized_for: None,
    eq_low_l,
    eq_low_r,
    eq_mid_l,
    eq_mid_r,
    eq_high_l,
    eq_high_r,
    last_eq_low_db: 0.0,
    last_eq_mid_db: 0.0,
    last_eq_high_db: 0.0,
}
```

Edit `Deck::unload`. Add after the existing field resets:

```rust
self.state.eq_low_db = 0.0;
self.state.eq_mid_db = 0.0;
self.state.eq_high_db = 0.0;
self.last_eq_low_db = 0.0;
self.last_eq_mid_db = 0.0;
self.last_eq_high_db = 0.0;
let sr = SAMPLE_RATE as f64;
self.eq_low_l = crate::eq::Biquad::low_shelf(sr, EQ_LOW_FREQ, 0.0);
self.eq_low_r = crate::eq::Biquad::low_shelf(sr, EQ_LOW_FREQ, 0.0);
self.eq_mid_l = crate::eq::Biquad::peaking(sr, EQ_MID_FREQ, EQ_MID_Q, 0.0);
self.eq_mid_r = crate::eq::Biquad::peaking(sr, EQ_MID_FREQ, EQ_MID_Q, 0.0);
self.eq_high_l = crate::eq::Biquad::high_shelf(sr, EQ_HIGH_FREQ, 0.0);
self.eq_high_r = crate::eq::Biquad::high_shelf(sr, EQ_HIGH_FREQ, 0.0);
```

Edit `Deck::cue`. After the existing field assignments, before `Ok(())`, add the same reset block (factor into a helper if duplication looks ugly; otherwise inline both):

```rust
self.state.eq_low_db = 0.0;
self.state.eq_mid_db = 0.0;
self.state.eq_high_db = 0.0;
self.last_eq_low_db = 0.0;
self.last_eq_mid_db = 0.0;
self.last_eq_high_db = 0.0;
let sr = SAMPLE_RATE as f64;
self.eq_low_l = crate::eq::Biquad::low_shelf(sr, EQ_LOW_FREQ, 0.0);
self.eq_low_r = crate::eq::Biquad::low_shelf(sr, EQ_LOW_FREQ, 0.0);
self.eq_mid_l = crate::eq::Biquad::peaking(sr, EQ_MID_FREQ, EQ_MID_Q, 0.0);
self.eq_mid_r = crate::eq::Biquad::peaking(sr, EQ_MID_FREQ, EQ_MID_Q, 0.0);
self.eq_high_l = crate::eq::Biquad::high_shelf(sr, EQ_HIGH_FREQ, 0.0);
self.eq_high_r = crate::eq::Biquad::high_shelf(sr, EQ_HIGH_FREQ, 0.0);
```

- [ ] **Step 3: Verify the project builds (no behavior change yet)**

Run: `cd engine && cargo build`
Expected: clean build. Existing tests should still pass:

Run: `cd engine && cargo test --test automation_test --test deck_test`
Expected: 17 + 10 (Task 2 tests now compile) tests pass. The deck tests don't yet exercise EQ — they should still pass because EQ at 0 dB is passthrough.

- [ ] **Step 4: Commit Task 2 + Task 3 together**

```
git add engine/src/automation.rs engine/src/deck.rs engine/tests/automation_test.rs
git commit -m "feat(engine): EqLow/EqMid/EqHigh automation targets + per-deck biquad chain (sub-project 2, tasks 2-3)"
```

---

### Task 4: `Deck::pull` applies the EQ chain

**Files:**
- Modify: `engine/src/deck.rs`
- Modify: `engine/tests/deck_test.rs`

- [ ] **Step 1: Write a failing test for the EQ chain**

Append to `engine/tests/deck_test.rs`:

```rust
#[tokio::test]
async fn eq_low_step_minus_40_kills_low_frequency_audio() {
    // Setup: cue a fixture, schedule EqLow Step to -40 dB before any audio plays.
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.play();
    d.test_set_fader(1.0);
    d.schedule(Automation {
        target: AutomationTarget::EqLow,
        t_start_frames: 0,
        t_end_frames: 1,
        from: -40.0,
        to: -40.0,
        curve: Curve::Step,
    });
    // Drop the first tick (decoder priming) so we're well into the body of the
    // 440 Hz fixture before measuring. Then measure RMS of two ticks of audio.
    let _ = d.pull(4096).await.unwrap();
    let kill_low = d.pull(8192).await.unwrap();
    let r_killed = rms_stereo(&kill_low);

    // Tear down and rerun with NO EQ automation to get the baseline RMS.
    let mut d2 = Deck::new(DeckId::A);
    d2.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d2.play();
    d2.test_set_fader(1.0);
    let _ = d2.pull(4096).await.unwrap();
    let baseline = d2.pull(8192).await.unwrap();
    let r_baseline = rms_stereo(&baseline);

    // 440 Hz is above the 200 Hz low-shelf knee but the shelf still has SOME
    // effect (it's a shelf, not a brick wall). Expect the killed RMS to be
    // noticeably (>10%) lower than baseline. Use 0.85 ratio so tests are
    // robust to slight RMS variation between ticks.
    assert!(
        r_killed < r_baseline * 0.85,
        "expected EqLow=-40dB to attenuate; killed {r_killed} vs baseline {r_baseline}"
    );
    assert!((d.state().eq_low_db - (-40.0)).abs() < 1e-9);
}

fn rms_stereo(samples: &[f32]) -> f32 {
    if samples.is_empty() { return 0.0; }
    let s: f32 = samples.iter().map(|x| x * x).sum();
    (s / samples.len() as f32).sqrt()
}
```

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test deck_test eq_low_step_minus_40_kills_low_frequency_audio`
Expected: FAIL — the EQ chain isn't applied yet, so killed_RMS ≈ baseline_RMS.

- [ ] **Step 3: Apply the EQ chain in `Deck::pull`**

Edit `engine/src/deck.rs`. Replace the `Deck::pull` method:

```rust
pub async fn pull(&mut self, n_frames: usize) -> Result<Vec<f32>> {
    let start_cursor = self.frame_cursor;
    self.maybe_prepare_tempo_ramp(start_cursor).await?;
    self.frame_cursor += n_frames as u64;

    if !matches!(self.state.play_state, PlayState::Playing) {
        self.automations.step(self.frame_cursor, &mut self.state);
        return Ok(vec![0.0; n_frames * (CHANNELS as usize)]);
    }

    // Pre-step: apply any automations at start_cursor so the biquad coefficient
    // recompute below uses the post-step EQ gains.
    self.automations.step(start_cursor, &mut self.state);
    self.recompute_eq_if_dirty();

    // Drain source queue.
    let need_samples = n_frames * (CHANNELS as usize);
    let mut out: Vec<f32> = Vec::with_capacity(need_samples);
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

    // Per-sample: evaluate automations, apply EQ chain (low → mid → high),
    // then apply fader.
    for f in 0..n_frames {
        let cursor = start_cursor + f as u64;
        self.automations.step(cursor, &mut self.state);
        let li = f * 2;
        let ri = li + 1;
        let l = self.eq_high_l.process(self.eq_mid_l.process(self.eq_low_l.process(out[li])));
        let r = self.eq_high_r.process(self.eq_mid_r.process(self.eq_low_r.process(out[ri])));
        let g = self.state.fader;
        out[li] = l * g;
        out[ri] = r * g;
    }
    // Final step for automations finishing at frame_cursor.
    self.automations.step(self.frame_cursor, &mut self.state);

    let emitted_seconds = n_frames as f64 / SAMPLE_RATE as f64;
    self.source_position_seconds += emitted_seconds * self.state.tempo;
    Ok(out)
}

fn recompute_eq_if_dirty(&mut self) {
    let sr = SAMPLE_RATE as f64;
    if (self.state.eq_low_db - self.last_eq_low_db).abs() > f64::EPSILON {
        self.eq_low_l.set_low_shelf(sr, EQ_LOW_FREQ, self.state.eq_low_db);
        self.eq_low_r.set_low_shelf(sr, EQ_LOW_FREQ, self.state.eq_low_db);
        self.last_eq_low_db = self.state.eq_low_db;
    }
    if (self.state.eq_mid_db - self.last_eq_mid_db).abs() > f64::EPSILON {
        self.eq_mid_l.set_peaking(sr, EQ_MID_FREQ, EQ_MID_Q, self.state.eq_mid_db);
        self.eq_mid_r.set_peaking(sr, EQ_MID_FREQ, EQ_MID_Q, self.state.eq_mid_db);
        self.last_eq_mid_db = self.state.eq_mid_db;
    }
    if (self.state.eq_high_db - self.last_eq_high_db).abs() > f64::EPSILON {
        self.eq_high_l.set_high_shelf(sr, EQ_HIGH_FREQ, self.state.eq_high_db);
        self.eq_high_r.set_high_shelf(sr, EQ_HIGH_FREQ, self.state.eq_high_db);
        self.last_eq_high_db = self.state.eq_high_db;
    }
}
```

- [ ] **Step 4: Verify the test passes**

Run: `cd engine && cargo test --test deck_test`
Expected: 11 tests pass (10 original + 1 new).

- [ ] **Step 5: Commit**

```
git add engine/src/deck.rs engine/tests/deck_test.rs
git commit -m "feat(engine): apply 3-band EQ chain inside Deck::pull (sub-project 2, task 4)"
```

---

### Task 5: `BassSwap` recipe (default + bass-light + bass-heavy)

**Files:**
- Modify: `engine/src/recipe.rs`
- Modify: `engine/tests/recipe_test.rs`

- [ ] **Step 1: Write the failing tests**

Append to `engine/tests/recipe_test.rs`:

```rust
use dj_engine::recipe::BassSwap;
use dj_engine::sidecar::BandValues;

fn track_with_bass(bpm: f64, bass: f64) -> LoadedTrack {
    let mut sc = sidecar_with(bpm, 280.0);
    sc.band_energy_means = Some(BandValues { bass, mid: 0.5, high: 0.5 });
    LoadedTrack {
        sidecar_path: PathBuf::new(),
        sidecar: sc,
    }
}

fn standard_plan() -> TransitionPlan {
    TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: true,
    }
}

fn ctx_for<'a>(outgoing: &'a LoadedTrack, incoming: &'a LoadedTrack, plan: &'a TransitionPlan) -> TransitionContext<'a> {
    TransitionContext {
        outgoing,
        incoming,
        plan,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    }
}

#[test]
fn bass_swap_default_emits_fader_plus_eq_low_automations() {
    let out_track = track_with_bass(120.0, 0.4);
    let in_track = track_with_bass(120.0, 0.4);
    let plan = standard_plan();
    let ctx = ctx_for(&out_track, &in_track, &plan);
    let r = BassSwap.build(&ctx);
    assert_eq!(r.outgoing_automations.len(), 2);
    assert!(r.outgoing_automations.iter().any(|a|
        a.target == AutomationTarget::Fader && a.curve == Curve::EqualPowerOut));
    assert!(r.outgoing_automations.iter().any(|a|
        a.target == AutomationTarget::EqLow && a.curve == Curve::Linear && a.to == -40.0));

    assert_eq!(r.incoming_automations.len(), 4);
    let in_step = r.incoming_automations.iter()
        .find(|a| a.target == AutomationTarget::EqLow && a.curve == Curve::Step)
        .expect("incoming init kill");
    assert_eq!(in_step.from, -40.0);
    assert_eq!(in_step.to, -40.0);
    let in_release = r.incoming_automations.iter()
        .find(|a| a.target == AutomationTarget::EqLow && a.curve == Curve::Linear)
        .expect("incoming release");
    assert_eq!(in_release.from, -40.0);
    assert_eq!(in_release.to, 0.0);
}

#[test]
fn bass_swap_bass_light_pair_skips_eq() {
    let out_track = track_with_bass(120.0, 0.2);
    let in_track = track_with_bass(120.0, 0.25);
    let plan = standard_plan();
    let ctx = ctx_for(&out_track, &in_track, &plan);
    let r = BassSwap.build(&ctx);
    // No EqLow automations on either deck.
    assert!(!r.outgoing_automations.iter().any(|a| a.target == AutomationTarget::EqLow));
    assert!(!r.incoming_automations.iter().any(|a| a.target == AutomationTarget::EqLow));
    // Shape matches EqualPowerCrossfade: outgoing Fader; incoming Fader + Tempo Step.
    assert_eq!(r.outgoing_automations.len(), 1);
    assert_eq!(r.incoming_automations.len(), 2);
}

#[test]
fn bass_swap_bass_heavy_pair_uses_wider_window() {
    let out_track = track_with_bass(120.0, 0.7);
    let in_track = track_with_bass(120.0, 0.8);
    let plan = standard_plan();
    let ctx = ctx_for(&out_track, &in_track, &plan);
    let r = BassSwap.build(&ctx);
    let bar_seconds = (60.0_f64 / 120.0) * 4.0;
    let l_frames = (plan.length_bars as f64 * bar_seconds * SR as f64) as u64;
    let out_kill = r.outgoing_automations.iter()
        .find(|a| a.target == AutomationTarget::EqLow && a.curve == Curve::Linear)
        .expect("outgoing kill");
    let in_release = r.incoming_automations.iter()
        .find(|a| a.target == AutomationTarget::EqLow && a.curve == Curve::Linear)
        .expect("incoming release");
    // Wide window: outgoing kill [0.30 L .. 0.48 L], incoming release [0.52 L .. 0.70 L].
    assert!((out_kill.t_start_frames as f64 - 0.30 * l_frames as f64).abs() < 10.0);
    assert!((out_kill.t_end_frames as f64 - 0.48 * l_frames as f64).abs() < 10.0);
    assert!((in_release.t_start_frames as f64 - 0.52 * l_frames as f64).abs() < 10.0);
    assert!((in_release.t_end_frames as f64 - 0.70 * l_frames as f64).abs() < 10.0);
}

#[test]
fn bass_swap_missing_band_energy_falls_back_to_default() {
    // band_energy_means = None on either track → treat as 0.5 → default window.
    let mut out_track = track_with_bass(120.0, 0.4);
    out_track.sidecar.band_energy_means = None;
    let mut in_track = track_with_bass(120.0, 0.4);
    in_track.sidecar.band_energy_means = None;
    let plan = standard_plan();
    let ctx = ctx_for(&out_track, &in_track, &plan);
    let r = BassSwap.build(&ctx);
    // Has EqLow automations (not skipped) and uses default window.
    let bar_seconds = (60.0_f64 / 120.0) * 4.0;
    let l_frames = (plan.length_bars as f64 * bar_seconds * SR as f64) as u64;
    let out_kill = r.outgoing_automations.iter()
        .find(|a| a.target == AutomationTarget::EqLow && a.curve == Curve::Linear)
        .expect("outgoing kill");
    // Default window: outgoing kill [0.35 L .. 0.48 L].
    assert!((out_kill.t_start_frames as f64 - 0.35 * l_frames as f64).abs() < 10.0);
    assert!((out_kill.t_end_frames as f64 - 0.48 * l_frames as f64).abs() < 10.0);
}

#[test]
fn pick_recipe_routes_use_eq_swap_to_bass_swap() {
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.0,
        incoming_crossfade_start_seconds: 0.0,
        length_bars: 16,
        use_eq_swap: true,
    };
    assert_eq!(pick_recipe(&plan), RecipeChoice::BassSwap);
}
```

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test recipe_test`
Expected: FAIL — `BassSwap` doesn't exist, `RecipeChoice::BassSwap` doesn't exist.

- [ ] **Step 3: Implement `BassSwap` + extend `pick_recipe`**

Edit `engine/src/recipe.rs`. Append the recipe + extend pick_recipe (replace its body):

```rust
pub struct BassSwap;

impl Recipe for BassSwap {
    fn name(&self) -> &'static str {
        "bass_swap_crossfade"
    }

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

        // Bass-content adaptation.
        let outgoing_bass = ctx
            .outgoing
            .sidecar
            .band_energy_means
            .as_ref()
            .map(|b| b.bass)
            .unwrap_or(0.5);
        let incoming_bass = ctx
            .incoming
            .sidecar
            .band_energy_means
            .as_ref()
            .map(|b| b.bass)
            .unwrap_or(0.5);

        // Both bass-light → just an equal-power crossfade, no EQ shenanigans.
        if outgoing_bass < 0.3 && incoming_bass < 0.3 {
            return RecipeOutput {
                outgoing_automations: vec![outgoing_fader],
                incoming_automations: vec![incoming_fader, incoming_tempo_step],
                total_duration_frames: length_frames,
            };
        }

        // Both bass-heavy → wider gap so big basslines clear before the new one enters.
        let (kill_start, kill_end, rel_start, rel_end) =
            if outgoing_bass > 0.5 && incoming_bass > 0.5 {
                (0.30, 0.48, 0.52, 0.70)
            } else {
                (0.35, 0.48, 0.52, 0.65)
            };

        let l = length_frames as f64;
        let outgoing_eq_low_kill = Automation {
            target: AutomationTarget::EqLow,
            t_start_frames: t0 + (kill_start * l) as u64,
            t_end_frames: t0 + (kill_end * l) as u64,
            from: 0.0,
            to: -40.0,
            curve: Curve::Linear,
        };
        let incoming_eq_low_init = Automation {
            target: AutomationTarget::EqLow,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: -40.0,
            to: -40.0,
            curve: Curve::Step,
        };
        let incoming_eq_low_release = Automation {
            target: AutomationTarget::EqLow,
            t_start_frames: t0 + (rel_start * l) as u64,
            t_end_frames: t0 + (rel_end * l) as u64,
            from: -40.0,
            to: 0.0,
            curve: Curve::Linear,
        };

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader, outgoing_eq_low_kill],
            incoming_automations: vec![
                incoming_fader,
                incoming_tempo_step,
                incoming_eq_low_init,
                incoming_eq_low_release,
            ],
            total_duration_frames: length_frames,
        }
    }
}
```

Replace the existing `RecipeChoice` enum and `pick_recipe`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecipeChoice {
    EqualPower,
    HardCut,
    BassSwap,
}

pub fn pick_recipe(plan: &TransitionPlan) -> RecipeChoice {
    if plan.length_bars == 0 {
        RecipeChoice::HardCut
    } else if plan.use_eq_swap {
        RecipeChoice::BassSwap
    } else {
        RecipeChoice::EqualPower
    }
}
```

- [ ] **Step 4: Verify the tests pass**

Run: `cd engine && cargo test --test recipe_test`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/recipe.rs engine/tests/recipe_test.rs
git commit -m "feat(engine): BassSwap recipe with bass-content adaptation (sub-project 2, task 5)"
```

---

### Task 6: `main.rs` wires `BassSwap` + updates `transition_type_label`

**Files:**
- Modify: `engine/src/main.rs`

- [ ] **Step 1: Update the recipe match in the main loop**

Edit `engine/src/main.rs`. In the loop, find the `let handle = match choice { ... }` block. Replace with:

```rust
let handle = match choice {
    dj_engine::recipe::RecipeChoice::EqualPower => {
        dj_engine::scheduler::TransitionScheduler::arm(
            &mut mixer,
            current_deck,
            other_deck,
            &current,
            &next_track,
            &plan,
            &dj_engine::recipe::EqualPowerCrossfade,
        )
        .await?
    }
    dj_engine::recipe::RecipeChoice::HardCut => {
        dj_engine::scheduler::TransitionScheduler::arm(
            &mut mixer,
            current_deck,
            other_deck,
            &current,
            &next_track,
            &plan,
            &dj_engine::recipe::HardCut,
        )
        .await?
    }
    dj_engine::recipe::RecipeChoice::BassSwap => {
        dj_engine::scheduler::TransitionScheduler::arm(
            &mut mixer,
            current_deck,
            other_deck,
            &current,
            &next_track,
            &plan,
            &dj_engine::recipe::BassSwap,
        )
        .await?
    }
};
```

Also update the `let recipe_name` block above it:

```rust
let recipe_name: &str = match choice {
    dj_engine::recipe::RecipeChoice::EqualPower => "equal_power_crossfade",
    dj_engine::recipe::RecipeChoice::HardCut => "hard_cut",
    dj_engine::recipe::RecipeChoice::BassSwap => "bass_swap_crossfade",
};
```

Replace `transition_type_label`:

```rust
fn transition_type_label(length_bars: usize, use_eq_swap: bool) -> &'static str {
    match (length_bars, use_eq_swap) {
        (0, _) => "hard_cut",
        (8, false) => "quick_cut",
        (8, true) => "bass_swap_quick_cut",
        (16, false) => "standard_blend",
        (16, true) => "bass_swap_standard_blend",
        (32, false) => "long_blend",
        (32, true) => "bass_swap_long_blend",
        _ => "custom_blend",
    }
}
```

- [ ] **Step 2: Verify build + tests + clippy**

Run sequentially:

```
cd engine
cargo build --release
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: all clean.

- [ ] **Step 3: Commit**

```
git add engine/src/main.rs
git commit -m "feat(engine): main.rs routes BassSwap + transition_type label reflects realized swap (sub-project 2, task 6)"
```

---

### Task 7: Optional — extend `mixer_integration_test` to exercise BassSwap

**Files:**
- Modify: `engine/tests/mixer_integration_test.rs`

- [ ] **Step 1: Add a BassSwap-driven test**

Append a test alongside the existing integration test. It mirrors the existing flow but uses fixtures with high bass content (set via the test track helper) and the `BassSwap` recipe directly:

```rust
use dj_engine::recipe::BassSwap;
use dj_engine::sidecar::BandValues;

fn bass_heavy_track(name: &str, dur: f64) -> LoadedTrack {
    let mut t = track(name, dur);
    t.sidecar.band_energy_means = Some(BandValues { bass: 0.7, mid: 0.5, high: 0.5 });
    t
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bass_swap_transition_outputs_continuous_audio() {
    let cap = (SAMPLE_RATE as usize) * 2 * 10;
    let (producer, mut consumer) = make_ring(cap);
    let mut mixer = Mixer::new(producer);

    let outgoing = bass_heavy_track("sine_440_6s.wav", 6.0);
    let incoming = bass_heavy_track("sine_660_6s.wav", 6.0);

    // Same setup as the EqualPower integration test.
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
        use_eq_swap: true,
    };
    let handle = TransitionScheduler::arm(
        &mut mixer,
        DeckId::A,
        DeckId::B,
        &outgoing,
        &incoming,
        &plan,
        &BassSwap,
    )
    .await
    .unwrap();
    tick_until(&mut mixer, handle.end_master_clock_frames).await.unwrap();

    let all = drain.await.unwrap();
    assert!(!all.is_empty());
    let mut zero_run = 0usize;
    let mut max_zero_run = 0usize;
    for s in &all {
        if *s == 0.0 {
            zero_run += 1;
            max_zero_run = max_zero_run.max(zero_run);
        } else {
            zero_run = 0;
        }
    }
    assert!(max_zero_run < 4096, "long silence: {max_zero_run} samples");
}
```

(Note for the implementer: factor the existing test's drain/setup logic into helpers if you find yourself copy-pasting them. If the existing test already has a `drain_to_vec` helper, reuse it; if not, the existing test's body has the pattern — extract it once.)

- [ ] **Step 2: Verify**

Run: `cd engine && cargo test --test mixer_integration_test`
Expected: both tests pass.

- [ ] **Step 3: Commit**

```
git add engine/tests/mixer_integration_test.rs
git commit -m "test(engine): integration test for BassSwap transition (sub-project 2, task 7)"
```

---

### Task 8: Final verification

**Files:** none — verification commands.

- [ ] **Step 1: `cargo fmt`**

Run: `cd engine && cargo fmt --check`
If output: run `cargo fmt && git add -A && git commit -m "chore(engine): fmt after sub-project 2"`.

- [ ] **Step 2: `cargo clippy`**

Run: `cd engine && cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 3: `cargo test`**

Run: `cd engine && cargo test`
Expected: every test passes.

- [ ] **Step 4: `cargo build --release`**

Run: `cd engine && cargo build --release`
Expected: success.

---

## Self-Review notes

Coverage check:
- Spec Component 1 (`eq.rs` shelves + peaking) → Task 1.
- Spec Component 2 (`DeckState` EQ fields + `Deck` biquads + reset) → Task 3.
- Spec Component 3 (`Deck::pull` EQ chain) → Task 4.
- Spec Component 4 (`AutomationTarget::Eq*`) → Task 2.
- Spec Component 5 (`BassSwap` recipe with bass-content adaptation) → Task 5.
- Spec Component 6 (`pick_recipe` route + `RecipeChoice::BassSwap`) → Task 5.
- Spec Component 7 (`main.rs` arm + label) → Task 6.

Type consistency:
- `DeckState.eq_low_db` (f64) used in `automation.rs::apply_value` and `deck.rs::recompute_eq_if_dirty`. Matches.
- `RecipeChoice::BassSwap` used in `recipe.rs::pick_recipe`, `main.rs` match, and `recipe_test.rs`. Matches.

No placeholders or "implement later" steps.
