# Scored Recipe Selection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace plan-only `pick_recipe(plan)` with a scored picker `pick_recipe(plan, outgoing, incoming, recency) -> RecipeChoice` that ranks all 7 recipes against the pair's properties and a recipe-recency penalty. Adds a `RecipeRecency` ring and a `--recipe-recency-window` CLI flag.

**Architecture:** Pure functions in `recipe.rs` (`score_recipe`, `PairScoreInputs::from_pair`, `pick_recipe`) + a new `recipe_recency.rs` for the ring. Main loop instantiates the ring, passes it to `pick_recipe`, records the chosen recipe after each transition. `--force-recipe` still wins when set.

**Tech Stack:** Rust 2021, no new dependencies, deterministic jitter via hash mixing.

**Spec:** `docs/superpowers/specs/2026-05-28-two-deck-mixer-scored-selection-design.md`

**Working branch:** `research/two-deck-mixer` (continues from sub-project 4).

---

## File structure

| Path | Action | Responsibility |
|---|---|---|
| `engine/src/recipe_recency.rs` | CREATE | `RecipeRecency` ring |
| `engine/src/lib.rs` | MODIFY | `pub mod recipe_recency;` |
| `engine/src/recipe.rs` | MODIFY | `PairScoreInputs`, `score_recipe`, replace `pick_recipe`, keep `pick_recipe_plan_only` |
| `engine/src/config.rs` | MODIFY | `Cli.recipe_recency_window: Option<usize>` |
| `engine/src/main.rs` | MODIFY | Instantiate `RecipeRecency`, pass to `pick_recipe`, record after each transition |
| `engine/tests/recipe_recency_test.rs` | CREATE | Ring unit tests |
| `engine/tests/recipe_test.rs` | MODIFY | `score_recipe` + scored `pick_recipe` tests |

---

## Tasks

### Task 1: `RecipeRecency` ring

**Files:**
- Create: `engine/src/recipe_recency.rs`
- Modify: `engine/src/lib.rs`
- Create: `engine/tests/recipe_recency_test.rs`

- [ ] **Step 1: Add module declaration**

Edit `engine/src/lib.rs`. Add `pub mod recipe_recency;` (alphabetical — after `pub mod recipe;`).

- [ ] **Step 2: Write failing tests**

Create `engine/tests/recipe_recency_test.rs`:

```rust
use dj_engine::recipe::RecipeChoice;
use dj_engine::recipe_recency::RecipeRecency;

#[test]
fn new_ring_contains_nothing() {
    let r = RecipeRecency::new(5);
    assert!(!r.contains(RecipeChoice::EqualPower));
    assert!(!r.contains(RecipeChoice::HardCut));
}

#[test]
fn record_then_contains() {
    let mut r = RecipeRecency::new(5);
    r.record(RecipeChoice::BassSwap);
    assert!(r.contains(RecipeChoice::BassSwap));
    assert!(!r.contains(RecipeChoice::EqualPower));
}

#[test]
fn ring_evicts_oldest_at_capacity() {
    let mut r = RecipeRecency::new(2);
    r.record(RecipeChoice::EqualPower);
    r.record(RecipeChoice::BassSwap);
    assert!(r.contains(RecipeChoice::EqualPower));
    r.record(RecipeChoice::FilterFade);
    // EqualPower should be evicted now.
    assert!(!r.contains(RecipeChoice::EqualPower));
    assert!(r.contains(RecipeChoice::BassSwap));
    assert!(r.contains(RecipeChoice::FilterFade));
}

#[test]
fn capacity_zero_disables_recency() {
    let mut r = RecipeRecency::new(0);
    r.record(RecipeChoice::BassSwap);
    assert!(!r.contains(RecipeChoice::BassSwap));
}
```

- [ ] **Step 3: Verify failure**

Run: `cd engine && cargo test --test recipe_recency_test`
Expected: FAIL — `RecipeRecency` doesn't exist.

- [ ] **Step 4: Implement `RecipeRecency`**

Create `engine/src/recipe_recency.rs`:

```rust
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
            ring: VecDeque::with_capacity(capacity),
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
```

- [ ] **Step 5: Verify pass**

Run: `cd engine && cargo test --test recipe_recency_test`
Expected: 4 tests pass.

- [ ] **Step 6: Commit**

```
git add engine/src/lib.rs engine/src/recipe_recency.rs engine/tests/recipe_recency_test.rs
git commit -m "feat(engine): RecipeRecency ring (sub-project 5, task 1)"
```

---

