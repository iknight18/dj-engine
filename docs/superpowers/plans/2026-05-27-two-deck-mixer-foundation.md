# Two-Deck Mixer Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor `engine/` from a single-decoder + transient-mixwindow model to a two-deck tick-driven mixer with persistent Deck A / Deck B objects, an Automation primitive, and a Recipe layer. Ship one volume-crossfade recipe + a hard-cut recipe that together reproduce today's default transition behavior.

**Architecture:** Two persistent `Deck` structs each own an ffmpeg+atempo subprocess and a small state (fader, tempo, position). Automations are timed, curved changes to one of those parameters. A `Recipe` compiles a transition (e.g., 16-bar equal-power crossfade) into a list of automations. A `Mixer` ticks every 4096 frames: pulls from each deck in parallel, sums, pushes to the ring buffer. A `TransitionScheduler` arms the next transition by cueing the incoming deck and scheduling automations.

**Tech Stack:** Rust 2024 edition, `tokio` async, `ringbuf`, `tracing`, ffmpeg via subprocess.

**Spec:** `docs/superpowers/specs/2026-05-27-two-deck-mixer-foundation-design.md`

---

## File structure

| Path | Action | Responsibility |
|---|---|---|
| `engine/src/automation.rs` | CREATE | `Curve`, `Automation`, `AutomationTarget`, `AutomationQueue` |
| `engine/src/recipe.rs` | CREATE | `Recipe` trait, `TransitionContext`, `RecipeOutput`, `EqualPowerCrossfade`, `HardCut`, `pick_recipe` |
| `engine/src/deck.rs` | CREATE | `Deck`, `DeckId`, `DeckState`, `PlayState`, internal `DeckSource` queue, atempo regime handling |
| `engine/src/scheduler.rs` | CREATE | `TransitionScheduler::arm` |
| `engine/src/mixer.rs` | REWRITE | `Mixer` struct with tick loop; remove `play_until_transition_and_crossfade` |
| `engine/src/main.rs` | MODIFY | Wire two Decks + Mixer + scheduler; main loop with role swap |
| `engine/src/lib.rs` | MODIFY | Declare new modules; remove `ffmpeg::mixwindow` from public surface |
| `engine/src/crossfade.rs` | MODIFY | Keep `equal_power_curve`; delete `mix_crossfade`, `mix_crossfade_eq`, `BassMixHint` |
| `engine/src/ffmpeg/mod.rs` | MODIFY | Stop declaring `mixwindow` module publicly |
| `engine/src/ffmpeg/mixwindow.rs` | DELETE-AND-MOVE | Helpers move to private `deck::stretch` module |
| `engine/src/transition.rs` | UNCHANGED | `TransitionPlan` still produced; consumed by `pick_recipe` |
| `engine/src/eq.rs` | MODIFY | `apply_swept_filter` gets `#[allow(dead_code)]` pending sub-project 3 |
| `engine/tests/automation_test.rs` | CREATE | Curve, Automation, AutomationQueue unit tests |
| `engine/tests/recipe_test.rs` | CREATE | `EqualPowerCrossfade::build`, `HardCut::build`, `pick_recipe` |
| `engine/tests/deck_test.rs` | CREATE | Deck lifecycle, pull silence/playing, fader, tempo regime tests |
| `engine/tests/scheduler_test.rs` | CREATE | Arm flow, automations scheduled on right decks |
| `engine/tests/two_deck_mixer_test.rs` | CREATE | End-to-end synthetic-sines integration |
| `engine/tests/crossfade_test.rs` | MODIFY | Remove `mix_crossfade` tests; keep `equal_power_curve` |
| `engine/tests/crossfade_eq_test.rs` | DELETE | All callers removed |
| `engine/tests/mixwindow_ramp_test.rs` | MODIFY | Move into `deck_test.rs` or rename / adjust namespace |
| `engine/tests/mixer_integration_test.rs` | REWRITE | Drive via new `Mixer` API |

---

## Tasks

### Task 1: `automation.rs` — `Curve` enum and `eval`

**Files:**
- Create: `engine/src/automation.rs`
- Create: `engine/tests/automation_test.rs`
- Modify: `engine/src/lib.rs` (add `pub mod automation;`)

- [ ] **Step 1: Add module declaration**

Edit `engine/src/lib.rs` — add the line `pub mod automation;` near the other `pub mod` declarations (alphabetical order between `align` and `buffer`).

- [ ] **Step 2: Write failing test**

Create `engine/tests/automation_test.rs`:

```rust
use dj_engine::automation::Curve;

const EPS: f64 = 1e-9;

#[test]
fn curve_linear_endpoints_and_midpoint() {
    assert!((Curve::Linear.eval(0.0) - 0.0).abs() < EPS);
    assert!((Curve::Linear.eval(0.5) - 0.5).abs() < EPS);
    assert!((Curve::Linear.eval(1.0) - 1.0).abs() < EPS);
}

#[test]
fn curve_linear_clamps_out_of_range() {
    assert!((Curve::Linear.eval(-0.5) - 0.0).abs() < EPS);
    assert!((Curve::Linear.eval(1.5) - 1.0).abs() < EPS);
}

#[test]
fn curve_equal_power_in() {
    assert!((Curve::EqualPowerIn.eval(0.0) - 0.0).abs() < EPS);
    assert!((Curve::EqualPowerIn.eval(1.0) - 1.0).abs() < EPS);
    // sin(π/4) = sqrt(2)/2
    let mid = Curve::EqualPowerIn.eval(0.5);
    assert!((mid - 0.7071067811865476).abs() < 1e-12);
}

#[test]
fn curve_equal_power_out() {
    assert!((Curve::EqualPowerOut.eval(0.0) - 1.0).abs() < EPS);
    assert!((Curve::EqualPowerOut.eval(1.0) - 0.0).abs() < EPS);
    let mid = Curve::EqualPowerOut.eval(0.5);
    assert!((mid - 0.7071067811865476).abs() < 1e-12);
}

#[test]
fn curve_step_jumps_at_one() {
    assert!((Curve::Step.eval(0.0) - 0.0).abs() < EPS);
    assert!((Curve::Step.eval(0.5) - 0.0).abs() < EPS);
    assert!((Curve::Step.eval(0.999) - 0.0).abs() < EPS);
    assert!((Curve::Step.eval(1.0) - 1.0).abs() < EPS);
}

#[test]
fn curve_equal_power_sum_is_constant_power() {
    // sin^2 + cos^2 == 1
    for i in 0..=10 {
        let t = i as f64 / 10.0;
        let a = Curve::EqualPowerIn.eval(t);
        let b = Curve::EqualPowerOut.eval(t);
        assert!((a * a + b * b - 1.0).abs() < 1e-12, "t={t}");
    }
}
```

- [ ] **Step 3: Verify the test fails**

Run: `cd engine && cargo test --test automation_test`
Expected: FAIL — `Curve` is not defined / `automation` module missing.

- [ ] **Step 4: Implement**

Create `engine/src/automation.rs`:

```rust
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
```

- [ ] **Step 5: Verify the tests pass**

Run: `cd engine && cargo test --test automation_test`
Expected: 6 tests pass.

- [ ] **Step 6: Commit**

```
git add engine/src/lib.rs engine/src/automation.rs engine/tests/automation_test.rs
git commit -m "feat(engine): Curve enum with linear/equal-power/step (sub-project 1, task 1)"
```

---

### Task 2: `Automation` struct + `sample_value`

**Files:**
- Modify: `engine/src/automation.rs`
- Modify: `engine/tests/automation_test.rs`

- [ ] **Step 1: Add failing tests**

Append to `engine/tests/automation_test.rs`:

```rust
use dj_engine::automation::{Automation, AutomationTarget};

fn ramp(target: AutomationTarget, from: f64, to: f64, curve: Curve, t0: u64, t1: u64) -> Automation {
    Automation { target, t_start_frames: t0, t_end_frames: t1, from, to, curve }
}

#[test]
fn sample_value_before_start_is_none() {
    let a = ramp(AutomationTarget::Fader, 0.0, 1.0, Curve::Linear, 100, 200);
    assert_eq!(a.sample_value(50), None);
    assert_eq!(a.sample_value(99), None);
}

#[test]
fn sample_value_at_start_is_from() {
    let a = ramp(AutomationTarget::Fader, 0.2, 0.8, Curve::Linear, 100, 200);
    let v = a.sample_value(100).unwrap();
    assert!((v - 0.2).abs() < 1e-9, "got {v}");
}

#[test]
fn sample_value_at_midpoint_linear() {
    let a = ramp(AutomationTarget::Fader, 0.0, 1.0, Curve::Linear, 0, 100);
    let v = a.sample_value(50).unwrap();
    assert!((v - 0.5).abs() < 1e-9, "got {v}");
}

#[test]
fn sample_value_at_or_after_end_is_none() {
    let a = ramp(AutomationTarget::Fader, 0.0, 1.0, Curve::Linear, 0, 100);
    assert_eq!(a.sample_value(100), None);
    assert_eq!(a.sample_value(150), None);
}

#[test]
fn sample_value_equal_power_in_midpoint() {
    let a = ramp(AutomationTarget::Fader, 0.0, 1.0, Curve::EqualPowerIn, 0, 100);
    let v = a.sample_value(50).unwrap();
    assert!((v - 0.7071067811865476).abs() < 1e-12);
}
```

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test automation_test`
Expected: FAIL — `Automation` not defined.

- [ ] **Step 3: Implement**

Append to `engine/src/automation.rs`:

```rust
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
```

- [ ] **Step 4: Verify pass**

Run: `cd engine && cargo test --test automation_test`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/automation.rs engine/tests/automation_test.rs
git commit -m "feat(engine): Automation primitive with sample_value (sub-project 1, task 2)"
```

---

### Task 3: `AutomationQueue` and `step`

**Files:**
- Modify: `engine/src/automation.rs`
- Modify: `engine/tests/automation_test.rs`

- [ ] **Step 1: Forward-declare `DeckState` placeholder**

Append to `engine/src/automation.rs`:

```rust
/// Mutable snapshot of every parameter an Automation can target.
/// Lives in `deck.rs`; declared here so this module can mutate it.
/// **Will be moved to deck.rs in Task 7 and re-exported.**
#[derive(Clone, Debug)]
pub struct DeckStateStub {
    pub fader: f32,
    pub tempo: f64,
}
```

