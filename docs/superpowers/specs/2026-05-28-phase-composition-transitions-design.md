# Phase Composition for Transitions + Three New DJ Recipes

**Date:** 2026-05-28
**Status:** Approved for planning
**Branch:** `research/two-deck-mixer`
**Sub-project:** post-5 (architectural follow-up)

## Summary

Restructure the recipe layer so each `Recipe::build()` returns a **declarative `TransitionScript`** (a vector of `Phase`s expressed in musical time — bars/beats) instead of a `RecipeOutput` (a vector of frame-stamped `Automation`s). A new `compile(script, ctx) -> RecipeOutput` function is the single place where bar-time becomes frame-time. The existing 7 recipes get faithfully rewritten on the phase DSL (no behavioral change — same `Automation`s produced). Then add three new recipes that exercise the per-deck controls currently dead in transitions: `ThreeBandSwap` (touches EqMid + EqHigh, which no recipe touches today), `EchoThrow` (uses `EchoMix` as a one-shot step, not a ramp), and `HighsFirst` (incoming intro with bass + mids killed, opened in stages on downbeats). The picker grows three new scoring branches.

The phase DSL is designed so a future sub-project can plug an AI-generated `TransitionScript` into the same compile pipeline with no scheduler or deck changes — that's an explicit design constraint, but no AI code is in scope here.

## Motivation

Two concrete gaps in today's transition system:

1. **EqMid and EqHigh are unused.** Every recipe today either touches Fader only, or Fader + EqLow (in `BassSwap`), or Fader + Filter, or Fader + EchoMix/LoopGate. The 3-band EQ has been built (sub-project 2) but only one band is ever automated in a transition. A real DJ uses all three.
2. **Swap moments don't land on downbeats.** `BassSwap` schedules its EqLow kill at `0.35 * length_frames` and release at `0.65 * length_frames` — arbitrary fractions of the window. A 16-bar transition at 120 BPM ends up with the bass kill at bar 5.6, not bar 6. Listeners feel a clean bass swap when it lands on beat-1 of a bar; the current placement is acoustically random.

Phase composition fixes #2 structurally (the DSL talks bars, the compiler handles frames) and makes #1 easy to address by adding `ThreeBandSwap`. It also creates a small structured surface — ~8 phase variants — that an LLM can emit as JSON in a future sub-project, instead of frame-stamped automation vectors.

## Goals

