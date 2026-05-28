# Two-Deck Mixer — Scored Recipe Selection (Sub-project 5)

**Date:** 2026-05-28
**Status:** Approved for planning
**Branch:** `research/two-deck-mixer`
**Sub-project:** 5 of 5 (final)

## Summary

Replace the plan-only `pick_recipe(plan)` decision with a **scored picker** that takes both the `TransitionPlan` and the actual `(outgoing, incoming)` pair, scores all available recipes against the pair's properties (energy delta, bass content, phrase alignment, key match), applies a **recipe-recency penalty** so the same recipe doesn't fire too often in a row, and picks the highest-scoring recipe with a small random jitter for variety. `FilterFade`, `FilterKill`, `EchoOut`, `LoopOut` — currently dormant unless `--force-recipe` is set — become naturally selectable based on the pair. `--force-recipe` stays as an override for manual testing.

## Goals

- Replace `pick_recipe(plan: &TransitionPlan) -> RecipeChoice` with `pick_recipe(plan, outgoing, incoming, recency) -> RecipeChoice`.
- Add a `RecipeRecency` ring (capacity 5) in a new `engine/src/recipe_recency.rs`. Same idea as the existing `RecencyRing<String>` for track paths.
- Score each `RecipeChoice` for the pair using simple rules drawn from energy delta, bass-content classification, phrase alignment, and key match. Apply a recency penalty so a recipe used recently scores lower. Tiebreak with small random jitter.
- Wire `RecipeRecency` through `main.rs`'s main loop and record the chosen recipe after each transition.
- `--force-recipe` still wins when set. Without it, scored selection picks.

## Non-Goals (this sub-project)

- No change to the `Selector` (track picker). Recipe choice is downstream of track choice — they don't co-optimize. (A future sub-project could co-score `(track, recipe)` pairs together if needed.)
- No change to individual recipes' `build()` logic.
- No change to `transition_type_label`: it still describes the **planner's** intent (e.g., `bass_swap_standard_blend`). The arming log shows both `transition_type` (intent) and `recipe` (realized) — when they diverge, it's because scored selection picked something other than the planner's natural choice.
- No reusable "transition policy" config file. Scoring weights are constants in code; tune by editing source.

## Scoring rules

For a `(plan, outgoing, incoming, recency)` tuple, each recipe gets a fit score in `[0.0, 1.0]`:

| Recipe | Fit score |
|---|---|
| `HardCut` | `1.0` if `plan.length_bars == 0`, else `0.0`. (Plan-driven; not scored against other recipes — hard cuts are absolute.) |
| `EqualPowerCrossfade` | `0.5` baseline — always a safe default. |
| `BassSwap` | `0.85` if `phrase_aligned && both_bass_heavy (>0.5)`; `0.45` if `phrase_aligned && at_least_one_bass_heavy`; `0.10` otherwise. (Captures the legacy `use_eq_swap` intuition but more granular.) |
| `FilterFade` | `0.75` if `energy_delta > 0.20` (clear build-up); `0.40` if `energy_delta > 0.05` (mild build-up); `0.0` otherwise. (`energy_delta = incoming.energy - outgoing.energy`.) |
| `FilterKill` | `0.70` if `energy_delta < -0.20` (clear wind-down); `0.40` if `energy_delta < -0.05` (mild wind-down); `0.0` otherwise. |
| `EchoOut` | `0.65` if `energy_delta < -0.15`; `0.35` baseline (somewhat versatile, especially for "wind down" feel). |
| `LoopOut` | `0.30` baseline (stylistic, infrequent). |