(Temporary; merged with the real `DeckState` in Task 7. Each task is self-contained, so we use a stub for the queue's unit tests now.)

- [ ] **Step 2: Add failing tests**

Append to `engine/tests/automation_test.rs`:

```rust
use dj_engine::automation::{AutomationQueue, DeckStateStub};

fn default_stub() -> DeckStateStub {
    DeckStateStub { fader: 0.0, tempo: 1.0 }
}

#[test]
fn empty_queue_no_change() {
    let mut q = AutomationQueue::new();
    let mut s = default_stub();
    q.step(0, &mut s);
    assert_eq!(s.fader, 0.0);
    assert_eq!(s.tempo, 1.0);
}

#[test]
fn fader_linear_ramp_applies() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::Fader, 0.0, 1.0, Curve::Linear, 0, 100));
    let mut s = default_stub();
    q.step(50, &mut s);
    assert!((s.fader - 0.5).abs() < 1e-6);
}

#[test]
fn fader_finished_leaves_to_installed_and_is_removed() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::Fader, 0.0, 1.0, Curve::Linear, 0, 100));
    let mut s = default_stub();
    q.step(150, &mut s);
    assert!((s.fader - 1.0).abs() < 1e-6);
    assert!(q.is_empty());
}

#[test]
fn future_automation_not_yet_applied() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::Fader, 0.0, 1.0, Curve::Linear, 100, 200));
    let mut s = default_stub();
    q.step(50, &mut s);
    assert_eq!(s.fader, 0.0);
    assert_eq!(q.len(), 1);
}

#[test]
fn tempo_step_installs_to_value() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::Tempo, 1.0, 1.07, Curve::Step, 0, 1));
    let mut s = default_stub();
    q.step(1, &mut s);
    assert!((s.tempo - 1.07).abs() < 1e-9);
    assert!(q.is_empty());
}
```

- [ ] **Step 3: Verify failure**

Run: `cd engine && cargo test --test automation_test`
Expected: FAIL — `AutomationQueue` not defined.

- [ ] **Step 4: Implement**

Append to `engine/src/automation.rs`:

```rust
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
    pub fn step(&mut self, cursor: u64, state: &mut DeckStateStub) {
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

fn apply_value(target: AutomationTarget, v: f64, state: &mut DeckStateStub) {
    match target {
        AutomationTarget::Fader => {
            state.fader = v.clamp(0.0, 1.0) as f32;
        }
        AutomationTarget::Tempo => {
            state.tempo = v;
        }
    }
}
```

- [ ] **Step 5: Verify pass**

Run: `cd engine && cargo test --test automation_test`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```
git add engine/src/automation.rs engine/tests/automation_test.rs
git commit -m "feat(engine): AutomationQueue with step (sub-project 1, task 3)"
```

---

### Task 4: `Recipe` trait skeleton (no impls yet)

**Files:**
- Create: `engine/src/recipe.rs`
- Modify: `engine/src/lib.rs` (add `pub mod recipe;`)

- [ ] **Step 1: Add module declaration**

Edit `engine/src/lib.rs` — add `pub mod recipe;` after `pub mod recency;` (alphabetical).

- [ ] **Step 2: Create the skeleton (compiles, no behavior yet)**

Create `engine/src/recipe.rs`:

```rust
//! Recipe layer: named transition styles compile to lists of Automation primitives.

use crate::automation::Automation;
use crate::library::LoadedTrack;
use crate::transition::TransitionPlan;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeckId {
    A,
    B,
}

pub struct TransitionContext<'a> {
    pub outgoing: &'a LoadedTrack,
    pub incoming: &'a LoadedTrack,
    pub plan: &'a TransitionPlan,
    pub master_clock_frames: u64,
    pub outgoing_deck: DeckId,
    pub incoming_deck: DeckId,
    pub sample_rate: u32,
}

pub struct RecipeOutput {
    pub outgoing_automations: Vec<Automation>,
    pub incoming_automations: Vec<Automation>,
    pub total_duration_frames: u64,
}

pub trait Recipe {
    fn name(&self) -> &'static str;
    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput;
}
```

(Note: `DeckId` is duplicated here temporarily so this module compiles before `deck.rs` exists. Task 7 introduces `deck::DeckId` and Task 14 re-exports it; we'll keep `recipe::DeckId` as a re-export from there.)

- [ ] **Step 3: Verify the project builds**

Run: `cd engine && cargo build`
Expected: clean build, no test changes yet.

- [ ] **Step 4: Commit**

```
git add engine/src/lib.rs engine/src/recipe.rs
git commit -m "feat(engine): Recipe trait + TransitionContext skeleton (sub-project 1, task 4)"
```

---

### Task 5: `EqualPowerCrossfade` recipe

**Files:**
- Modify: `engine/src/recipe.rs`
- Create: `engine/tests/recipe_test.rs`

- [ ] **Step 1: Write failing test**

Create `engine/tests/recipe_test.rs`:

```rust
use dj_engine::automation::{AutomationTarget, Curve};
use dj_engine::library::LoadedTrack;
use dj_engine::recipe::{DeckId, EqualPowerCrossfade, Recipe, TransitionContext};
use dj_engine::sidecar::Sidecar;
use dj_engine::transition::TransitionPlan;

const SR: u32 = 44100;

fn sidecar_with(bpm: f64, cue_out: f64) -> Sidecar {
    Sidecar {
        schema_version: 1,
        analyzer_version: "test".into(),
        source_path: "test.mp3".into(),
        source_mtime: 0.0,
        source_size_bytes: 0,
        duration_seconds: 300.0,
        sample_rate: SR as u64,
        bpm: Some(bpm),
        bpm_confidence: Some(1.0),
        beats_seconds: None,
        downbeats_seconds: None,
        key: None,
        key_label: None,
        key_confidence: None,
        energy_curve: None,
        energy_mean: Some(0.5),
        band_energy_means: None,
        cue_in_seconds: Some(0.0),
        cue_out_seconds: Some(cue_out),
        intro_end_seconds: Some(16.0),
        outro_start_seconds: Some(200.0),
        is_mixable: Some(true),
        mixable_reasons: None,
        error: None,
        ssm: None,
        novelty: None,
        rms_db: None,
    }
}

fn track(bpm: f64) -> LoadedTrack {
    LoadedTrack { sidecar: sidecar_with(bpm, 280.0) }
}

#[test]
fn equal_power_same_bpm_emits_one_out_two_in_no_ramp() {
    let outgoing = track(120.0);
    let incoming = track(120.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
    };
    let ctx = TransitionContext {
        outgoing: &outgoing,
        incoming: &incoming,
        plan: &plan,
        master_clock_frames: 1_000_000,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let r = EqualPowerCrossfade.build(&ctx);
    assert_eq!(r.outgoing_automations.len(), 1);
    assert_eq!(r.outgoing_automations[0].target, AutomationTarget::Fader);
    assert_eq!(r.outgoing_automations[0].curve, Curve::EqualPowerOut);
    assert_eq!(r.outgoing_automations[0].from, 1.0);
    assert_eq!(r.outgoing_automations[0].to, 0.0);

    // Same BPM → no pitch-journey ramp; only Fader-in + Tempo-Step (no-op).
    assert_eq!(r.incoming_automations.len(), 2);
    assert!(r.incoming_automations.iter().any(|a|
        a.target == AutomationTarget::Fader && a.curve == Curve::EqualPowerIn
    ));
    assert!(r.incoming_automations.iter().any(|a|
        a.target == AutomationTarget::Tempo && a.curve == Curve::Step
    ));
}

#[test]
fn equal_power_diff_bpm_emits_ramp() {
    let outgoing = track(120.0);
    let incoming = track(124.0); // +3.3% → triggers ramp
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
    };
    let ctx = TransitionContext {
        outgoing: &outgoing,
        incoming: &incoming,
        plan: &plan,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let r = EqualPowerCrossfade.build(&ctx);
    assert_eq!(r.outgoing_automations.len(), 1);
    assert_eq!(r.incoming_automations.len(), 3);
    let ramp = r.incoming_automations.iter().find(|a|
        a.target == AutomationTarget::Tempo && a.curve == Curve::Linear
    ).expect("expected pitch-journey ramp");
    let expected_ratio = 120.0_f64 / 124.0_f64;
    assert!((ramp.from - expected_ratio).abs() < 1e-9);
    assert!((ramp.to - 1.0).abs() < 1e-9);
}

#[test]
fn equal_power_crossfade_length_matches_plan() {
    let outgoing = track(120.0);
    let incoming = track(120.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
    };
    let ctx = TransitionContext {
        outgoing: &outgoing,
        incoming: &incoming,
        plan: &plan,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let r = EqualPowerCrossfade.build(&ctx);
    let bar_seconds = (60.0_f64 / 120.0) * 4.0;
    let expected_frames = (plan.length_bars as f64 * bar_seconds * SR as f64) as u64;
    let out_fader = &r.outgoing_automations[0];
    assert_eq!(out_fader.t_end_frames - out_fader.t_start_frames, expected_frames);
}
```

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test recipe_test`
Expected: FAIL — `EqualPowerCrossfade` does not exist.

- [ ] **Step 3: Implement**

Append to `engine/src/recipe.rs`:

```rust
use crate::automation::{AutomationTarget, Curve};

const RAMP_BARS: usize = 16;
const RAMP_IDENTITY_EPSILON: f64 = 0.005;

pub struct EqualPowerCrossfade;

impl Recipe for EqualPowerCrossfade {
    fn name(&self) -> &'static str {
        "equal_power_crossfade"
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

        let mut incoming_automations = vec![incoming_fader, incoming_tempo_step];
        let mut total = length_frames;

        if (ratio - 1.0).abs() >= RAMP_IDENTITY_EPSILON {
            let ramp_seconds = RAMP_BARS as f64 * bar_seconds;
            let ramp_frames = (ramp_seconds * sr) as u64;
            // Source-side headroom check: clamp ramp to cue_out remaining.
            let cue_out = ctx.incoming.sidecar.cue_out_seconds.unwrap_or(f64::INFINITY);
            // Source consumed by crossfade post-roll = length_frames / sr * ratio + small overhead.
            // We accept the same approximation as today's mixer.
            let source_after_crossfade = ctx.plan.incoming_crossfade_start_seconds
                + (length_frames as f64 / sr) * ratio;
            let max_ramp_source = (cue_out - source_after_crossfade).max(0.0);
            let max_ramp_output = max_ramp_source / ((ratio + 1.0) / 2.0).max(0.001);
            let effective_seconds = ramp_seconds.min(max_ramp_output);
            if effective_seconds >= bar_seconds {
                let effective_frames = (effective_seconds * sr) as u64;
                incoming_automations.push(Automation {
                    target: AutomationTarget::Tempo,
                    t_start_frames: t1,
                    t_end_frames: t1 + effective_frames,
                    from: ratio,
                    to: 1.0,
                    curve: Curve::Linear,
                });
                total = length_frames + effective_frames;
            }
        }

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader],
            incoming_automations,
            total_duration_frames: total,
        }
    }
}
```

- [ ] **Step 4: Verify pass**

Run: `cd engine && cargo test --test recipe_test`
Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/recipe.rs engine/tests/recipe_test.rs
git commit -m "feat(engine): EqualPowerCrossfade recipe with pitch-journey ramp (sub-project 1, task 5)"
```