### Task 2: `score_recipe` + `PairScoreInputs`

**Files:**
- Modify: `engine/src/recipe.rs`
- Modify: `engine/tests/recipe_test.rs`

- [ ] **Step 1: Write failing tests**

Append to `engine/tests/recipe_test.rs`:

```rust
use dj_engine::recipe::{score_recipe, PairScoreInputs};

fn inputs_default() -> PairScoreInputs {
    PairScoreInputs {
        length_bars: 16,
        use_eq_swap: false,
        energy_delta: 0.0,
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
    }
}

#[test]
fn score_hard_cut_only_when_length_zero() {
    let mut i = inputs_default();
    i.length_bars = 0;
    assert_eq!(score_recipe(RecipeChoice::HardCut, &i), 1.0);
    i.length_bars = 16;
    assert_eq!(score_recipe(RecipeChoice::HardCut, &i), 0.0);
}

#[test]
fn score_equal_power_is_baseline_half() {
    let i = inputs_default();
    assert!((score_recipe(RecipeChoice::EqualPower, &i) - 0.5).abs() < 1e-9);
}

#[test]
fn score_bass_swap_high_when_both_heavy_and_phrase_aligned() {
    let mut i = inputs_default();
    i.outgoing_bass = 0.7;
    i.incoming_bass = 0.7;
    i.phrase_aligned = true;
    assert!(score_recipe(RecipeChoice::BassSwap, &i) > 0.8);
}

#[test]
fn score_bass_swap_low_when_not_phrase_aligned() {
    let mut i = inputs_default();
    i.outgoing_bass = 0.7;
    i.incoming_bass = 0.7;
    i.phrase_aligned = false;
    assert!(score_recipe(RecipeChoice::BassSwap, &i) < 0.2);
}

#[test]
fn score_filter_fade_rises_with_energy_boost() {
    let mut i = inputs_default();
    i.energy_delta = 0.3;
    assert!(score_recipe(RecipeChoice::FilterFade, &i) > 0.7);
    i.energy_delta = 0.10;
    assert!(score_recipe(RecipeChoice::FilterFade, &i) > 0.35);
    assert!(score_recipe(RecipeChoice::FilterFade, &i) < 0.45);
    i.energy_delta = -0.3;
    assert!(score_recipe(RecipeChoice::FilterFade, &i) < 0.01);
}

#[test]
fn score_filter_kill_rises_with_energy_drop() {
    let mut i = inputs_default();
    i.energy_delta = -0.3;
    assert!(score_recipe(RecipeChoice::FilterKill, &i) > 0.6);
    i.energy_delta = 0.3;
    assert!(score_recipe(RecipeChoice::FilterKill, &i) < 0.01);
}

#[test]
fn score_echo_out_rises_on_wind_down() {
    let mut i = inputs_default();
    i.energy_delta = -0.5;
    assert!(score_recipe(RecipeChoice::EchoOut, &i) > 0.6);
    i.energy_delta = 0.0;
    let baseline = score_recipe(RecipeChoice::EchoOut, &i);
    assert!(baseline > 0.3 && baseline < 0.4);
}

#[test]
fn score_loop_out_is_low_baseline() {
    let i = inputs_default();
    assert!(score_recipe(RecipeChoice::LoopOut, &i) < 0.35);
}
```

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test recipe_test`
Expected: FAIL — `score_recipe`, `PairScoreInputs` don't exist.

- [ ] **Step 3: Implement `PairScoreInputs` + `score_recipe`**

Append to `engine/src/recipe.rs`:

```rust
/// Pre-computed inputs the recipe scorer needs. Factoring out the sidecar
/// dereference makes the scorer trivially unit-testable.
#[derive(Clone, Debug)]
pub struct PairScoreInputs {
    pub length_bars: usize,
    pub use_eq_swap: bool,
    /// `incoming.energy_mean - outgoing.energy_mean` (positive = ramp up).
    pub energy_delta: f64,
    /// `band_energy_means.bass` from each sidecar; defaults to 0.5 when absent.
    pub outgoing_bass: f64,
    pub incoming_bass: f64,
    /// True iff BOTH tracks are marked `is_phrase_aligned`.
    pub phrase_aligned: bool,
}