Then apply a **recency penalty**: if a recipe is in `recency` (the ring of the last N choices), its score is multiplied by `0.5`. Hardcut bypasses the penalty (it's always correct when chosen).

Then a small **uniform jitter** in `[0.0, 0.04]` is added to break ties between recipes scoring close together (e.g., `EqualPower` 0.5 vs `EchoOut` 0.35 with one penalty applied).

The highest-scoring recipe wins. Ties (after jitter, theoretically impossible but defensively) break to `EqualPower`.

## Hardcut short-circuit

When `plan.length_bars == 0`, the scored picker still returns `HardCut` unconditionally (matching `plan_transition`'s hard-cut decision for big energy jumps). Hard cuts are an audible cliff and must NOT be subject to recipe-recency dilution.

## Variety knob

The `RecipeRecency` ring's capacity (default `5`) controls how aggressively the picker rotates recipes. With capacity 5 and the 0.5 penalty: a recipe just used drops below most alternatives, so the next 1-2 transitions will pick something else, and after ~5 transitions the penalty clears. Adjustable via `--recipe-recency-window <N>` CLI flag (0 disables the penalty entirely, useful for debugging score behavior).

## Components

### Component 1 — `RecipeRecency`

New file `engine/src/recipe_recency.rs`:

```rust
use std::collections::VecDeque;

use crate::recipe::RecipeChoice;

pub struct RecipeRecency {
    ring: VecDeque<RecipeChoice>,
    capacity: usize,
}

impl RecipeRecency {
    pub fn new(capacity: usize) -> Self {
        Self { ring: VecDeque::with_capacity(capacity.max(1)), capacity }
    }

    pub fn record(&mut self, choice: RecipeChoice) {
        if self.capacity == 0 { return; }
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

### Component 2 — Scoring inputs

A small helper in `recipe.rs` reads the pair's properties (so the scorer can be unit-tested without async):

```rust
pub struct PairScoreInputs {
    pub length_bars: usize,
    pub use_eq_swap: bool,
    pub energy_delta: f64,        // incoming - outgoing
    pub outgoing_bass: f64,       // 0..1, default 0.5 when missing
    pub incoming_bass: f64,
    pub phrase_aligned: bool,
}

impl PairScoreInputs {
    pub fn from_pair(plan: &TransitionPlan, outgoing: &LoadedTrack, incoming: &LoadedTrack) -> Self {
        let outgoing_energy = outgoing.sidecar.energy_mean.unwrap_or(0.5);
        let incoming_energy = incoming.sidecar.energy_mean.unwrap_or(0.5);
        let outgoing_bass = outgoing.sidecar.band_energy_means.as_ref().map(|b| b.bass).unwrap_or(0.5);
        let incoming_bass = incoming.sidecar.band_energy_means.as_ref().map(|b| b.bass).unwrap_or(0.5);
        let phrase_aligned = outgoing.sidecar.phrase_aligned() && incoming.sidecar.phrase_aligned();
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
```

### Component 3 — Scoring function

```rust
pub fn score_recipe(choice: RecipeChoice, inputs: &PairScoreInputs) -> f64 {
    match choice {
        RecipeChoice::HardCut => if inputs.length_bars == 0 { 1.0 } else { 0.0 },
        RecipeChoice::EqualPower => 0.5,
        RecipeChoice::BassSwap => {
            let both_heavy = inputs.outgoing_bass > 0.5 && inputs.incoming_bass > 0.5;
            let any_heavy = inputs.outgoing_bass > 0.5 || inputs.incoming_bass > 0.5;
            if inputs.phrase_aligned && both_heavy { 0.85 }
            else if inputs.phrase_aligned && any_heavy { 0.45 }
            else { 0.10 }
        }
        RecipeChoice::FilterFade => {
            if inputs.energy_delta > 0.20 { 0.75 }
            else if inputs.energy_delta > 0.05 { 0.40 }
            else { 0.0 }
        }
        RecipeChoice::FilterKill => {
            if inputs.energy_delta < -0.20 { 0.70 }
            else if inputs.energy_delta < -0.05 { 0.40 }
            else { 0.0 }
        }
        RecipeChoice::EchoOut => {
            if inputs.energy_delta < -0.15 { 0.65 } else { 0.35 }
        }
        RecipeChoice::LoopOut => 0.30,
    }
}
```

### Component 4 — Scored `pick_recipe`

Replace `recipe.rs::pick_recipe` (keep the old signature as `pick_recipe_plan_only` for tests / fallback):

```rust
pub fn pick_recipe(
    plan: &TransitionPlan,
    outgoing: &LoadedTrack,
    incoming: &LoadedTrack,
    recency: &RecipeRecency,
) -> RecipeChoice {
    // Hard-cut short-circuit. Always honored, never recency-penalized.
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
    for choice in candidates {
        let mut score = score_recipe(choice, &inputs);
        if recency.contains(choice) {
            score *= 0.5;
        }
        // Deterministic jitter from (choice index + a per-call salt) so tests can
        // be deterministic. The salt is the recency length, so the jitter changes
        // each transition.
        let salt = (recency.contains(choice) as u64 * 17) + (choice as u64);
        let jitter = ((salt * 2654435761u64) % 1000) as f64 / 1000.0 * jitter_range;
        score += jitter;
        if score > best.1 {
            best = (choice, score);
        }
    }
    best.0
}
```

(Note: the deterministic jitter trick avoids pulling in `rand` for this function and lets unit tests exercise specific score outcomes. A bigger jitter spread or `thread_rng()` would also work; deterministic is easier for tests.)

Keep `pick_recipe_plan_only(plan)` for any test that wants to verify the legacy hard-cut/bass-swap routing:

```rust
pub fn pick_recipe_plan_only(plan: &TransitionPlan) -> RecipeChoice {
    if plan.length_bars == 0 { RecipeChoice::HardCut }
    else if plan.use_eq_swap { RecipeChoice::BassSwap }
    else { RecipeChoice::EqualPower }
}
```

### Component 5 — `RecipeChoice: Hash + Eq + Copy`

Already derives `Copy, Clone, Debug, PartialEq, Eq`. For `recency.contains` we use linear scan, so no Hash needed.

### Component 6 — `main.rs` integration

Add to imports: `use dj_engine::recipe_recency::RecipeRecency;`

In the main fn, before the loop:

```rust
let recipe_recency_window: usize = cli.recipe_recency_window.unwrap_or(5);
let mut recipe_recency = RecipeRecency::new(recipe_recency_window);
```

Replace the choice computation:

```rust
let natural_choice = dj_engine::recipe::pick_recipe(
    &plan, &current, &next_track, &recipe_recency
);
let choice = cli
    .force_recipe
    .map(|fr| fr.to_recipe_choice())
    .unwrap_or(natural_choice);
```

After the arm + tick_until, record:

```rust
recipe_recency.record(choice);
```

### Component 7 — CLI flag

Edit `engine/src/config.rs`:

```rust
/// How many recent recipe choices to penalize when scoring the next transition.
/// 0 disables the penalty (useful for debugging score behavior). Default 5.
#[arg(long)]
pub recipe_recency_window: Option<usize>,
```

## Testing strategy

### Unit tests

- **`recipe_recency.rs`**:
  - `record` / `contains` semantics; ring evicts oldest after capacity.
  - `capacity == 0` is a no-op (never marks anything as recent).
- **`recipe.rs`** scoring:
  - `score_recipe(HardCut, length_bars=0)` returns 1.0.
  - `score_recipe(HardCut, length_bars=16)` returns 0.0.
  - `score_recipe(BassSwap, both_heavy + phrase_aligned)` ≥ 0.85.
  - `score_recipe(BassSwap, not phrase_aligned)` ≤ 0.10.
  - `score_recipe(FilterFade, energy_delta=0.3)` ≥ 0.75.
  - `score_recipe(FilterFade, energy_delta=-0.3)` == 0.0.
  - `score_recipe(FilterKill, energy_delta=-0.3)` ≥ 0.70.
  - `score_recipe(EchoOut, energy_delta=-0.5)` ≥ 0.65.
- **`recipe.rs::pick_recipe`**:
  - Hard-cut plan (`length_bars=0`) returns `HardCut` regardless of pair / recency.
  - Bass-heavy phrase-aligned pair without recency returns `BassSwap`.
  - Same pair, with `BassSwap` in recency → picks something else (BassSwap penalty drops it to ~0.42, EqualPower's 0.5 + jitter wins).
  - Big energy boost (`+0.4`) returns `FilterFade`.
  - Big energy drop (`-0.4`) returns `FilterKill` or `EchoOut` (both score high; recency or jitter decides — assert the result is one of those two).

### Integration test

Optional: run a synthetic main-loop simulation with hand-crafted sidecar pairs and verify the picked-recipe sequence shows variety. Probably overkill; the unit tests cover the logic.

## Risks

- **`LoopOut` and `EchoOut` may rarely fire under the default weights** because their baseline scores (0.30, 0.35) are below `EqualPower`'s 0.50. They surface mostly via the recency penalty knocking down EqualPower or BassSwap after several consecutive uses. Acceptable — these recipes are stylistic flourishes, not workhorses. If they sound too rare, bump their baseline scores or shrink EqualPower's.
- **`FilterFade` / `FilterKill` may dominate on libraries with widely-varying energy.** If the library has many >0.2 energy deltas, the filter recipes will win most transitions. Tune by tightening the energy thresholds or lowering the win scores.
- **Random jitter is deterministic per call but varies across transitions** (because the recency state changes). Tests pin specific recency states for reproducibility.
- **No co-optimization with track selection.** The selector may pick a track that happens to make `EqualPower` the best fit when a different track would have enabled `BassSwap`. Acceptable for v1; future enhancement.

## Open questions

- The scoring constants are hand-tuned by intuition. After listening, the user may want to bump some up or down. Easy follow-up — single file, one function.

## Roadmap (after sub-project 5)

This is the last planned sub-project for this branch. With sub-project 5 merged, the engine will:

1. Pick tracks via the existing selector (BPM/key/energy + recency).
2. Plan the transition boundary (snap to phrase, choose length).
3. **Score and pick a recipe based on the pair's properties + recipe recency** — new.
4. Arm and execute the transition.

Possible future side-quests if the user wants:

- **Phase-nudge**: detect transient-level beat drift in the crossfade window and apply a micro-shift to keep kicks aligned. Side-quest mentioned earlier.
- **Live pitch slider** / control surface: turn the autonomous engine into an operator-driven DJ rig. Mentioned in the earlier "what about real DJ board?" exchange.
- **Co-optimized (track, recipe) selection**: have the selector consider what recipe each candidate would enable when ranking tracks.
- **Tunable scoring policy**: load weights from a config file so the user can adjust without recompiling.

## Project layout changes

```
engine/src/
  recipe_recency.rs          ← NEW: RecipeRecency ring
  lib.rs                     ← +pub mod recipe_recency;
  recipe.rs                  ← +score_recipe, +PairScoreInputs, replace pick_recipe
                               signature, keep pick_recipe_plan_only for tests
  config.rs                  ← +recipe_recency_window: Option<usize>
  main.rs                    ← instantiate RecipeRecency, pass to pick_recipe,
                               record after each transition
engine/tests/
  recipe_recency_test.rs     ← NEW: ring behavior
  recipe_test.rs             ← +score_recipe tests, +pick_recipe (scored) tests
```