---

### Task 6: `HardCut` recipe + `pick_recipe` router

**Files:**
- Modify: `engine/src/recipe.rs`
- Modify: `engine/tests/recipe_test.rs`

- [ ] **Step 1: Write failing tests**

Append to `engine/tests/recipe_test.rs`:

```rust
use dj_engine::recipe::{pick_recipe, HardCut, RecipeChoice};

#[test]
fn hard_cut_emits_step_automations() {
    let outgoing = track(120.0);
    let incoming = track(120.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 0,
        use_eq_swap: false,
    };
    let ctx = TransitionContext {
        outgoing: &outgoing,
        incoming: &incoming,
        plan: &plan,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let r = HardCut.build(&ctx);
    assert_eq!(r.outgoing_automations.len(), 1);
    assert_eq!(r.outgoing_automations[0].curve, Curve::Step);
    assert!(r.incoming_automations
        .iter()
        .any(|a| a.target == AutomationTarget::Fader && a.curve == Curve::Step));
}

#[test]
fn pick_recipe_chooses_hard_cut_for_zero_bars() {
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.0,
        incoming_crossfade_start_seconds: 0.0,
        length_bars: 0,
        use_eq_swap: false,
    };
    assert_eq!(pick_recipe(&plan), RecipeChoice::HardCut);
}

#[test]
fn pick_recipe_chooses_equal_power_for_nonzero_bars() {
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.0,
        incoming_crossfade_start_seconds: 0.0,
        length_bars: 16,
        use_eq_swap: false,
    };
    assert_eq!(pick_recipe(&plan), RecipeChoice::EqualPower);

    let plan_eq = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.0,
        incoming_crossfade_start_seconds: 0.0,
        length_bars: 16,
        use_eq_swap: true, // sub-project 1 still picks EqualPower; EQ swap added later
    };
    assert_eq!(pick_recipe(&plan_eq), RecipeChoice::EqualPower);
}
```

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test recipe_test`
Expected: FAIL — `HardCut`, `pick_recipe`, `RecipeChoice` not defined.

- [ ] **Step 3: Implement**

Append to `engine/src/recipe.rs`:

```rust
pub struct HardCut;

impl Recipe for HardCut {
    fn name(&self) -> &'static str {
        "hard_cut"
    }

    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let sr = ctx.sample_rate as f64;
        let t0 = ctx.master_clock_frames;

        let outgoing_fader_off = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: 1.0,
            to: 0.0,
            curve: Curve::Step,
        };
        let incoming_fader_on = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: 0.0,
            to: 1.0,
            curve: Curve::Step,
        };
        let incoming_tempo_step = Automation {
            target: AutomationTarget::Tempo,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: ratio,
            to: ratio,
            curve: Curve::Step,
        };

        let mut incoming_automations = vec![incoming_fader_on, incoming_tempo_step];
        let mut total = 1u64;

        if (ratio - 1.0).abs() >= RAMP_IDENTITY_EPSILON {
            let ramp_seconds = RAMP_BARS as f64 * bar_seconds;
            let cue_out = ctx.incoming.sidecar.cue_out_seconds.unwrap_or(f64::INFINITY);
            let source_after = ctx.plan.incoming_crossfade_start_seconds;
            let max_ramp_source = (cue_out - source_after).max(0.0);
            let max_ramp_output = max_ramp_source / ((ratio + 1.0) / 2.0).max(0.001);
            let effective_seconds = ramp_seconds.min(max_ramp_output);
            if effective_seconds >= bar_seconds {
                let effective_frames = (effective_seconds * sr) as u64;
                incoming_automations.push(Automation {
                    target: AutomationTarget::Tempo,
                    t_start_frames: t0 + 1,
                    t_end_frames: t0 + 1 + effective_frames,
                    from: ratio,
                    to: 1.0,
                    curve: Curve::Linear,
                });
                total = 1 + effective_frames;
            }
        }

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader_off],
            incoming_automations,
            total_duration_frames: total,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecipeChoice {
    EqualPower,
    HardCut,
}

pub fn pick_recipe(plan: &TransitionPlan) -> RecipeChoice {
    if plan.length_bars == 0 {
        RecipeChoice::HardCut
    } else {
        RecipeChoice::EqualPower
    }
}
```

- [ ] **Step 4: Verify pass**

Run: `cd engine && cargo test --test recipe_test`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/recipe.rs engine/tests/recipe_test.rs
git commit -m "feat(engine): HardCut recipe + pick_recipe router (sub-project 1, task 6)"
```

---

### Task 7: `deck.rs` — `DeckId`, `PlayState`, `DeckState`

**Files:**
- Create: `engine/src/deck.rs`
- Modify: `engine/src/lib.rs` (add `pub mod deck;`)
- Modify: `engine/src/automation.rs` (replace `DeckStateStub` with re-exported `DeckState`)
- Modify: `engine/src/recipe.rs` (replace local `DeckId` with re-export from `deck`)

- [ ] **Step 1: Add module declaration**

Edit `engine/src/lib.rs`: add `pub mod deck;` after `pub mod crossfade;` (alphabetical).

- [ ] **Step 2: Create deck.rs minimal data**

Create `engine/src/deck.rs`:

```rust
//! Deck: one persistent playback channel with its own decoder + state.

use crate::library::LoadedTrack;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum DeckId {
    A,
    B,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayState {
    Stopped,
    Cueing,
    Playing,
}

#[derive(Clone, Debug)]
pub struct DeckState {
    pub fader: f32,
    pub tempo: f64,
    pub source_path: Option<String>,
    pub play_state: PlayState,
}

impl Default for DeckState {
    fn default() -> Self {
        Self {
            fader: 0.0,
            tempo: 1.0,
            source_path: None,
            play_state: PlayState::Stopped,
        }
    }
}
```

- [ ] **Step 3: Replace `DeckStateStub` references with `DeckState`**

Edit `engine/src/automation.rs`:
- Delete the `DeckStateStub` struct.
- Change `AutomationQueue::step` signature: `pub fn step(&mut self, cursor: u64, state: &mut crate::deck::DeckState)`.
- Change `apply_value` signature accordingly.

Edit `engine/tests/automation_test.rs`:
- Change `use dj_engine::automation::{..., DeckStateStub}` → `use dj_engine::deck::DeckState; use dj_engine::deck::PlayState;`
- Update `default_stub` to return `DeckState::default()` and rename to `default_state`.
- Update all `DeckStateStub { fader, tempo }` references to `DeckState { fader, tempo, ..Default::default() }`.

Edit `engine/src/recipe.rs`:
- Remove the local `pub enum DeckId { A, B }` definition.
- Add `pub use crate::deck::DeckId;` at the top.

- [ ] **Step 4: Verify everything still compiles + tests still pass**

Run: `cd engine && cargo test --test automation_test --test recipe_test`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/lib.rs engine/src/deck.rs engine/src/automation.rs engine/src/recipe.rs engine/tests/automation_test.rs
git commit -m "feat(engine): DeckId, PlayState, DeckState in deck.rs (sub-project 1, task 7)"
```

---

### Task 8: `DeckSource` enum + internal pull helper

**Files:**
- Modify: `engine/src/deck.rs`

- [ ] **Step 1: Add internal types**

Append to `engine/src/deck.rs`:

```rust
use crate::ffmpeg::decoder::PlayingDecoder;
use crate::ffmpeg::{BYTES_PER_FRAME, CHANNELS, SAMPLE_RATE};
use crate::error::Result;

/// One PCM source within a deck. The deck plays through a queue of these,
/// advancing to the next when the current is exhausted.
pub(crate) enum DeckSource {
    /// Long-running ffmpeg decoder (with or without atempo).
    Decoder(PlayingDecoder),
    /// Pre-decoded PCM (used for the chunked atempo ramp + splice tail).
    Buffer {
        pcm: Vec<f32>,
        offset_samples: usize,
    },
}

impl DeckSource {
    /// Pull up to `n_frames` of interleaved stereo PCM. Returns a buffer that
    /// may be shorter than `n_frames * 2` if the source is exhausted.
    pub(crate) async fn pull(&mut self, n_frames: usize) -> Result<Vec<f32>> {
        match self {
            DeckSource::Decoder(decoder) => decoder.read_frames(n_frames).await,
            DeckSource::Buffer { pcm, offset_samples } => {
                let need_samples = n_frames * (CHANNELS as usize);
                let end = (*offset_samples + need_samples).min(pcm.len());
                let out = pcm[*offset_samples..end].to_vec();
                *offset_samples = end;
                Ok(out)
            }
        }
    }

    pub(crate) fn is_exhausted(&self) -> bool {
        match self {
            DeckSource::Decoder(_) => false, // can't tell without a read; treated as never exhausted
            DeckSource::Buffer { pcm, offset_samples } => *offset_samples >= pcm.len(),
        }
    }
}
```

- [ ] **Step 2: Verify build**

Run: `cd engine && cargo build`
Expected: clean build.

- [ ] **Step 3: Commit**

```
git add engine/src/deck.rs
git commit -m "feat(engine): DeckSource enum for decoder/buffer playback queue (sub-project 1, task 8)"
```

---

### Task 9: `Deck` struct + accessors + `schedule` + `unload`

**Files:**
- Modify: `engine/src/deck.rs`
- Create: `engine/tests/deck_test.rs`

- [ ] **Step 1: Add the `Deck` struct**

Append to `engine/src/deck.rs`:

```rust
use std::collections::VecDeque;

use crate::automation::{Automation, AutomationQueue};