impl PairScoreInputs {
    pub fn from_pair(
        plan: &TransitionPlan,
        outgoing: &LoadedTrack,
        incoming: &LoadedTrack,
    ) -> Self {
        let outgoing_energy = outgoing.sidecar.energy_mean.unwrap_or(0.5);
        let incoming_energy = incoming.sidecar.energy_mean.unwrap_or(0.5);
        let outgoing_bass = outgoing
            .sidecar
            .band_energy_means
            .as_ref()
            .map(|b| b.bass)
            .unwrap_or(0.5);
        let incoming_bass = incoming
            .sidecar
            .band_energy_means
            .as_ref()
            .map(|b| b.bass)
            .unwrap_or(0.5);
        let phrase_aligned =
            outgoing.sidecar.phrase_aligned() && incoming.sidecar.phrase_aligned();
        Self {
            length_bars: plan.length_bars,
            use_eq_swap: plan.use_eq_swap,
            energy_delta: incoming_energy - outgoing_energy,
            outgoing_bass,
            incoming_bass,
            phrase_aligned,
        }
    }
}

/// Score a recipe's fit for the pair. Higher = better. Score range [0, 1].
pub fn score_recipe(choice: RecipeChoice, inputs: &PairScoreInputs) -> f64 {
    match choice {
        RecipeChoice::HardCut => {
            if inputs.length_bars == 0 {
                1.0
            } else {
                0.0
            }
        }
        RecipeChoice::EqualPower => 0.5,
        RecipeChoice::BassSwap => {
            let both_heavy = inputs.outgoing_bass > 0.5 && inputs.incoming_bass > 0.5;
            let any_heavy = inputs.outgoing_bass > 0.5 || inputs.incoming_bass > 0.5;
            if inputs.phrase_aligned && both_heavy {
                0.85
            } else if inputs.phrase_aligned && any_heavy {
                0.45
            } else {
                0.10
            }
        }
        RecipeChoice::FilterFade => {
            if inputs.energy_delta > 0.20 {
                0.75
            } else if inputs.energy_delta > 0.05 {
                0.40
            } else {
                0.0
            }
        }
        RecipeChoice::FilterKill => {
            if inputs.energy_delta < -0.20 {
                0.70
            } else if inputs.energy_delta < -0.05 {
                0.40
            } else {
                0.0
            }
        }
        RecipeChoice::EchoOut => {
            if inputs.energy_delta < -0.15 {
                0.65
            } else {
                0.35
            }
        }
        RecipeChoice::LoopOut => 0.30,
    }
}
```

- [ ] **Step 4: Verify pass**

Run: `cd engine && cargo test --test recipe_test`
Expected: existing tests + 8 new score_recipe tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/recipe.rs engine/tests/recipe_test.rs
git commit -m "feat(engine): score_recipe + PairScoreInputs (sub-project 5, task 2)"
```

---

### Task 3: Scored `pick_recipe`

**Files:**
- Modify: `engine/src/recipe.rs`
- Modify: `engine/tests/recipe_test.rs`

- [ ] **Step 1: Write failing tests**

Append to `engine/tests/recipe_test.rs`:

```rust
use dj_engine::recipe_recency::RecipeRecency;

fn bass_heavy_phrase_aligned_pair() -> (LoadedTrack, LoadedTrack) {
    let mut a = track_with_bass(120.0, 0.7);
    let mut b = track_with_bass(120.0, 0.7);
    a.sidecar.is_phrase_aligned = Some(true);
    b.sidecar.is_phrase_aligned = Some(true);
    (a, b)
}

fn standard_blend_plan() -> TransitionPlan {
    TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
    }
}

#[test]
fn scored_picks_hard_cut_when_length_zero() {
    let (a, b) = bass_heavy_phrase_aligned_pair();
    let mut plan = standard_blend_plan();
    plan.length_bars = 0;
    let recency = RecipeRecency::new(5);
    assert_eq!(pick_recipe(&plan, &a, &b, &recency), RecipeChoice::HardCut);
}

#[test]
fn scored_picks_bass_swap_for_heavy_aligned_pair_no_recency() {
    let (a, b) = bass_heavy_phrase_aligned_pair();
    let plan = standard_blend_plan();
    let recency = RecipeRecency::new(5);
    assert_eq!(pick_recipe(&plan, &a, &b, &recency), RecipeChoice::BassSwap);
}

#[test]
fn scored_skips_recent_bass_swap() {
    let (a, b) = bass_heavy_phrase_aligned_pair();
    let plan = standard_blend_plan();
    let mut recency = RecipeRecency::new(5);
    recency.record(RecipeChoice::BassSwap);
    // Penalized 0.85 * 0.5 = 0.425, below EqualPower's 0.5. Picker should
    // switch to EqualPower (or EchoOut at 0.35, but EqualPower is higher).
    let pick = pick_recipe(&plan, &a, &b, &recency);
    assert_ne!(pick, RecipeChoice::BassSwap, "should skip recently-used BassSwap");
}

#[test]
fn scored_picks_filter_fade_for_big_energy_boost() {
    let mut a = track_with_bass(120.0, 0.5);
    let mut b = track_with_bass(120.0, 0.5);
    a.sidecar.energy_mean = Some(0.3);
    b.sidecar.energy_mean = Some(0.7); // +0.4 boost
    a.sidecar.is_phrase_aligned = Some(false);
    b.sidecar.is_phrase_aligned = Some(false);
    let plan = standard_blend_plan();
    let recency = RecipeRecency::new(5);
    assert_eq!(pick_recipe(&plan, &a, &b, &recency), RecipeChoice::FilterFade);
}

#[test]
fn scored_picks_filter_kill_or_echo_out_for_big_energy_drop() {
    let mut a = track_with_bass(120.0, 0.5);
    let mut b = track_with_bass(120.0, 0.5);
    a.sidecar.energy_mean = Some(0.8);
    b.sidecar.energy_mean = Some(0.4); // -0.4 drop
    a.sidecar.is_phrase_aligned = Some(false);
    b.sidecar.is_phrase_aligned = Some(false);
    let plan = standard_blend_plan();
    let recency = RecipeRecency::new(5);
    let pick = pick_recipe(&plan, &a, &b, &recency);
    assert!(
        pick == RecipeChoice::FilterKill || pick == RecipeChoice::EchoOut,
        "expected FilterKill or EchoOut, got {pick:?}"
    );
}

#[test]
fn pick_recipe_plan_only_preserved_for_legacy_tests() {
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.0,
        incoming_crossfade_start_seconds: 0.0,
        length_bars: 16,
        use_eq_swap: true,
    };
    assert_eq!(pick_recipe_plan_only(&plan), RecipeChoice::BassSwap);
}
```