- New module exposing `TransitionScript`, `Phase`, `BarPos`, `BarSpan`, `DeckRole`, `EqBand`, and a `compile(script, ctx) -> RecipeOutput` function. Compiler is panic-free and clamping.
- `Recipe::build()` signature changes from `RecipeOutput` to `TransitionScript`. Each existing recipe is faithfully rewritten (same `Automation`s produced; existing integration tests pass).
- Three new recipes: `ThreeBandSwap`, `EchoThrow`, `HighsFirst`. Three new variants in `RecipeChoice`. Three new branches in `score_recipe`. Three new `--force-recipe` values.
- Existing scheduler, mixer, deck, automation queue, sidecar format, and CLI shape are untouched (except `--force-recipe`'s enum gains three values).
- Serde derives on all DSL types so a future AI-source sub-project can deserialize JSON scripts directly.

## Non-Goals (this sub-project)

- **No re-alignment of existing recipes.** `BassSwap`'s `0.35/0.48/0.52/0.65` fractions get expressed as `BarPos::bars = 0.35 * length_bars` (etc.) in the migration so the produced `Automation`s are byte-identical to today. Re-aligning those fractions to clean downbeats is a behavioral change requiring its own ear-test and its own spec.
- **No AI recipe source.** The DSL is designed AI-friendly (serde derives, clamping compiler) but no LLM client, no JSON schema, no `RecipeChoice::AiSuggested` variant is added here. That's a future sub-project.
- **No new automation primitives.** Phases compile to the existing `AutomationTarget` set (`Fader`, `Tempo`, `EqLow`, `EqMid`, `EqHigh`, `Filter`, `EchoMix`, `LoopGate`). `EchoThrow` is two `EchoMix` Step automations under the hood; no new effect plumbing.
- **No phase-level scoring.** Recipes are still atomic from the picker's point of view. Scoring branches in `score_recipe` look at pair properties, not phase contents.
- **No re-tuning of existing scoring constants.** New recipes get new branches; existing branches are unchanged.

## Core types

```rust
// engine/src/recipe_script.rs

use serde::{Deserialize, Serialize};
use crate::automation::Curve;
use crate::recipe::DeckInitConfig;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransitionScript {
    pub length_bars: usize,
    pub phases: Vec<Phase>,
    pub outgoing_init: Option<DeckInitConfig>,
    pub incoming_init: Option<DeckInitConfig>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeckRole { Outgoing, Incoming }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EqBand { Low, Mid, High }

/// Fractional bars from window start. `bars: 4.5` = beat 18 of a 4/4 piece.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BarPos { pub bars: f64 }

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BarSpan { pub start: BarPos, pub end: BarPos }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Phase {
    Fader   { deck: DeckRole, from: f64, to: f64, span: BarSpan, curve: Curve },
    EqRamp  { deck: DeckRole, band: EqBand, from_db: f64, to_db: f64, span: BarSpan, curve: Curve },
    EqStep  { deck: DeckRole, band: EqBand, value_db: f64, at: BarPos },
    Filter  { deck: DeckRole, from: f64, to: f64, span: BarSpan, curve: Curve },
    EchoMix { deck: DeckRole, from: f64, to: f64, span: BarSpan, curve: Curve },
    /// Echo throw: Step EchoMix to 1.0 at `at`, then Step to 0.0 at `at + hold_bars`.
    EchoThrow { deck: DeckRole, at: BarPos, hold_bars: f64 },
    /// LoopGate on at `on_at`, off at `off_at`. Two Step automations.
    LoopGate { deck: DeckRole, on_at: BarPos, off_at: BarPos },
    /// Tempo Step at bar 0. Every recipe emits one for `Incoming`.
    TempoAlign { deck: DeckRole, ratio: f64 },
}
```

`Curve` and `DeckInitConfig` get `#[derive(Serialize, Deserialize)]` added so the script can round-trip.

## Compiler

```rust
pub fn compile(script: &TransitionScript, ctx: &TransitionContext) -> RecipeOutput;
```

**Frame math.** Computed once from `ctx.outgoing.sidecar.bpm` (defaulting to 120.0, matching current behavior) and `ctx.sample_rate`:

```rust
let outgoing_bpm   = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
let bar_seconds    = (60.0 / outgoing_bpm) * 4.0;
let frames_per_bar = bar_seconds * (ctx.sample_rate as f64);
let bar_to_frame   = |b: BarPos| {
    let clamped = b.bars.clamp(0.0, script.length_bars as f64);
    ctx.master_clock_frames + (clamped * frames_per_bar) as u64
};
let total_duration_frames =
    (script.length_bars as f64 * frames_per_bar) as u64;
```

**Phase → Automation mapping:**

| Phase | Emitted `Automation`(s) (on the right deck's vector) |
|---|---|
| `Fader { deck, from, to, span, curve }` | `[{ Fader, bar_to_frame(span.start), bar_to_frame(span.end), from, to, curve }]` |
| `EqRamp { deck, band, from_db, to_db, span, curve }` | `[{ EqLow\|EqMid\|EqHigh, bar_to_frame(span.start), bar_to_frame(span.end), from_db, to_db, curve }]` |
| `EqStep { deck, band, value_db, at }` | `[{ EqLow\|EqMid\|EqHigh, bar_to_frame(at), bar_to_frame(at)+1, value_db, value_db, Step }]` |
| `Filter { deck, from, to, span, curve }` | `[{ Filter, …, from, to, curve }]` |
| `EchoMix { deck, from, to, span, curve }` | `[{ EchoMix, …, from, to, curve }]` |
| `EchoThrow { deck, at, hold_bars }` | `[{ EchoMix, bar_to_frame(at), +1, 1.0, 1.0, Step }, { EchoMix, bar_to_frame(at+hold_bars), +1, 0.0, 0.0, Step }]` |
| `LoopGate { deck, on_at, off_at }` | `[{ LoopGate, bar_to_frame(on_at), +1, 1.0, 1.0, Step }, { LoopGate, bar_to_frame(off_at), +1, 0.0, 0.0, Step }]` |
| `TempoAlign { deck, ratio }` | `[{ Tempo, t0, t0+1, ratio, ratio, Step }]` |

**Validation (clamp + warn, never panic):**

- `BarPos.bars` outside `[0, length_bars]` → clamped to range. Warn-logged once per compile if any phase clamps.
- `BarSpan` with `end < start` → phase is dropped (no automation emitted). Warn-logged.
- `BarSpan` with `end == start` → emit a 1-frame Step automation (`t_end = t_start + 1`). This is the canonical "step at this BarPos" idiom for `Fader` / `EqRamp` / `Filter` / `EchoMix` phases. (`EqStep`, `EchoThrow`, `LoopGate`, `TempoAlign` already encode step semantics directly.)
- EQ dB values clamped to `[-40, 6]` to match the existing `apply_value` clamp in `automation.rs`. (Already clamped at apply-time too; compile-time clamp avoids logging surprises.)
- `Fader` / `EchoMix` `from`/`to` clamped to `[0, 1]`.
- `Filter` clamped to `[-1, +1]`.

`RecipeOutput.total_duration_frames` = the computed `total_duration_frames` above.

## Recipe trait change and call sites

```rust
pub trait Recipe {
    fn name(&self) -> &'static str;
    fn build(&self, ctx: &TransitionContext<'_>) -> TransitionScript;  // was RecipeOutput
}
```

A helper at the call seam:

```rust
// recipe.rs
pub fn build_and_compile(recipe: &dyn Recipe, ctx: &TransitionContext) -> RecipeOutput {
    let script = recipe.build(ctx);
    compile(&script, ctx)
}
```

`main.rs` and `scheduler.rs` call sites switch from `recipe.build(&ctx)` to `build_and_compile(recipe, &ctx)`. Everything downstream is byte-identical.

## Migration of the 7 existing recipes

Each existing recipe is rewritten to emit a `TransitionScript` that compiles to **the same `Automation`s it produces today** (up to the same `as u64` rounding). Concretely:

- **`EqualPowerCrossfade`** → `[TempoAlign(Incoming), Fader(Outgoing, 1→0, [0,L], EqualPowerOut), Fader(Incoming, 0→1, [0,L], EqualPowerIn)]`.
- **`HardCut`** → `[TempoAlign(Incoming), Fader(Outgoing, 1→0, span=bars(0,0), Step), Fader(Incoming, 0→1, span=bars(0,0), Step)]`. A `BarSpan` with `end == start` is the canonical signal to the compiler to emit a 1-frame Step automation (matching the existing HardCut's `t_end = t0 + 1`). `length_bars = 0` for HardCut, so `bar_to_frame(0) = t0`. This rule is documented in the compiler section.
- **`BassSwap`** → `EqualPowerCrossfade`'s phases, plus, conditionally:
  - `EqStep(Incoming, Low, -40dB, at bar 0)`
  - `EqRamp(Outgoing, Low, 0→-40dB, [0.35*L, 0.48*L], Linear)`
  - `EqRamp(Incoming, Low, -40→0dB, [0.52*L, 0.65*L], Linear)` for the standard window
  - For "both bass-heavy", the fractions become `[0.30, 0.48, 0.52, 0.70]` (preserving current behavior).
- **`FilterFade`** → equal-power phases + `EqStep` to set initial filter (modeled as a `Filter` phase with `from=to=1.0, span=[0,1-frame]`) + `Filter(Incoming, 1→0, [0,L], Linear)`. (Alternative: extend the DSL with a `FilterStep` like `EqStep` so init is symmetric. Decision: do NOT extend; the compiler can recognize a 1-frame `Filter` span as a step, no DSL bloat.)
- **`FilterKill`** → equal-power phases + `Filter(Outgoing, 0→1, [0, 0.6*L], Linear)`.
- **`EchoOut`** → equal-power phases + `EchoMix(Outgoing, 0→1, [0, L/2], Linear)` + `outgoing_init` carrying `echo_delay_seconds = 60/outgoing_bpm` (1 beat) and `echo_feedback = 0.5`.
- **`LoopOut`** → equal-power phases + `LoopGate(Outgoing, on_at: 0, off_at: L)` + `outgoing_init` with `loop_bars = 4`, `loop_bar_seconds = bar_seconds`.

**Faithfulness check.** The existing integration tests in `engine/tests/{eq,filter,effects,mixer_integration,scheduler,crossfade,phrase,recipe,transition,deck,automation}_test.rs` are the contract. If a test reports drift, the cause is investigated:
- If the drift is < 1 frame and due to `as u64` cast order, accept it and update the test's expected frame.
- If the drift is larger, the migration mapping above is wrong; fix the script, don't relax the test.

## Three new recipes

For all three, `L = script.length_bars`.

### `ThreeBandSwap`

Three-stage swap on integer bar positions (for L = 8/16/32, the listed BarPos values are all whole bars):

```rust
let phases = vec![
    TempoAlign { deck: Incoming, ratio },
    Fader { deck: Outgoing, from: 1.0, to: 0.0, span: bars(0.0, L), curve: EqualPowerOut },
    Fader { deck: Incoming, from: 0.0, to: 1.0, span: bars(0.0, L), curve: EqualPowerIn },

    // Highs first.
    EqStep { deck: Incoming, band: High, value_db: -40.0, at: bar(0.0) },
    EqRamp { deck: Incoming, band: High,
             from_db: -40.0, to_db: 0.0,
             span: bars(0.0, L / 8.0), curve: Linear },

    // Mids cross in the middle quarter.
    EqRamp { deck: Outgoing, band: Mid,
             from_db: 0.0, to_db: -40.0,
             span: bars(3.0 * L / 8.0, 5.0 * L / 8.0), curve: Linear },
    EqRamp { deck: Incoming, band: Mid,
             from_db: -40.0, to_db: 0.0,
             span: bars(3.0 * L / 8.0, 5.0 * L / 8.0), curve: Linear },

    // Bass swap on a single downbeat near the end.
    EqStep { deck: Incoming, band: Low, value_db: -40.0, at: bar(0.0) },
    EqStep { deck: Outgoing, band: Low, value_db: -40.0, at: bar(7.0 * L / 8.0) },
    EqStep { deck: Incoming, band: Low, value_db:   0.0, at: bar(7.0 * L / 8.0) },
];
```

For L=16 the bass swap lands on bar 14, mids cross over bars [6, 10], highs in over bars [0, 2]. For L=32: bar 28, [12, 20], [0, 4]. For L=8: bar 7, [3, 5], [0, 1]. All integer bars.

### `EchoThrow`

Equal-power crossfade with a one-shot echo throw on outgoing's exit:

```rust
let phases = vec![
    TempoAlign { deck: Incoming, ratio },
    Fader { deck: Outgoing, from: 1.0, to: 0.0, span: bars(0.0, L), curve: EqualPowerOut },
    Fader { deck: Incoming, from: 0.0, to: 1.0, span: bars(0.0, L), curve: EqualPowerIn },
    EchoThrow { deck: Outgoing, at: bar(L - 2.0), hold_bars: 1.0 },
];

let outgoing_init = Some(DeckInitConfig {
    echo_delay_seconds: Some(60.0 / outgoing_bpm),  // 1 beat
    echo_feedback: Some(0.75),
    loop_bars: None,
    loop_bar_seconds: None,
});
```

For L=8 the throw fires at bar 6; L=16 → bar 14; L=32 → bar 30. The throw holds for 1 bar then drops back to dry — by which time the equal-power outgoing fader is near zero.

### `HighsFirst`

Incoming intro with bass + mids killed at start, opened in stages, with bass swap synchronized to outgoing's bass kill:

```rust
let phases = vec![
    TempoAlign { deck: Incoming, ratio },
    Fader { deck: Outgoing, from: 1.0, to: 0.0, span: bars(0.0, L), curve: EqualPowerOut },
    Fader { deck: Incoming, from: 0.0, to: 1.0,
            span: bars(0.0, L / 8.0), curve: EqualPowerIn },  // fast fader rise

    EqStep { deck: Incoming, band: Low,  value_db: -40.0, at: bar(0.0) },
    EqStep { deck: Incoming, band: Mid,  value_db: -40.0, at: bar(0.0) },

    // Mids fade in over the first half.
    EqRamp { deck: Incoming, band: Mid, from_db: -40.0, to_db: 0.0,
             span: bars(L / 4.0, L / 2.0), curve: Linear },

    // Synchronized bass swap at 3L/4.
    EqStep { deck: Outgoing, band: Low, value_db: -40.0, at: bar(3.0 * L / 4.0) },
    EqStep { deck: Incoming, band: Low, value_db:   0.0, at: bar(3.0 * L / 4.0) },
];
```

For L=16: fader [0,2], mids [4,8], bass swap bar 12. For L=32: fader [0,4], mids [8,16], bar 24. For L=8: fader [0,1], mids [2,4], bar 6.

## Picker and scoring additions

`RecipeChoice` grows three variants (added at the end so existing ordinal-dependent code is undisturbed):

```rust
pub enum RecipeChoice {
    EqualPower,
    HardCut,
    BassSwap,
    FilterFade,
    FilterKill,
    EchoOut,
    LoopOut,
    ThreeBandSwap,  // NEW
    EchoThrow,      // NEW
    HighsFirst,     // NEW
}
```

`recipe_choice_id()` returns `8` for `ThreeBandSwap`, `9` for `EchoThrow`, `10` for `HighsFirst` (used in the deterministic-jitter mix in `pick_recipe`).

`pick_recipe()`'s `candidates` array grows from 6 to 9 (the HardCut short-circuit is unchanged).

New branches in `score_recipe`:

```rust
RecipeChoice::ThreeBandSwap => {
    let both_heavy = inputs.outgoing_bass > 0.5 && inputs.incoming_bass > 0.5;
    let any_heavy  = inputs.outgoing_bass > 0.5 || inputs.incoming_bass > 0.5;
    let long_enough = inputs.length_bars >= 16;
    if inputs.phrase_aligned && both_heavy && long_enough { 0.90 }
    else if inputs.phrase_aligned && both_heavy { 0.70 }
    else if inputs.phrase_aligned && any_heavy { 0.50 }
    else { 0.20 }
}

RecipeChoice::EchoThrow => {
    let smooth = inputs.energy_delta.abs() < 0.10 && inputs.length_bars >= 16;
    let mild   = inputs.energy_delta.abs() < 0.15 && inputs.length_bars >= 8;
    if smooth { 0.65 }
    else if mild { 0.55 }
    else { 0.20 }
}

RecipeChoice::HighsFirst => {
    let big_ramp = inputs.energy_delta > 0.20 && inputs.phrase_aligned && inputs.length_bars >= 16;
    let med_ramp = inputs.energy_delta > 0.10 && inputs.phrase_aligned;
    let any_ramp = inputs.energy_delta > 0.05;
    if big_ramp { 0.85 }
    else if med_ramp { 0.55 }
    else if any_ramp { 0.30 }
    else { 0.10 }
}
```

`recency_hash()` iterates over all 10 `RecipeChoice` variants (the existing list grows by 3).

`--force-recipe` accepts three new values:
- `three_band_swap` → `RecipeChoice::ThreeBandSwap`
- `echo_throw`      → `RecipeChoice::EchoThrow`
- `highs_first`     → `RecipeChoice::HighsFirst`

## Future hook for AI-generated scripts (out of scope, design constraint only)

The DSL types carry `#[derive(Serialize, Deserialize)]` so a future sub-project can:

1. Send the pair's sidecars to an LLM with a system prompt describing the `Phase` vocabulary.
2. Receive a JSON `TransitionScript`.
3. Deserialize and pass to `compile()`.

Two things in *this* spec make that future cheap and safe:

- **Compiler is clamping and panic-free.** An LLM emitting `BarPos { bars: 47.0 }` for a 16-bar window doesn't crash the engine — the phase clamps and is logged.
- **Phases are content-addressable** (no opaque IDs, no frame numbers in scripts). An LLM only needs to know the verb names + the per-deck control set; no internal state.

A future `RecipeChoice::AiSuggested(Box<TransitionScript>)` could route through the same `compile()` path. Not in this spec.

## Components and file layout

```
engine/src/
  recipe_script.rs       ← NEW: TransitionScript, Phase, BarPos, BarSpan,
                                DeckRole, EqBand, compile(), validation helpers
  recipe.rs              ← Recipe trait return type changed
                          ← existing 7 recipes rewritten to emit TransitionScript
                          ← three new recipes (ThreeBandSwap, EchoThrow, HighsFirst)
                          ← RecipeChoice gains 3 variants
                          ← score_recipe gains 3 branches
                          ← recency_hash iterates over all 10 variants
                          ← build_and_compile helper
  automation.rs          ← +Serde derives on Curve (one-line attribute)
  config.rs              ← --force-recipe accepts three new string values
  main.rs                ← call site uses build_and_compile
  scheduler.rs           ← call site uses build_and_compile (if applicable)
  lib.rs                 ← +pub mod recipe_script;

engine/tests/
  script_test.rs         ← NEW: phase-level assertions for new recipes
  compile_test.rs        ← NEW: compiler unit tests (frame math, clamping, EchoThrow expansion)
  recipe_test.rs         ← +score_recipe branches for 3 new recipes
                          ← +pick_recipe (scored) tests covering the new recipes
  {existing per-recipe tests} ← stay green after migration (frame drift < 1 frame allowed)
```

## Testing strategy

### New unit / integration tests

- **`compile_test.rs`**:
  - `BarPos { bars: 4.0 }` at 120 BPM, 44.1 kHz → frame offset `352800`. (Sanity.)
  - `EchoThrow { at: bar(14), hold_bars: 1 }` for a 16-bar script at 120 BPM expands to exactly two `Automation`s: EchoMix to 1.0 at frame `bar_to_frame(14)`, EchoMix to 0.0 at frame `bar_to_frame(15)`. Both are 1-frame Step automations.
  - `EqStep { Outgoing, Low, value_db: -55.0, at: bar(2) }` → emitted EqLow Automation has `from = to = -40.0` (clamped).
  - `BarSpan { start: 5, end: 3 }` → phase dropped, no automation emitted, log captured.
  - `BarPos { bars: 47.0 }` in an L=16 script → clamped to 16.0 in the emitted frame index.
  - Total duration in frames matches `length_bars * frames_per_bar` rounded.

- **`script_test.rs`** (phase-level for new recipes):
  - `ThreeBandSwap.build(ctx_16bar)` returns a script whose `phases` matches the expected shape (assert variant pattern + key fields for each — not a brittle full-equality check).
  - Same for `EchoThrow` and `HighsFirst`, including verifying `outgoing_init` is set correctly for `EchoThrow`.

- **`recipe_test.rs` additions**:
  - `score_recipe(ThreeBandSwap, both_heavy + phrase_aligned + L=16)` ≥ 0.90.
  - `score_recipe(ThreeBandSwap, both_heavy + phrase_aligned + L=8)` ≥ 0.70.
  - `score_recipe(EchoThrow, energy_delta=0.05, L=16)` ≥ 0.65.
  - `score_recipe(HighsFirst, energy_delta=0.3 + phrase_aligned + L=32)` ≥ 0.85.
  - `pick_recipe` with a pair that strongly matches `ThreeBandSwap` and an empty recency returns `ThreeBandSwap`.
  - `pick_recipe` with `ThreeBandSwap` in recency falls through to a lower-scoring alternative.

### Existing tests

The 7 existing per-recipe integration tests must pass after migration. If any test reports a drift of more than 1 frame on any automation's `t_start_frames` or `t_end_frames`, the migration mapping is wrong and gets fixed (not the test). A 1-frame drift due to `as u64` rounding sliding from one cast site to another is acceptable: update the test's expected frame to match.

## Risks

- **HardCut's 1-frame Step span is awkward in bar-time.** Resolved by the `BarSpan::end == BarSpan::start` → 1-frame Step rule (see Compiler section). `HardCut`'s migration uses `bars(0, 0)` for both faders and the compiler does the right thing.
- **Migration drift.** Some existing recipes will round differently after migration if we're not careful (the existing code does `(0.35 * length_frames) as u64`; the new code does `(0.35 * length_bars * frames_per_bar) as u64`, mathematically identical but the cast happens once on a larger value). Most likely no drift; if there is any, it's ≤ 1 frame and we update the test.
- **Curve serde.** `Curve` is currently a plain enum. Adding `#[derive(Serialize, Deserialize)]` is one line but requires `serde` to be in the dependency tree (it already is via the sidecar/library code).
- **DSL surface creep.** Resist adding more phase variants in this spec. If a recipe needs something the DSL can't express, extend the DSL deliberately, not opportunistically.
- **Test verbosity.** Phase-level tests on the rewritten 7 existing recipes are NOT in scope here — the existing automation-level integration tests are the contract. Adding phase-level tests for the 7 existing recipes would be cleanup work for a future sub-project.

## Open questions

- For `EchoThrow`, is `echo_feedback = 0.75` audibly correct, or should it be higher (closer to runaway) for a more dramatic tail? Default to 0.75; revisit by ear.
- `ThreeBandSwap`'s scoring penalizes short windows (L < 16). Should L=8 ever pick it? Current scoring says yes but with a lower score (0.70). Acceptable.
- Should `HighsFirst` be considered when `incoming.energy < outgoing.energy` (a "wind-down" pair)? The current scoring requires `energy_delta > 0.05`, so no. If listening reveals it sounds good in reverse, swap the sign in a follow-up.

## Roadmap (after this sub-project)

With phase composition merged, the natural follow-ups are:

1. **Re-align existing recipes to bar boundaries.** Sweep `BassSwap`'s fractions, the `FilterKill` 0.6, the `EchoOut` 0.5 to clean bar/beat positions. Ear-test driven. Now structurally trivial.
2. **AI recipe source.** A new `RecipeChoice::AiSuggested(Box<TransitionScript>)` variant. The picker calls an LLM with the pair's sidecars; the response is a JSON script that deserializes directly. The compile path is already AI-safe.
3. **Phase-level scoring.** Look inside the script when scoring (e.g., "does this phase set up too much going on in bar 8?"). Probably overkill but possible.
4. **Re-tune existing scoring constants.** Listen and iterate.