pub struct Deck {
    id: DeckId,
    state: DeckState,
    automations: AutomationQueue,
    sources: VecDeque<DeckSource>,
    /// Master-clock frame count this deck has emitted.
    frame_cursor: u64,
    /// Current source-time position in seconds (used when respawning decoders).
    source_position_seconds: f64,
    /// cue_out boundary in source-time seconds; respawns clamp to this.
    source_end_seconds: f64,
}

impl Deck {
    pub fn new(id: DeckId) -> Self {
        Self {
            id,
            state: DeckState::default(),
            automations: AutomationQueue::new(),
            sources: VecDeque::new(),
            frame_cursor: 0,
            source_position_seconds: 0.0,
            source_end_seconds: f64::INFINITY,
        }
    }

    pub fn id(&self) -> DeckId {
        self.id
    }

    pub fn state(&self) -> &DeckState {
        &self.state
    }

    pub fn frame_cursor(&self) -> u64 {
        self.frame_cursor
    }

    pub fn schedule(&mut self, a: Automation) {
        self.automations.schedule(a);
    }

    pub async fn unload(&mut self) {
        self.sources.clear();
        self.state.source_path = None;
        self.state.play_state = PlayState::Stopped;
        self.state.fader = 0.0;
        self.state.tempo = 1.0;
        self.source_position_seconds = 0.0;
        self.source_end_seconds = f64::INFINITY;
    }
}
```

- [ ] **Step 2: Write failing tests**

Create `engine/tests/deck_test.rs`:

```rust
use dj_engine::deck::{Deck, DeckId, PlayState};

#[test]
fn new_deck_is_stopped_with_zero_fader_unity_tempo() {
    let d = Deck::new(DeckId::A);
    assert_eq!(d.state().play_state, PlayState::Stopped);
    assert_eq!(d.state().fader, 0.0);
    assert!((d.state().tempo - 1.0).abs() < 1e-9);
    assert_eq!(d.id(), DeckId::A);
    assert_eq!(d.frame_cursor(), 0);
}

#[tokio::test]
async fn unload_resets_state() {
    let mut d = Deck::new(DeckId::B);
    d.unload().await;
    assert_eq!(d.state().play_state, PlayState::Stopped);
    assert!(d.state().source_path.is_none());
}
```

- [ ] **Step 3: Verify**

Run: `cd engine && cargo test --test deck_test`
Expected: tests pass.

- [ ] **Step 4: Commit**

```
git add engine/src/deck.rs engine/tests/deck_test.rs
git commit -m "feat(engine): Deck struct with new/unload/accessors (sub-project 1, task 9)"
```

---

### Task 10: `Deck::cue` — spawn decoder, set Cueing state

**Files:**
- Modify: `engine/src/deck.rs`
- Modify: `engine/tests/deck_test.rs`

- [ ] **Step 1: Write failing test**

Append to `engine/tests/deck_test.rs`:

```rust
use dj_engine::library::LoadedTrack;
use dj_engine::sidecar::Sidecar;
use std::path::PathBuf;

fn fixture_track() -> LoadedTrack {
    // Use a tiny WAV fixture committed under tests/fixtures/.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sine_440_3s.wav");
    LoadedTrack {
        sidecar: Sidecar {
            schema_version: 1,
            analyzer_version: "test".into(),
            source_path: path.to_string_lossy().into_owned(),
            source_mtime: 0.0,
            source_size_bytes: 0,
            duration_seconds: 3.0,
            sample_rate: 44100,
            bpm: Some(120.0),
            bpm_confidence: Some(1.0),
            beats_seconds: None,
            downbeats_seconds: None,
            key: None,
            key_label: None,
            key_confidence: None,
            energy_curve: None,
            energy_mean: Some(0.5),
            band_energy_means: None,
            cue_in_seconds: Some(0.0),
            cue_out_seconds: Some(3.0),
            intro_end_seconds: Some(0.0),
            outro_start_seconds: Some(3.0),
            is_mixable: Some(true),
            mixable_reasons: None,
            error: None,
            ssm: None,
            novelty: None,
            rms_db: None,
        },
    }
}

#[tokio::test]
async fn cue_transitions_to_cueing_state() {
    let mut d = Deck::new(DeckId::A);
    let track = fixture_track();
    d.cue(track, 0.0).await.expect("cue ok");
    assert_eq!(d.state().play_state, PlayState::Cueing);
    assert!(d.state().source_path.is_some());
}
```

- [ ] **Step 2: Create the fixture WAV**

Run from repo root:

```powershell
ffmpeg -hide_banner -loglevel error -f lavfi -i "sine=frequency=440:duration=3:sample_rate=44100" -ac 2 engine/tests/fixtures/sine_440_3s.wav
```

(Create the `tests/fixtures/` directory first if it doesn't exist: `mkdir engine/tests/fixtures`.)

- [ ] **Step 3: Verify the test fails**

Run: `cd engine && cargo test --test deck_test cue_transitions_to_cueing_state`
Expected: FAIL — `cue` method not defined.

- [ ] **Step 4: Implement `cue`**

Append to `engine/src/deck.rs`:

```rust
use std::path::PathBuf;

impl Deck {
    /// Load a track and spawn the decoder positioned at `source_start_seconds`.
    /// After this call: Cueing, fader=0, tempo=1.0 (caller sets tempo via automation).
    pub async fn cue(
        &mut self,
        track: LoadedTrack,
        source_start_seconds: f64,
    ) -> Result<()> {
        let path: PathBuf = track.sidecar.source_path.clone().into();
        let cue_out = track
            .sidecar
            .cue_out_seconds
            .unwrap_or(track.sidecar.duration_seconds);
        let decoder = PlayingDecoder::spawn(&path, source_start_seconds, cue_out).await?;
        self.sources.clear();
        self.sources.push_back(DeckSource::Decoder(decoder));
        self.state.source_path = Some(track.sidecar.source_path.clone());
        self.state.play_state = PlayState::Cueing;
        self.state.fader = 0.0;
        self.state.tempo = 1.0;
        self.source_position_seconds = source_start_seconds;
        self.source_end_seconds = cue_out;
        Ok(())
    }
}
```

- [ ] **Step 5: Verify the test passes**

Run: `cd engine && cargo test --test deck_test`
Expected: all tests pass (assumes ffmpeg is on PATH).

- [ ] **Step 6: Commit**

```
git add engine/src/deck.rs engine/tests/deck_test.rs engine/tests/fixtures/sine_440_3s.wav
git commit -m "feat(engine): Deck::cue spawns decoder + Cueing state (sub-project 1, task 10)"
```

---

### Task 11: `Deck::play` + `Deck::pull` silence (Stopped/Cueing returns zeros)

**Files:**
- Modify: `engine/src/deck.rs`
- Modify: `engine/tests/deck_test.rs`

- [ ] **Step 1: Write failing tests**

Append to `engine/tests/deck_test.rs`:

```rust
#[tokio::test]
async fn pull_stopped_returns_zeros() {
    let mut d = Deck::new(DeckId::A);
    let pcm = d.pull(1024).await.expect("pull ok");
    assert_eq!(pcm.len(), 1024 * 2);
    assert!(pcm.iter().all(|s| *s == 0.0));
    assert_eq!(d.frame_cursor(), 1024);
}

#[tokio::test]
async fn pull_cueing_returns_zeros() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0).await.unwrap();
    let pcm = d.pull(512).await.unwrap();
    assert_eq!(pcm.len(), 512 * 2);
    assert!(pcm.iter().all(|s| *s == 0.0));
    assert_eq!(d.frame_cursor(), 512);
    // Still in Cueing because play() hasn't been called.
    assert_eq!(d.state().play_state, PlayState::Cueing);
}

#[tokio::test]
async fn play_transitions_to_playing() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0).await.unwrap();
    d.play();
    assert_eq!(d.state().play_state, PlayState::Playing);
}
```

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test deck_test`
Expected: FAIL — `play` and `pull` not defined.

- [ ] **Step 3: Implement**

Append to `engine/src/deck.rs`:

```rust
impl Deck {
    pub fn play(&mut self) {
        if matches!(self.state.play_state, PlayState::Cueing) {
            self.state.play_state = PlayState::Playing;
        }
    }

    /// Pull `n_frames` of interleaved stereo PCM. Always advances `frame_cursor`
    /// by `n_frames`. Returns zeros for Stopped/Cueing; the Playing path is added
    /// in Task 12.
    pub async fn pull(&mut self, n_frames: usize) -> Result<Vec<f32>> {
        self.frame_cursor += n_frames as u64;
        if !matches!(self.state.play_state, PlayState::Playing) {
            return Ok(vec![0.0; n_frames * (CHANNELS as usize)]);
        }
        // Playing branch — Task 12 fills this in.
        Ok(vec![0.0; n_frames * (CHANNELS as usize)])
    }
}
```

- [ ] **Step 4: Verify pass**

Run: `cd engine && cargo test --test deck_test`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/deck.rs engine/tests/deck_test.rs
git commit -m "feat(engine): Deck::play + Deck::pull silence path (sub-project 1, task 11)"
```

---

### Task 12: `Deck::pull` Playing branch with fader gain

**Files:**
- Modify: `engine/src/deck.rs`
- Modify: `engine/tests/deck_test.rs`

- [ ] **Step 1: Write failing test**

Append to `engine/tests/deck_test.rs`:

```rust
#[tokio::test]
async fn pull_playing_produces_nonzero_with_fader_unity() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0).await.unwrap();
    d.play();
    // Manually set fader=1.0 (Task 13 adds automation-driven setting).
    // We use the test helper added below to poke state.
    d.test_set_fader(1.0);
    let pcm = d.pull(4096).await.unwrap();
    assert_eq!(pcm.len(), 4096 * 2);
    let energy: f32 = pcm.iter().map(|s| s.abs()).sum();
    assert!(energy > 1.0, "expected non-trivial audio, got energy {energy}");
}

#[tokio::test]
async fn pull_playing_with_fader_zero_returns_silence() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0).await.unwrap();
    d.play();
    // fader stays at 0.0 (its initial value)
    let pcm = d.pull(4096).await.unwrap();
    assert!(pcm.iter().all(|s| *s == 0.0));
}
```

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test deck_test`
Expected: FAIL — the first test expects non-zero output but current pull returns zeros for Playing; `test_set_fader` not defined.

- [ ] **Step 3: Implement Playing branch + test helper**