(Note: the existing `pick_recipe_*` tests that pass only a `plan` need to be updated to call `pick_recipe_plan_only` since `pick_recipe`'s signature changed.)

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test recipe_test`
Expected: FAIL — `pick_recipe` signature mismatch, `pick_recipe_plan_only` doesn't exist.

- [ ] **Step 3: Implement scored `pick_recipe`**

Edit `engine/src/recipe.rs`. **Rename** the existing `pick_recipe` to `pick_recipe_plan_only`, then add the new scored version:

```rust
/// Legacy plan-only picker. Kept for tests and as a reference baseline.
pub fn pick_recipe_plan_only(plan: &TransitionPlan) -> RecipeChoice {
    if plan.length_bars == 0 {
        RecipeChoice::HardCut
    } else if plan.use_eq_swap {
        RecipeChoice::BassSwap
    } else {
        RecipeChoice::EqualPower
    }
}

/// Scored picker: ranks all 7 recipes against the pair's properties and a
/// recency penalty. Hard-cut shortcuts when `plan.length_bars == 0`.
pub fn pick_recipe(
    plan: &TransitionPlan,
    outgoing: &LoadedTrack,
    incoming: &LoadedTrack,
    recency: &crate::recipe_recency::RecipeRecency,
) -> RecipeChoice {
    if plan.length_bars == 0 {
        return RecipeChoice::HardCut;
    }
    let inputs = PairScoreInputs::from_pair(plan, outgoing, incoming);
    let candidates = [
        RecipeChoice::EqualPower,
        RecipeChoice::BassSwap,
        RecipeChoice::FilterFade,
        RecipeChoice::FilterKill,
        RecipeChoice::EchoOut,
        RecipeChoice::LoopOut,
    ];

    let mut best = (RecipeChoice::EqualPower, f64::MIN);
    let jitter_range = 0.04;
    for (idx, choice) in candidates.iter().enumerate() {
        let mut score = score_recipe(*choice, &inputs);
        if recency.contains(*choice) {
            score *= 0.5;
        }
        // Deterministic jitter that varies per call: hash the recency length
        // mixed with the candidate index. Stable for tests with a fixed
        // recency state.
        let salt = (idx as u64).wrapping_mul(2654435761).wrapping_add(recency_hash(recency));
        let jitter = (salt % 1000) as f64 / 1000.0 * jitter_range;
        score += jitter;
        if score > best.1 {
            best = (*choice, score);
        }
    }
    best.0
}

fn recipe_choice_id(c: RecipeChoice) -> u64 {
    // Stable, ordered id used in the deterministic-jitter mix below.
    match c {
        RecipeChoice::EqualPower => 1,
        RecipeChoice::HardCut => 2,
        RecipeChoice::BassSwap => 3,
        RecipeChoice::FilterFade => 4,
        RecipeChoice::FilterKill => 5,
        RecipeChoice::EchoOut => 6,
        RecipeChoice::LoopOut => 7,
    }
}

fn recency_hash(recency: &crate::recipe_recency::RecipeRecency) -> u64 {
    // Fold the recency's current contents into a u64. Since RecipeRecency
    // doesn't expose iteration, mix in the "contains" result for each choice.
    let mut h: u64 = 0x9e3779b97f4a7c15;
    for c in [
        RecipeChoice::EqualPower,
        RecipeChoice::HardCut,
        RecipeChoice::BassSwap,
        RecipeChoice::FilterFade,
        RecipeChoice::FilterKill,
        RecipeChoice::EchoOut,
        RecipeChoice::LoopOut,
    ] {
        let bit = if recency.contains(c) { 1 } else { 0 };
        h = h
            .wrapping_mul(0x100000001b3)
            .wrapping_add(recipe_choice_id(c) * 31 + bit);
    }
    h
}
```

- [ ] **Step 4: Update existing pick_recipe tests that call the old single-arg signature**

In `engine/tests/recipe_test.rs`, find any test that calls `pick_recipe(&plan)` (single-arg) and update it to call `pick_recipe_plan_only(&plan)` instead. The existing tests `pick_recipe_chooses_hard_cut_for_zero_bars`, `pick_recipe_chooses_equal_power_for_nonzero_bars`, and `pick_recipe_routes_use_eq_swap_to_bass_swap` need to switch.

- [ ] **Step 5: Verify pass**

Run: `cd engine && cargo test --test recipe_test`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```
git add engine/src/recipe.rs engine/tests/recipe_test.rs
git commit -m "feat(engine): scored pick_recipe with recency penalty + deterministic jitter (sub-project 5, task 3)"
```

---

### Task 4: CLI flag + `main.rs` integration

**Files:**
- Modify: `engine/src/config.rs`
- Modify: `engine/src/main.rs`

- [ ] **Step 1: Add `recipe_recency_window` field**

Edit `engine/src/config.rs`. Add to the `Cli` struct:

```rust
/// Penalize recipes used in the last N transitions when scoring (so the same
/// recipe doesn't fire repeatedly). Default 5. Pass 0 to disable.
#[arg(long)]
pub recipe_recency_window: Option<usize>,
```

- [ ] **Step 2: Wire `RecipeRecency` into main.rs**

Edit `engine/src/main.rs`. Add the import near the others:

```rust
use dj_engine::recipe_recency::RecipeRecency;
```

Before the main loop, after `let mut recency = RecencyRing::new(20);`, add:

```rust
let recipe_recency_window = cli.recipe_recency_window.unwrap_or(5);
let mut recipe_recency = RecipeRecency::new(recipe_recency_window);
```

Update the choice computation:

```rust
let natural_choice = dj_engine::recipe::pick_recipe(
    &plan, &current, &next_track, &recipe_recency,
);
let choice = cli
    .force_recipe
    .map(|fr| fr.to_recipe_choice())
    .unwrap_or(natural_choice);
```

After the transition completes (after `mixer.deck(current_deck).unload().await;` and the role swap, before the `recency.record(...)` for tracks), add:

```rust
recipe_recency.record(choice);
```

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
git commit -m "feat(engine): wire RecipeRecency through main loop + --recipe-recency-window (sub-project 5, task 4)"
```

---

### Task 5: Final verification

**Files:** none — verification only.

- [ ] **Step 1: `cargo fmt`**

Run: `cd engine && cargo fmt --check`
If output: `cargo fmt && git add -A && git commit -m "chore(engine): fmt after sub-project 5"`.

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
- Spec Component 1 (`RecipeRecency`) → Task 1.
- Spec Component 2 (`PairScoreInputs`) → Task 2.
- Spec Component 3 (`score_recipe`) → Task 2.
- Spec Component 4 (scored `pick_recipe` + legacy fallback) → Task 3.
- Spec Component 5 (no Hash needed) → N/A.
- Spec Component 6 (main.rs integration) → Task 4.
- Spec Component 7 (CLI flag) → Task 4.

No placeholders. Each code step is complete. Hard-cut short-circuit preserved through Task 3's `pick_recipe`. Recency penalty applies to non-hardcut recipes via the multiplier in Task 3's loop.