Replace the `pull` body in `engine/src/deck.rs`:

```rust
impl Deck {
    pub async fn pull(&mut self, n_frames: usize) -> Result<Vec<f32>> {
        self.frame_cursor += n_frames as u64;

        if !matches!(self.state.play_state, PlayState::Playing) {
            return Ok(vec![0.0; n_frames * (CHANNELS as usize)]);
        }

        // Drain from the source queue until we have n_frames or run dry.
        let need_samples = n_frames * (CHANNELS as usize);
        let mut out: Vec<f32> = Vec::with_capacity(need_samples);
        while out.len() < need_samples {
            let remaining_frames = (need_samples - out.len()) / (CHANNELS as usize);
            if remaining_frames == 0 {
                break;
            }
            let Some(source) = self.sources.front_mut() else { break };
            let chunk = source.pull(remaining_frames).await?;
            if chunk.is_empty() {
                // Source ran dry; advance.
                self.sources.pop_front();
                continue;
            }
            out.extend_from_slice(&chunk);
            if source.is_exhausted() {
                self.sources.pop_front();
            }
        }
        // Pad with silence if the queue is empty.
        if out.len() < need_samples {
            out.resize(need_samples, 0.0);
        }

        // Apply current fader as a constant scalar over this tick. Task 13
        // makes this per-sample by evaluating automations at sub-tick resolution.
        let g = self.state.fader;
        if g != 1.0 {
            for s in out.iter_mut() {
                *s *= g;
            }
        }

        // Advance source-position estimate (used by tempo respawns in Task 14).
        let emitted_seconds = n_frames as f64 / SAMPLE_RATE as f64;
        self.source_position_seconds += emitted_seconds * self.state.tempo;

        Ok(out)
    }

    /// Test-only fader setter — bypasses the automation queue.
    #[doc(hidden)]
    pub fn test_set_fader(&mut self, v: f32) {
        self.state.fader = v;
    }
}
```

- [ ] **Step 4: Verify pass**

Run: `cd engine && cargo test --test deck_test`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/deck.rs engine/tests/deck_test.rs
git commit -m "feat(engine): Deck::pull Playing path with fader gain (sub-project 1, task 12)"
```

---

### Task 13: Automation evaluation inside `Deck::pull`

**Files:**
- Modify: `engine/src/deck.rs`
- Modify: `engine/tests/deck_test.rs`

- [ ] **Step 1: Write failing test**

Append to `engine/tests/deck_test.rs`:

```rust
use dj_engine::automation::{Automation, AutomationTarget, Curve};

#[tokio::test]
async fn fader_linear_automation_ramps_signal() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0).await.unwrap();
    d.play();
    // Schedule a Linear 0→1 fader over the first 8192 frames.
    d.schedule(Automation {
        target: AutomationTarget::Fader,
        t_start_frames: 0,
        t_end_frames: 8192,
        from: 0.0,
        to: 1.0,
        curve: Curve::Linear,
    });
    // Pull half the ramp.
    let first = d.pull(4096).await.unwrap();
    let energy_first: f32 = first.iter().map(|s| s.abs()).sum();
    // Pull the rest of the ramp.
    let second = d.pull(4096).await.unwrap();
    let energy_second: f32 = second.iter().map(|s| s.abs()).sum();
    // Second half should be louder (ramp 0→1 means fader avg is ~0.25 then ~0.75).
    assert!(energy_second > energy_first * 2.0,
            "expected second half louder; got first={energy_first} second={energy_second}");
}
```

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test deck_test`
Expected: FAIL — fader stays at 0 since automation is queued but never evaluated.

- [ ] **Step 3: Implement automation evaluation in pull**

Edit `engine/src/deck.rs`. In `Deck::pull`, replace the constant-fader application loop with a per-sample evaluation. New `pull`:

```rust
impl Deck {
    pub async fn pull(&mut self, n_frames: usize) -> Result<Vec<f32>> {
        let start_cursor = self.frame_cursor;
        self.frame_cursor += n_frames as u64;

        if !matches!(self.state.play_state, PlayState::Playing) {
            // Still tick the automation queue so future automations land correctly.
            self.automations.step(self.frame_cursor, &mut self.state);
            return Ok(vec![0.0; n_frames * (CHANNELS as usize)]);
        }

        // Drain source queue.
        let need_samples = n_frames * (CHANNELS as usize);
        let mut out: Vec<f32> = Vec::with_capacity(need_samples);
        while out.len() < need_samples {
            let remaining_frames = (need_samples - out.len()) / (CHANNELS as usize);
            if remaining_frames == 0 {
                break;
            }
            let Some(source) = self.sources.front_mut() else { break };
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

        // Per-sample fader: evaluate automation queue at frame granularity
        // (apply the same gain to L and R of each frame).
        // We step the queue at every frame to keep the deck's state coherent
        // with sample-level gain changes. For TICK_FRAMES=4096, this is ~4096
        // step() calls per tick — cheap (each is a Vec scan of typically <5).
        for f in 0..n_frames {
            let cursor = start_cursor + f as u64;
            self.automations.step(cursor, &mut self.state);
            let g = self.state.fader;
            let li = f * 2;
            let ri = li + 1;
            out[li] *= g;
            out[ri] *= g;
        }
        // Final step at frame_cursor (after the loop) to install any to-values
        // for automations that finish exactly at frame_cursor.
        self.automations.step(self.frame_cursor, &mut self.state);

        let emitted_seconds = n_frames as f64 / SAMPLE_RATE as f64;
        self.source_position_seconds += emitted_seconds * self.state.tempo;

        Ok(out)
    }
}
```

- [ ] **Step 4: Verify pass**

Run: `cd engine && cargo test --test deck_test`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/deck.rs engine/tests/deck_test.rs
git commit -m "feat(engine): per-sample automation evaluation in Deck::pull (sub-project 1, task 13)"
```

---

### Task 14: Tempo Linear ramp + chunked atempo + splice

**Files:**
- Modify: `engine/src/deck.rs`
- Move logic from: `engine/src/ffmpeg/mixwindow.rs` (preserved as a private `deck::stretch` submodule; will be deleted from public surface in Task 20)
- Modify: `engine/tests/deck_test.rs`

- [ ] **Step 1: Add internal `stretch` submodule (paste in existing mixwindow code)**

Append to `engine/src/deck.rs`:

```rust
pub(crate) mod stretch {
    //! Internal stretch helpers (formerly engine/src/ffmpeg/mixwindow.rs).
    //! Same code, same constants — moved here to make the deck self-contained.

    pub use crate::ffmpeg::mixwindow::{
        decode_atempo_at_source, decode_stretched_ramp, ramp_source_consumed_seconds,
        ATEMPO_PER_CALL_OVERHEAD_S,
    };
}
```

(We re-export rather than copy. Task 20 inlines and removes the public `ffmpeg::mixwindow` surface.)

- [ ] **Step 2: Detect Tempo Linear automation and pre-decode ramp**

The trick: when `Deck::pull` sees that the next active automation is `Tempo Linear from=r0 to=r1 curve=Linear`, we (a) decode the entire ramp into PCM up-front, (b) pre-spawn the post-ramp decoder, (c) read its first `SPLICE_FADE_MS` worth of frames, (d) splice-mix the ramp tail with the post-ramp head, (e) replace the ramp's tail with the spliced result, (f) push a `DeckSource::Buffer` of the ramp PCM and a `DeckSource::Decoder` of the post-ramp decoder onto the sources queue, in that order.

This work is triggered on the **first** pull whose `start_cursor + n_frames > tempo_linear.t_start_frames` — i.e., when we are about to enter the ramp.

Add to `engine/src/deck.rs`:

```rust
const SPLICE_FADE_MS: f64 = 300.0;
const RAMP_CHUNK_BARS: usize = 4;
const RAMP_BARS: usize = 16;
const RAMP_IDENTITY_EPSILON: f64 = 0.005;

impl Deck {
    /// Look for a pending Tempo Linear automation that we have not yet
    /// materialized into source-queue PCM. If found, build the ramp buffer +
    /// post-ramp decoder + splice and push them onto the queue.
    async fn maybe_prepare_tempo_ramp(&mut self, cursor: u64) -> Result<()> {
        // The AutomationQueue is private to it; expose a method or iterate via
        // a public accessor added now.
        if let Some(ramp) = self.automations.peek_pending_tempo_linear() {
            // Materialize once: track which ramps we've materialized via a flag.
            if self.ramp_materialized_for == Some(ramp.t_start_frames) {
                return Ok(());
            }
            // Lookahead: only materialize when within 1 bar of the start.
            let one_bar_frames = (SAMPLE_RATE as f64 * 60.0 / 120.0 * 4.0) as u64;
            if ramp.t_start_frames > cursor + one_bar_frames {
                return Ok(());
            }

            let source_path: std::path::PathBuf = self
                .state
                .source_path
                .as_ref()
                .map(|s| s.into())
                .ok_or_else(|| crate::error::EngineError::Ffmpeg("no source".into()))?;
            let start_ratio = ramp.from;
            let end_ratio = ramp.to;
            let ramp_frames = ramp.t_end_frames - ramp.t_start_frames;
            let ramp_seconds = ramp_frames as f64 / SAMPLE_RATE as f64;
            let chunks = (RAMP_BARS / RAMP_CHUNK_BARS).max(1);

            // Source-time position where the ramp begins.
            let ramp_source_start = self.source_position_seconds;
            let ramp_consumed = stretch::ramp_source_consumed_seconds(
                start_ratio,
                end_ratio,
                ramp_seconds,
                chunks,
            );
            let post_ramp_source = (ramp_source_start + ramp_consumed)
                .min(self.source_end_seconds);

            // Decode the ramp PCM (blocking await — runs ahead of real time).
            let mut ramp_pcm = stretch::decode_stretched_ramp(
                &source_path,
                ramp_source_start,
                start_ratio,
                end_ratio,
                ramp_seconds,
                chunks,
            )
            .await?;

            // Pre-spawn the post-ramp decoder and read SPLICE_FADE_MS of its head.
            let splice_frames = ((SPLICE_FADE_MS / 1000.0) * SAMPLE_RATE as f64) as usize;
            let splice_samples = splice_frames * (CHANNELS as usize);
            let new_start = (post_ramp_source - SPLICE_FADE_MS / 1000.0).max(0.0);
            let mut new_decoder =
                PlayingDecoder::spawn(&source_path, new_start, self.source_end_seconds).await?;
            let mut new_head = new_decoder.read_frames(splice_frames).await?;
            if new_head.len() < splice_samples {
                new_head.resize(splice_samples, 0.0);
            }
            // Splice: replace ramp_pcm's last splice_samples with an equal-power
            // crossfade between ramp_tail and new_head.
            if ramp_pcm.len() >= splice_samples {
                use std::f64::consts::FRAC_PI_2;
                let body_end = ramp_pcm.len() - splice_samples;
                for i in 0..splice_samples {
                    let t = (i / 2) as f64 / splice_frames.max(1) as f64;
                    let g_out = (t * FRAC_PI_2).cos();
                    let g_in = (t * FRAC_PI_2).sin();
                    let ramp_sample = ramp_pcm[body_end + i] as f64;
                    let new_sample = new_head[i] as f64;
                    ramp_pcm[body_end + i] = (ramp_sample * g_out + new_sample * g_in) as f32;
                }
            }

            // Clear the existing source queue (the steady-state decoder for the
            // pre-ramp portion has already been pulled through; whatever is left
            // is residual). Push buffer, then post-ramp decoder.
            self.sources.clear();
            self.sources.push_back(DeckSource::Buffer {
                pcm: ramp_pcm,
                offset_samples: 0,
            });
            self.sources.push_back(DeckSource::Decoder(new_decoder));

            // Update source position to reflect that we've now committed to
            // post-ramp playback.
            self.source_position_seconds = new_start + (SPLICE_FADE_MS / 1000.0);
            self.ramp_materialized_for = Some(ramp.t_start_frames);
        }
        Ok(())
    }
}
```

Add the supporting field to `Deck`:

```rust
pub struct Deck {
    // ... existing fields ...
    ramp_materialized_for: Option<u64>,
}
```

And update `Deck::new` to initialize: `ramp_materialized_for: None`.

Add a peek helper to `AutomationQueue` in `engine/src/automation.rs`:

```rust
impl AutomationQueue {
    /// Inspect the earliest pending Tempo automation with `curve == Linear`.
    /// Used by the deck to decide when to pre-decode a ramp.
    pub fn peek_pending_tempo_linear(&self) -> Option<&Automation> {
        self.pending
            .iter()
            .filter(|a| a.target == AutomationTarget::Tempo && a.curve == Curve::Linear)
            .min_by_key(|a| a.t_start_frames)
    }
}
```

Call `maybe_prepare_tempo_ramp` at the start of `Deck::pull`:

```rust
impl Deck {
    pub async fn pull(&mut self, n_frames: usize) -> Result<Vec<f32>> {
        let start_cursor = self.frame_cursor;
        self.maybe_prepare_tempo_ramp(start_cursor).await?;
        self.frame_cursor += n_frames as u64;
        // ... rest as before ...
    }
}
```

- [ ] **Step 3: Write a failing test for the ramp**

Append to `engine/tests/deck_test.rs`:

```rust
#[tokio::test]
async fn tempo_linear_ramp_swaps_to_post_ramp_decoder() {
    // Cue a 6-second test fixture (we'll need it; see Step 4).
    let mut d = Deck::new(DeckId::A);
    let track = LoadedTrack {
        sidecar: Sidecar {
            source_path: format!(
                "{}/tests/fixtures/sine_440_6s.wav",
                env!("CARGO_MANIFEST_DIR")
            ),
            duration_seconds: 6.0,
            cue_out_seconds: Some(6.0),
            ..fixture_track().sidecar
        },
    };
    d.cue(track, 0.0).await.unwrap();
    d.play();
    d.test_set_fader(1.0);
    // Schedule a Tempo Step from 1.07 → 1.07 (no-op, set the deck's tempo state).
    d.schedule(Automation {
        target: AutomationTarget::Tempo,
        t_start_frames: 0,
        t_end_frames: 1,
        from: 1.07,
        to: 1.07,
        curve: Curve::Step,
    });
    // Schedule a Linear ramp 1.07 → 1.0 over the next ~2 seconds.
    d.schedule(Automation {
        target: AutomationTarget::Tempo,
        t_start_frames: 0,
        t_end_frames: 88_200, // ~2s at 44.1 kHz
        from: 1.07,
        to: 1.0,
        curve: Curve::Linear,
    });
    // Pull through a few ticks; the deck should produce continuous audio.
    let mut total: Vec<f32> = Vec::new();
    for _ in 0..30 {
        let chunk = d.pull(4096).await.unwrap();
        total.extend_from_slice(&chunk);
    }
    // Check no long gap of silence (> 1024 frames in a row of zeros = bug).
    let mut zero_run = 0usize;
    let mut max_zero_run = 0usize;
    for s in &total {
        if *s == 0.0 {
            zero_run += 1;
            max_zero_run = max_zero_run.max(zero_run);
        } else {
            zero_run = 0;
        }
    }
    assert!(max_zero_run < 1024 * 2, "long silence gap: {max_zero_run} samples");
}
```

Create the 6s fixture: `ffmpeg -hide_banner -loglevel error -f lavfi -i "sine=frequency=440:duration=6:sample_rate=44100" -ac 2 engine/tests/fixtures/sine_440_6s.wav`

- [ ] **Step 4: Verify pass**

Run: `cd engine && cargo test --test deck_test tempo_linear_ramp_swaps_to_post_ramp_decoder`
Expected: pass.

- [ ] **Step 5: Commit**

```
git add engine/src/deck.rs engine/src/automation.rs engine/tests/deck_test.rs engine/tests/fixtures/sine_440_6s.wav
git commit -m "feat(engine): Tempo Linear ramp + chunked atempo + splice in Deck (sub-project 1, task 14)"
```

---

### Task 15: `Mixer` struct + `tick`

**Files:**
- Modify: `engine/src/mixer.rs` (rewrite the file)
- Modify: `engine/src/lib.rs` (no change — `mixer` already declared)

- [ ] **Step 1: Replace mixer.rs entirely**

Replace the contents of `engine/src/mixer.rs`:

```rust
//! Mixer: ticks two persistent decks, sums their PCM, pushes to the ring.

use tracing::info;

use crate::buffer::{Producer, SampleProducer};
use crate::deck::{Deck, DeckId};
use crate::error::Result;
use crate::ffmpeg::CHANNELS;

pub const TICK_FRAMES: usize = 4096;

pub struct Mixer {
    deck_a: Deck,
    deck_b: Deck,
    producer: SampleProducer,
    master_clock_frames: u64,
}

impl Mixer {
    pub fn new(producer: SampleProducer) -> Self {
        Self {
            deck_a: Deck::new(DeckId::A),
            deck_b: Deck::new(DeckId::B),
            producer,
            master_clock_frames: 0,
        }
    }

    pub fn deck(&mut self, id: DeckId) -> &mut Deck {
        match id {
            DeckId::A => &mut self.deck_a,
            DeckId::B => &mut self.deck_b,
        }
    }

    pub fn master_clock_frames(&self) -> u64 {
        self.master_clock_frames
    }

    /// Pull from both decks in parallel, sum, push to the ring buffer.
    pub async fn tick(&mut self) -> Result<()> {
        let (a, b) = tokio::join!(
            self.deck_a.pull(TICK_FRAMES),
            self.deck_b.pull(TICK_FRAMES),
        );
        let a = a?;
        let b = b?;
        debug_assert_eq!(a.len(), TICK_FRAMES * (CHANNELS as usize));
        debug_assert_eq!(b.len(), TICK_FRAMES * (CHANNELS as usize));

        let mut mixed = vec![0.0f32; a.len()];
        for i in 0..mixed.len() {
            mixed[i] = a[i] + b[i];
        }
        push_all(&mut self.producer, &mixed).await;
        self.master_clock_frames += TICK_FRAMES as u64;
        Ok(())
    }
}

async fn push_all(producer: &mut SampleProducer, samples: &[f32]) {
    let mut offset = 0;
    while offset < samples.len() {
        let n = producer.push_slice(&samples[offset..]);
        if n == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        } else {
            offset += n;
        }
    }
}

/// Convenience entry-point: tick until master_clock_frames >= end_frames.
/// Used by the main loop to play out a transition or until the next decision.
pub async fn tick_until(mixer: &mut Mixer, end_frames: u64) -> Result<()> {
    while mixer.master_clock_frames() < end_frames {
        mixer.tick().await?;
    }
    info!(end = end_frames, "tick_until complete");
    Ok(())
}
```

- [ ] **Step 2: Verify the project builds**

Run: `cd engine && cargo build`
Expected: clean build. The old `play_until_transition_and_crossfade` is gone; `main.rs` will fail to compile and be fixed in Task 17.

(If `main.rs` currently fails to build, that's expected — Task 17 fixes it. For now we run the unit tests only.)

```
cd engine && cargo test --lib --tests deck_test recipe_test automation_test
```

Expected: tests pass; main.rs may not compile until Task 17.

- [ ] **Step 3: Commit**

```
git add engine/src/mixer.rs
git commit -m "feat(engine): Mixer struct with parallel-pull tick (sub-project 1, task 15)"
```

---

### Task 16: `TransitionScheduler::arm`

**Files:**
- Create: `engine/src/scheduler.rs`
- Modify: `engine/src/lib.rs` (add `pub mod scheduler;`)
- Create: `engine/tests/scheduler_test.rs`

- [ ] **Step 1: Add module + skeleton**

Edit `engine/src/lib.rs`: add `pub mod scheduler;` after `pub mod recipe;`.

Create `engine/src/scheduler.rs`:

```rust
//! TransitionScheduler: cues the incoming deck, builds automations from a
//! recipe, schedules them on both decks, returns a handle with the end time.

use crate::deck::DeckId;
use crate::error::Result;
use crate::library::LoadedTrack;
use crate::mixer::Mixer;
use crate::recipe::{Recipe, RecipeOutput, TransitionContext};
use crate::transition::TransitionPlan;
use crate::ffmpeg::SAMPLE_RATE;

const CUE_HEADROOM_BARS: f64 = 1.0;

pub struct TransitionHandle {
    pub end_master_clock_frames: u64,
    pub recipe_name: &'static str,
}

pub struct TransitionScheduler;

impl TransitionScheduler {
    pub async fn arm<R: Recipe>(
        mixer: &mut Mixer,
        outgoing_deck: DeckId,
        incoming_deck: DeckId,
        outgoing: &LoadedTrack,
        incoming: &LoadedTrack,
        plan: &TransitionPlan,
        recipe: &R,
    ) -> Result<TransitionHandle> {
        let outgoing_bpm = outgoing.sidecar.bpm.unwrap_or(120.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let cue_headroom_seconds = CUE_HEADROOM_BARS * bar_seconds;
        let cue_start = (plan.incoming_crossfade_start_seconds - cue_headroom_seconds).max(0.0);

        // 1) Cue the incoming deck.
        mixer
            .deck(incoming_deck)
            .cue(incoming.clone(), cue_start)
            .await?;
        mixer.deck(incoming_deck).play();

        // 2) Compute t_start: master_clock + (outgoing_outro_start - outgoing's
        //    current source position) * SAMPLE_RATE.
        let master_clock = mixer.master_clock_frames();
        // We can't query the outgoing deck's source position from here without
        // exposing it. For now, treat t_start as "the next tick" — the main
        // loop is responsible for sequencing so the outgoing deck reaches
        // outro_start at master_clock_frames.
        let t_start = master_clock;

        // 3) Build automations.
        let ctx = TransitionContext {
            outgoing,
            incoming,
            plan,
            master_clock_frames: t_start,
            outgoing_deck,
            incoming_deck,
            sample_rate: SAMPLE_RATE,
        };
        let RecipeOutput {
            outgoing_automations,
            incoming_automations,
            total_duration_frames,
        } = recipe.build(&ctx);

        // 4) Schedule on the respective decks.
        for a in outgoing_automations {
            mixer.deck(outgoing_deck).schedule(a);
        }
        for a in incoming_automations {
            mixer.deck(incoming_deck).schedule(a);
        }

        Ok(TransitionHandle {
            end_master_clock_frames: t_start + total_duration_frames,
            recipe_name: recipe.name(),
        })
    }
}
```

- [ ] **Step 2: Write a scheduler test (synthetic)**

Create `engine/tests/scheduler_test.rs`:

```rust
use dj_engine::buffer::make_ring;
use dj_engine::deck::DeckId;
use dj_engine::ffmpeg::SAMPLE_RATE;
use dj_engine::library::LoadedTrack;
use dj_engine::mixer::Mixer;
use dj_engine::recipe::EqualPowerCrossfade;
use dj_engine::scheduler::TransitionScheduler;
use dj_engine::sidecar::Sidecar;
use dj_engine::transition::TransitionPlan;
use std::path::PathBuf;

fn fixture_track(name: &str, dur: f64) -> LoadedTrack {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/fixtures/{name}"));
    LoadedTrack {
        sidecar: Sidecar {
            schema_version: 1,
            analyzer_version: "test".into(),
            source_path: path.to_string_lossy().into_owned(),
            source_mtime: 0.0,
            source_size_bytes: 0,
            duration_seconds: dur,
            sample_rate: 44100,
            bpm: Some(120.0),
            bpm_confidence: Some(1.0),
            beats_seconds: None,
            downbeats_seconds: None,
            key: None,
            key_label: None,
            key_confidence: None,
            energy_curve: None,
            energy_mean: Some(0.5),
            band_energy_means: None,
            cue_in_seconds: Some(0.0),
            cue_out_seconds: Some(dur),
            intro_end_seconds: Some(0.0),
            outro_start_seconds: Some(dur),
            is_mixable: Some(true),
            mixable_reasons: None,
            error: None,
            ssm: None,
            novelty: None,
            rms_db: None,
        },
    }
}

#[tokio::test]
async fn arm_cues_incoming_and_schedules_automations() {
    let cap = (SAMPLE_RATE as usize) * 2 * 10;
    let (producer, _consumer) = make_ring(cap);
    let mut mixer = Mixer::new(producer);
    let outgoing = fixture_track("sine_440_6s.wav", 6.0);
    let incoming = fixture_track("sine_660_6s.wav", 6.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.0,
        incoming_crossfade_start_seconds: 2.0,
        length_bars: 16,
        use_eq_swap: false,
    };
    let handle = TransitionScheduler::arm(
        &mut mixer,
        DeckId::A,
        DeckId::B,
        &outgoing,
        &incoming,
        &plan,
        &EqualPowerCrossfade,
    )
    .await
    .expect("arm");

    let bar_seconds = (60.0_f64 / 120.0) * 4.0;
    let expected = (plan.length_bars as f64 * bar_seconds * SAMPLE_RATE as f64) as u64;
    assert!(
        handle.end_master_clock_frames >= expected,
        "end {} < expected {expected}",
        handle.end_master_clock_frames
    );
    assert_eq!(handle.recipe_name, "equal_power_crossfade");
}
```

Generate the second fixture: `ffmpeg -hide_banner -loglevel error -f lavfi -i "sine=frequency=660:duration=6:sample_rate=44100" -ac 2 engine/tests/fixtures/sine_660_6s.wav`

- [ ] **Step 3: Verify pass**

Run: `cd engine && cargo test --test scheduler_test`
Expected: pass.

- [ ] **Step 4: Commit**

```
git add engine/src/lib.rs engine/src/scheduler.rs engine/tests/scheduler_test.rs engine/tests/fixtures/sine_660_6s.wav
git commit -m "feat(engine): TransitionScheduler::arm cues + schedules (sub-project 1, task 16)"
```

---

### Task 17: `main.rs` — rewire to use Mixer + Scheduler

**Files:**
- Modify: `engine/src/main.rs`

- [ ] **Step 1: Replace main's transition loop**

Replace the body of `engine/src/main.rs` after the existing setup code (encoder + ring buffer + writer task) with the new loop. The full replacement (delete from the seed-track setup all the way to the end of the existing main loop, then paste):

```rust
    // Seed track
    let seed = pick_seed(&library, cli.seed_track.as_deref());
    info!(seed = %seed.sidecar.source_path, "seed track");

    // Mixer with two decks. Cue + play the seed on Deck A.
    let mut mixer = dj_engine::mixer::Mixer::new(producer);
    {
        let cue_in = seed.sidecar.cue_in_seconds.unwrap_or(0.0);
        let deck = mixer.deck(dj_engine::deck::DeckId::A);
        deck.cue(seed.clone(), cue_in).await?;
        deck.play();
        deck.test_set_fader(1.0);
    }

    let mut current = seed;
    let mut current_deck = dj_engine::deck::DeckId::A;
    let mut other_deck = dj_engine::deck::DeckId::B;

    let mut recency = RecencyRing::new(20);
    recency.record(current.sidecar.source_path.clone());
    let mut energy = EnergyTarget::new(current.sidecar.energy_mean.unwrap_or(0.5));
    let http = reqwest::Client::new();
    icecast_meta::update_now_playing(&http, &metadata_cfg, &current.sidecar.source_path).await;

    loop {
        let next_track = {
            let mut sel = Selector::new(&library, &recency, energy.current());
            match sel.pick_next(&current) {
                Some(t) => t.clone(),
                None => {
                    warn!("selector returned nothing; ending set");
                    break;
                }
            }
        };

        let plan = dj_engine::transition::plan_transition(
            &current.sidecar, &next_track.sidecar);

        // Play current track until its outro_start (then the recipe takes over).
        // outro_start in source-time → master-clock frames.
        let outgoing_bpm = current.sidecar.bpm.unwrap_or(120.0);
        let _ = outgoing_bpm; // currently the deck plays at native tempo; future sub-projects honor pitch slider
        let cue_in = current.sidecar.cue_in_seconds.unwrap_or(0.0);
        let outro_start = plan.outgoing_crossfade_start_seconds;
        let frames_until_outro = ((outro_start - cue_in).max(0.0)
            * dj_engine::ffmpeg::SAMPLE_RATE as f64) as u64;
        let outro_deadline = mixer.master_clock_frames() + frames_until_outro;
        dj_engine::mixer::tick_until(&mut mixer, outro_deadline).await?;

        // Arm the transition.
        let choice = dj_engine::recipe::pick_recipe(&plan);
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
        };

        // Tick through the transition.
        dj_engine::mixer::tick_until(&mut mixer, handle.end_master_clock_frames).await?;

        // Unload outgoing, swap roles.
        mixer.deck(current_deck).unload().await;
        std::mem::swap(&mut current_deck, &mut other_deck);
        recency.record(next_track.sidecar.source_path.clone());
        energy.advance();
        icecast_meta::update_now_playing(&http, &metadata_cfg, &next_track.sidecar.source_path).await;
        current = next_track;
    }

    let _ = writer_task.await;
    Ok(())
```

Remove the `use dj_engine::mixer::play_until_transition_and_crossfade;` line and the `use dj_engine::ffmpeg::decoder::PlayingDecoder;` line at the top of main.rs (no longer needed at top level).

- [ ] **Step 2: Verify the project builds**

Run: `cd engine && cargo build`
Expected: clean build.

- [ ] **Step 3: Run the full test suite**

Run: `cd engine && cargo test`
Expected: New tests all pass. Existing `mixer_integration_test.rs`, `crossfade_test.rs`, `crossfade_eq_test.rs`, `mixwindow_ramp_test.rs` may now fail because they reference removed code paths — those get fixed in Tasks 21 and 22.

Note for the executor: if a previously-passing test fails because it calls `play_until_transition_and_crossfade` (now removed), mark the test `#[ignore]` with a comment `// rewritten in task 21` and continue. Don't try to fix it yet.

- [ ] **Step 4: Commit**

```
git add engine/src/main.rs
git commit -m "feat(engine): wire main.rs to Mixer + Scheduler + role swap (sub-project 1, task 17)"
```

---

### Task 18: Slim down `crossfade.rs`

**Files:**
- Modify: `engine/src/crossfade.rs`

- [ ] **Step 1: Replace the file**

Replace `engine/src/crossfade.rs` entirely:

```rust
//! Equal-power crossfade curve helper. Used by `Curve::EqualPowerIn/Out` in
//! the automation layer. The mix-two-buffers helpers were removed when the
//! two-deck tick-driven mixer landed; per-tick fader application now lives in
//! `Deck::pull`.

use std::f64::consts::FRAC_PI_2;

/// Return (outgoing_gain, incoming_gain) for `t` in [0, 1].
pub fn equal_power_curve(t: f64) -> (f64, f64) {
    let t = t.clamp(0.0, 1.0);
    let in_gain = (t * FRAC_PI_2).sin();
    let out_gain = (t * FRAC_PI_2).cos();
    (out_gain, in_gain)
}
```

- [ ] **Step 2: Verify build**

Run: `cd engine && cargo build`
Expected: clean build.

- [ ] **Step 3: Commit**

```
git add engine/src/crossfade.rs
git commit -m "refactor(engine): slim crossfade.rs (delete mix_crossfade/mix_crossfade_eq) (sub-project 1, task 18)"
```

---

### Task 19: Delete `ffmpeg/mixwindow.rs` public surface

**Files:**
- Modify: `engine/src/ffmpeg/mod.rs`
- Modify: `engine/src/deck.rs` (inline the stretch helpers; no longer re-exported)
- Delete: `engine/src/ffmpeg/mixwindow.rs`

- [ ] **Step 1: Inline the helpers into a private deck submodule**

In `engine/src/deck.rs`, replace the `pub(crate) mod stretch { pub use crate::ffmpeg::mixwindow::...; }` block with the full code from today's `engine/src/ffmpeg/mixwindow.rs`, wrapped in a private module:

```rust
mod stretch {
    //! Internal helpers: chunked atempo decode for the pitch-journey ramp.
    //! (Formerly engine/src/ffmpeg/mixwindow.rs.)

    use std::path::Path;
    use std::process::Stdio;
    use tokio::io::AsyncReadExt;
    use tokio::process::Command;

    use crate::error::{EngineError, Result};
    use crate::ffmpeg::{CHANNELS, SAMPLE_RATE};

    pub const ATEMPO_PER_CALL_OVERHEAD_S: f64 = 0.13;

    // ... paste the full bodies of decode_atempo_at_source, decode_stretched_ramp,
    // ramp_source_consumed_seconds, atempo_chain from the old mixwindow.rs ...
}
```

(The executor: open `engine/src/ffmpeg/mixwindow.rs`, copy the function bodies, paste into the `mod stretch` block in `deck.rs`. Adjust the import paths if needed.)

- [ ] **Step 2: Remove `ffmpeg::mixwindow` from the public surface**

Edit `engine/src/ffmpeg/mod.rs`:

```rust
pub mod decoder;
pub mod encoder;

pub const SAMPLE_RATE: u32 = 44100;
pub const CHANNELS: u32 = 2;
pub const SAMPLE_BYTES: usize = 4;
pub const BYTES_PER_FRAME: usize = (CHANNELS as usize) * SAMPLE_BYTES;
```

Delete the `pub mod mixwindow;` line. Delete the file `engine/src/ffmpeg/mixwindow.rs`.

- [ ] **Step 3: Verify build**

Run: `cd engine && cargo build`
Expected: clean build.

- [ ] **Step 4: Commit**

```
git add engine/src/deck.rs engine/src/ffmpeg/mod.rs
git rm engine/src/ffmpeg/mixwindow.rs
git commit -m "refactor(engine): move stretch helpers into deck::stretch (sub-project 1, task 19)"
```

---

### Task 20: Handle `eq.rs::apply_swept_filter` (now dead until sub-project 3)

**Files:**
- Modify: `engine/src/eq.rs`

- [ ] **Step 1: Annotate dead but preserved code**

Open `engine/src/eq.rs`. Find `pub fn apply_swept_filter` (also `pub fn apply_filter` if present, and any helper marked `pub` that is only used by the swept filter). Add `#[allow(dead_code)]` above their `pub fn` declarations.

(Rationale: sub-project 3 reintroduces callers; preserving the implementation avoids re-deriving the biquad math.)

- [ ] **Step 2: Verify build is warning-free**

Run: `cd engine && cargo build`
Expected: no warnings.

Run: `cd engine && cargo clippy -- -D warnings`
Expected: clean.

- [ ] **Step 3: Commit**

```
git add engine/src/eq.rs
git commit -m "chore(engine): mark apply_swept_filter dead-code-allowed pending sub-project 3 (task 20)"
```

---

### Task 21: Update / delete existing tests

**Files:**
- Modify: `engine/tests/crossfade_test.rs`
- Delete: `engine/tests/crossfade_eq_test.rs`
- Move/Rewrite: `engine/tests/mixwindow_ramp_test.rs` → fold into `engine/tests/deck_test.rs` (or delete if `deck_test.rs` already covers ramps)
- Rewrite: `engine/tests/mixer_integration_test.rs`

- [ ] **Step 1: Slim `crossfade_test.rs`**

Open `engine/tests/crossfade_test.rs`. Delete every test that references `mix_crossfade` or `mix_crossfade_eq` or `BassMixHint`. Keep tests for `equal_power_curve`. The file should end up with maybe one or two tests.

- [ ] **Step 2: Delete `crossfade_eq_test.rs`**

Run: `git rm engine/tests/crossfade_eq_test.rs`

- [ ] **Step 3: Delete `mixwindow_ramp_test.rs`**

If `deck_test.rs::tempo_linear_ramp_swaps_to_post_ramp_decoder` covers the cases, delete `engine/tests/mixwindow_ramp_test.rs` via `git rm`. Otherwise, port the test cases into `deck_test.rs` first, then delete.

- [ ] **Step 4: Rewrite `mixer_integration_test.rs`**

Replace `engine/tests/mixer_integration_test.rs` contents with a single end-to-end exercise:

```rust
use dj_engine::buffer::{make_ring, Consumer};
use dj_engine::deck::DeckId;
use dj_engine::ffmpeg::SAMPLE_RATE;
use dj_engine::library::LoadedTrack;
use dj_engine::mixer::{tick_until, Mixer};
use dj_engine::recipe::EqualPowerCrossfade;
use dj_engine::scheduler::TransitionScheduler;
use dj_engine::sidecar::Sidecar;
use dj_engine::transition::TransitionPlan;
use std::path::PathBuf;

fn track(name: &str, dur: f64) -> LoadedTrack {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}"));
    LoadedTrack {
        sidecar: Sidecar {
            schema_version: 1,
            analyzer_version: "test".into(),
            source_path: path.to_string_lossy().into_owned(),
            source_mtime: 0.0,
            source_size_bytes: 0,
            duration_seconds: dur,
            sample_rate: 44100,
            bpm: Some(120.0),
            bpm_confidence: Some(1.0),
            beats_seconds: None,
            downbeats_seconds: None,
            key: None,
            key_label: None,
            key_confidence: None,
            energy_curve: None,
            energy_mean: Some(0.5),
            band_energy_means: None,
            cue_in_seconds: Some(0.0),
            cue_out_seconds: Some(dur),
            intro_end_seconds: Some(0.0),
            outro_start_seconds: Some(dur),
            is_mixable: Some(true),
            mixable_reasons: None,
            error: None,
            ssm: None,
            novelty: None,
            rms_db: None,
        },
    }
}

#[tokio::test]
async fn full_transition_outputs_continuous_audio() {
    let cap = (SAMPLE_RATE as usize) * 2 * 10;
    let (producer, mut consumer) = make_ring(cap);
    let mut mixer = Mixer::new(producer);

    let outgoing = track("sine_440_6s.wav", 6.0);
    let incoming = track("sine_660_6s.wav", 6.0);

    // Cue + play outgoing on Deck A.
    {
        let d = mixer.deck(DeckId::A);
        d.cue(outgoing.clone(), 0.0).await.unwrap();
        d.play();
        d.test_set_fader(1.0);
    }

    // Drain ~0.5 s of A solo so the consumer test can see audio before xfade.
    tick_until(&mut mixer, 22_050).await.unwrap();

    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.5,
        incoming_crossfade_start_seconds: 1.0,
        length_bars: 8,
        use_eq_swap: false,
    };
    let handle = TransitionScheduler::arm(
        &mut mixer,
        DeckId::A,
        DeckId::B,
        &outgoing,
        &incoming,
        &plan,
        &EqualPowerCrossfade,
    )
    .await
    .unwrap();
    tick_until(&mut mixer, handle.end_master_clock_frames).await.unwrap();

    // Drain everything we wrote into the ring.
    let mut all = Vec::new();
    let mut buf = vec![0f32; 4096];
    loop {
        let n = consumer.pop_slice(&mut buf);
        if n == 0 { break; }
        all.extend_from_slice(&buf[..n]);
    }
    assert!(!all.is_empty());
    // Check for a long run of zeros (underrun).
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

- [ ] **Step 5: Verify**

Run: `cd engine && cargo test`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```
git add -A engine/tests/
git commit -m "test(engine): rewrite mixer integration; drop crossfade_eq + mixwindow_ramp tests (sub-project 1, task 21)"
```

---

### Task 22: Final verification

**Files:** none — just verification commands.

- [ ] **Step 1: `cargo fmt`**

Run: `cd engine && cargo fmt --check`
Expected: clean. If not, run `cargo fmt` and commit any formatting changes.

- [ ] **Step 2: `cargo clippy`**

Run: `cd engine && cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 3: Full test suite**

Run: `cd engine && cargo test`
Expected: every test passes.

- [ ] **Step 4: Build release**

Run: `cd engine && cargo build --release`
Expected: success.

- [ ] **Step 5: Commit any final cleanup**

If there are formatting changes:

```
git add -A
git commit -m "chore(engine): cargo fmt after sub-project 1"
```

If everything was already clean, skip this step.

---

## Self-Review notes

Coverage check:
- Spec Section "Component 1 — Deck" → Tasks 7-14.
- Spec Section "Component 2 — Automation" → Tasks 1-3.
- Spec Section "Component 3 — Recipe" → Tasks 4-6.
- Spec Section "Component 4 — Mixer" → Task 15.
- Spec Section "Component 5 — TransitionScheduler" → Task 16.
- Spec Section "Migration plan" → Tasks 17-21.
- Spec Section "Testing strategy" → Tasks 1-16 each include unit tests; Task 21 covers integration.
- Spec "Known temporary regressions" (bass-swap disappears) → documented in commit message of Task 18, behavior preserved by retaining `equal_power_curve` (Task 18) and selecting only EqualPower/HardCut in `pick_recipe` (Task 6).

Out-of-band risks tracked:
- **Drop-in A/B verification** — the spec called for a programmatic A/B test against a captured baseline. This plan opts to verify drop-in equivalence only through the integration test (continuity + frequency content) and **manual listening A/B by the user** (handled outside this plan). A captured-fixture byte comparison would require keeping `play_until_transition_and_crossfade` alive through the implementation, which conflicts with the linear refactor. If the user wants tighter automated verification, add a follow-up task to render fixtures at the start of the branch with the old code, then compare in Task 21.
