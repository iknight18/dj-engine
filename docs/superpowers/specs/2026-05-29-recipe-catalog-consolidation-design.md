# Recipe Catalog Consolidation — Design

**Date:** 2026-05-29
**Status:** Draft, pending user review
**Spec depends on:** `docs/recipe-feedback-2026-05-29.md` (the listening session that informs every decision here).

## Goal

Reduce the 16-recipe catalog to 7 based on extensive listening feedback. Drop 8 recipes that fail to land musically for this listener's use case (focused indoor listening, no dance floor). Merge two underperforming bass-swap variants into one new stems-aware `RhythmSwap` that swaps drums + bass together (not bass alone). Rebalance picker scoring so `filter-fade` becomes the default crossfade instead of `equal-power`.

## Motivation

The 16-recipe catalog was built up speculatively. The listening session validated three clear patterns:

- **Continuous filter sweeps work** — `filter-fade` consistently lands cleanly, `filter-kill` works for energy-down.
- **Step-based EQ reveals do NOT work** — 4 of 4 recipes built around discrete Step EQ-band reveals (`three-band-swap`, `highs-first`, `breakdown-bridge`, `breakdown-into-drop`) were rejected. The step-on-downbeat refactor that produced them was a wrong turn.
- **Layering vocals or full mixes on top of outgoing fundamentally clashes** — `vocal-overlay`, `double-drop`, the bass-only stem swap all produce the "two melodies is ambiguous" effect. The fix is to never have two melody-bearing layers playing simultaneously.

Separately, the echo subsystem failed audibility on both echo-out (subtle) and echo-throw (claimed by user to sound like "fast crossfade"). Until that subsystem is investigated, the two echo recipes are dead weight.

Carrying 9 underperforming or broken recipes alongside the working ones complicates the picker scoring, adds maintenance cost, and dilutes the design. Consolidating to 7 recipes (6 keepers + 1 new RhythmSwap) leaves a clean, intentional catalog.

## Scope

### Delivers

**Catalog change: 16 recipes → 7 recipes.** 10 struct deletions, 1 struct addition.

**Deletions (10 structs + Recipe impls):**
- **8 dropped per listening feedback:** `EchoOut`, `EchoThrow`, `ThreeBandSwap`, `HighsFirst`, `BreakdownBridge`, `DoubleDrop`, `VocalOverlay`, `BreakdownIntoDrop`.
- **2 merged into RhythmSwap:** `BassSwap`, `BassSwapStemAware`.

**Addition (1 struct):**
- `RhythmSwap` — stems-aware, swaps drums + bass together at a single bar (default L/2). Replaces the two bass-swap variants.

**Kept (6 unchanged + 1 new = 7 final):**
- `EqualPower`, `HardCut`, `FilterFade`, `FilterKill`, `LoopOut`, `Drop` (unchanged code; scoring changes for `FilterFade` + `EqualPower` below).
- `RhythmSwap` (new).

**Picker scoring rebalanced:**
- `FilterFade` becomes the default crossfade with floor score 0.55 (currently 0.40 for moderate energy ramp, 0.0 below). Always selectable.
- `EqualPower` demoted from score 0.15 to score 0.10 (still as final fallback).
- `RhythmSwap` scoring uses both `outgoing_bass` + `incoming_bass` AND new `outgoing_drum_density_mean` + `incoming_drum_density_mean` fields.

**Schema additions to `PairScoreInputs`:**
- `outgoing_drum_density_mean: f64`
- `incoming_drum_density_mean: f64`
(Computed as mean of `sidecar.stems.drum_density`; 0.0 when stems absent.)

### Out of scope

- **"Punctuated hard-cut" variant** with outgoing FX before the cut — separate follow-up spec.
- **`loop-out` enhancement** (replace equal-power fade-in with filter-fade or rhythm-stem leak-in) — separate follow-up spec.
- **Echo subsystem investigation** — deferred. With both echo recipes dropped, this no longer blocks anything.
- **Deleting `EchoLine` / `EchoMix` automation target from infrastructure** — they remain in the codebase as currently-unused code in `engine/src/effects.rs` and `engine/src/automation.rs`, with a comment indicating no recipe currently consumes them. Future echo recipes can re-enable. Marking dead code is cheaper than deletion + re-creation later.
- **Deleting `LoopGate` automation target** — still used by `LoopOut`.
- **Removing `DeckInitConfig::echo_delay_seconds` / `echo_feedback`** — kept as Option fields for future use. The current recipes that use them (`EchoOut`, `EchoThrow`) are deleted; `DeckInitConfig` stays.
- **Removing the analyzer's `drum_density` / `bass_density` / `vocal_density` arrays from sidecars** — these are consumed by `RhythmSwap` (drum) and remain useful regardless of which recipes use them.

## Architecture

### Final catalog (7 recipes)

| Choice | Stems? | Picker role | Approx. score range |
|---|---|---|---|
| `FilterFade` | No | **Default crossfade** | 0.55–0.85 |
| `FilterKill` | No | Energy-down | 0.0–0.70 |
| `EqualPower` | No | Final fallback | 0.10 (constant) |
| `HardCut` | No | Genre mismatch / length=0 | 0.0 / 1.0 |
| `LoopOut` | No | Unstructured outro | 0.30 (constant for now) |
| `Drop` | No | Energy-up + phrase-aligned | 0.10–0.75 |
| `RhythmSwap` (NEW) | Yes | Both tracks bass+drums heavy + phrase-aligned + length ≥ 8 | 0.05–0.92 |

### RhythmSwap design

**Recipe name:** `rhythm_swap` (CLI flag `rhythm-swap`).

**Length:** Uses `ctx.plan.length_bars` (no fixed length); only selected when `length_bars >= 8`.

**Concept:** Two-phase gesture. First half is a SUBTLE rhythm reveal — incoming's drums + bass gradually fade in underneath outgoing's continuing melody. Second half begins with a FAST melody handoff at the phrase boundary (bar L/2). At all times only ONE melody is audible — outgoing's during the reveal, incoming's after the swap. The rejected layering-clash failure mode is structurally impossible.

**Phase composition** (where `L = ctx.plan.length_bars`, `swap_bar = L/2.0`):

```rust
phases: vec![
    Phase::TempoAlign { deck: Incoming, ratio },

    // Incoming mixed fader to 1 at bar 0. cue_stems leaves fader=0; without
    // this Step the deck stays silent.
    Phase::Fader { deck: Incoming, from: 1.0, to: 1.0, span: bars(0.0, 0.0), curve: Step },

    // Outgoing mixed fader gracefully fades out over the second half.
    // Strictly cleanup — all outgoing stems are at 0 after swap_bar, so the
    // fader value is irrelevant audibly, but emitting it makes the log /
    // visualisation reflect reality.
    Phase::Fader { deck: Outgoing, from: 1.0, to: 0.0, span: bars(swap_bar, L), curve: EqualPowerOut },

    // ====== Subtle rhythm reveal — bars [0, swap_bar] ======
    // Outgoing's rhythm gradually fades out; incoming's rhythm gradually
    // fades in. The two crossfade so neither drum kit doubles the other.
    Phase::StemFader { deck: Outgoing, stem: Drums, from: 1.0, to: 0.0, span: bars(0.0, swap_bar), curve: Linear },
    Phase::StemFader { deck: Outgoing, stem: Bass,  from: 1.0, to: 0.0, span: bars(0.0, swap_bar), curve: Linear },
    Phase::StemFader { deck: Incoming, stem: Drums, from: 0.0, to: 1.0, span: bars(0.0, swap_bar), curve: Linear },
    Phase::StemFader { deck: Incoming, stem: Bass,  from: 0.0, to: 1.0, span: bars(0.0, swap_bar), curve: Linear },

    // Incoming melody held off during the reveal. Initial Step→0 at bar 0
    // because cue_stems sets all stems = 1.0; the Step explicitly mutes
    // vocals + other for the layering window.
    Phase::StemFader { deck: Incoming, stem: Vocals, from: 0.0, to: 0.0, span: bars(0.0, 0.0), curve: Step },
    Phase::StemFader { deck: Incoming, stem: Other,  from: 0.0, to: 0.0, span: bars(0.0, 0.0), curve: Step },

    // ====== Fast melody handoff — bar L/2 (phrase boundary) ======
    // Outgoing's vocals + other Step to 0; incoming's vocals + other Step
    // to 1. Both Steps fire on the same downbeat. The "fast" swap.
    Phase::StemFader { deck: Outgoing, stem: Vocals, from: 0.0, to: 0.0, span: bars(swap_bar, swap_bar), curve: Step },
    Phase::StemFader { deck: Outgoing, stem: Other,  from: 0.0, to: 0.0, span: bars(swap_bar, swap_bar), curve: Step },
    Phase::StemFader { deck: Incoming, stem: Vocals, from: 1.0, to: 1.0, span: bars(swap_bar, swap_bar), curve: Step },
    Phase::StemFader { deck: Incoming, stem: Other,  from: 1.0, to: 1.0, span: bars(swap_bar, swap_bar), curve: Step },
],
```

**Stem-fader state by phase** (illustrative for L=16, swap_bar=8):

| Time | OUT.drums | OUT.bass | OUT.vocals | OUT.other | IN.drums | IN.bass | IN.vocals | IN.other |
|---|---|---|---|---|---|---|---|---|
| pre-cue | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| post-cue (incoming just cued) | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| bar 0 (Steps + ramps begin) | 1 (ramp→) | 1 (ramp→) | 1 | 1 | 0 (ramp→) | 0 (ramp→) | 0 | 0 |
| bar 4 (mid-reveal) | 0.5 | 0.5 | 1 | 1 | 0.5 | 0.5 | 0 | 0 |
| bar 8 (swap fires) | 0 | 0 | 0 | 0 | 1 | 1 | 1 | 1 |
| bars 8–16 | 0 | 0 | 0 | 0 | 1 | 1 | 1 | 1 |

What the listener hears:
- **bar 0:** outgoing's full mix, incoming's rhythm just starting to whisper in.
- **bars 0–4:** outgoing still dominates, incoming's rhythm building underneath.
- **bars 4–8:** outgoing's rhythm fading out, incoming's rhythm taking over — outgoing's melody stays on top. **One melody (outgoing's) over a transitioning rhythm bed.**
- **bar 8 (the handoff):** vocals/melody snap from outgoing to incoming on the phrase downbeat. Rhythm section already established as incoming's by this point.
- **bars 8–16:** incoming as the new full track. Outgoing's mixed fader fades gracefully to 0.

**Why L/2 as the swap bar:** the engine's planner already constructs `length_bars` to be 8/16/32 so L/2 is always a phrase boundary in 4/4 time. A future variant could scan `incoming.drum_density` for the moment incoming's rhythm becomes most energetic and snap the swap bar to that, but L/2 is a safe default that matches the spec's "phrase boundary" intent.

**Why Linear rather than EqualPower curves on the rhythm reveal:** the rhythm reveal is a CROSSFADE of stems (outgoing-drums 1→0 paired with incoming-drums 0→1). For equal-power preservation we'd want EqualPowerOut/EqualPowerIn pair, BUT the rhythm reveal is meant to be perceptually a "gradual transfer" rather than constant power. Linear gives a smoother subjective build than EqualPower-pair would (the EP pair has a faster mid-section because of the cos/sin curve). If listening reveals a power dip at bar L/4, switch to EqualPower pair.

**Why the outgoing mixed fader fades only over the second half [L/2, L]:** during the rhythm reveal, outgoing's stems are still partially audible — we want them at their declared stem-fader values, not multiplied by a fader that's already dropping. After the swap, outgoing's stems are all 0 so the fader value doesn't affect audio; but emitting the fader fade keeps the audio engine state coherent and the logs honest about which deck is contributing.

### Scoring

The strong tier requires not just that both tracks are drum-heavy, but that their drum densities are **similar**. Mismatched densities (e.g., 0.55 vs 0.95) cause the rhythm swap to feel like an unintended energy jump — the listener perceives the new rhythm as much busier (or sparser) than the old one. For the swap to feel like a "transfer" rather than a "drop" or "lift," the rhythm densities must be close.

```rust
RecipeChoice::RhythmSwap => {
    let drum_density_delta = (inputs.outgoing_drum_density_mean
        - inputs.incoming_drum_density_mean).abs();
    let drums_heavy = inputs.outgoing_drum_density_mean > 0.5
        && inputs.incoming_drum_density_mean > 0.5;
    let drums_similar = drum_density_delta < 0.15;
    let bass_heavy = inputs.outgoing_bass > 0.5 && inputs.incoming_bass > 0.5;
    let energy_moderate = inputs.energy_delta.abs() < 0.20;
    let layering_long_enough = inputs.length_bars >= 8;
    let strong = inputs.stems_available
        && inputs.phrase_aligned
        && layering_long_enough
        && drums_heavy
        && drums_similar
        && energy_moderate;
    if strong && bass_heavy {
        0.92
    } else if strong {
        0.75
    } else if inputs.stems_available && layering_long_enough && drums_heavy {
        // Drums are heavy on both decks but not similar enough for the strong
        // tier — recipe still fires sometimes, but with reduced confidence.
        0.40
    } else if inputs.stems_available && layering_long_enough {
        0.20
    } else {
        0.05
    }
}
```

**Thresholds chosen:**
- `drum_density_delta < 0.15` — 15% of the [0,1] range. Tracks with densities (0.7, 0.85) qualify; (0.5, 0.85) don't.
- `drums_heavy > 0.5` — both tracks must have meaningful drum content. Sparser-drum tracks fall to the lower tier.

**Why not require bass_similar too:** the bass stem's "density" character is more about presence/groove than about volume — two tracks both having a steady bassline at very different energies still feel compatible because the listener locks onto the rhythm pattern, not the bass volume. Drums dominate the perceived energy of a rhythm section, so drum density is the right gate. Bass density can become a v2 refinement.

**Caveat about mean vs windowed density:** `outgoing_drum_density_mean` and `incoming_drum_density_mean` are means over the whole track. For tracks with sparse intros/outros and busy bodies, this is a reasonable proxy because the bars actually mixed (outgoing's outro region, incoming's intro region) are usually closer to the track's body than to its extremes. If listening reveals mismatches anyway (e.g., a track has a sparse outro but high overall mean), a v2 can add `drum_density_in_window(start_seconds, end_seconds)` helper on `Sidecar`, matching the pattern of `local_bpm_in_window`.

### FilterFade promotion

Current:
```rust
RecipeChoice::FilterFade => {
    if inputs.energy_delta > 0.20      { 0.75 }
    else if inputs.energy_delta > 0.05 { 0.40 }
    else                               { 0.0 }
}
```

New:
```rust
RecipeChoice::FilterFade => {
    // Always-selectable default crossfade. Higher score on energy-up,
    // but never falls to 0 — beats equal-power's 0.10 floor.
    if inputs.energy_delta > 0.20      { 0.85 }
    else if inputs.energy_delta > 0.05 { 0.70 }
    else if inputs.energy_delta > -0.05 { 0.55 }
    else                               { 0.30 }
}
```

### EqualPower demotion

```rust
RecipeChoice::EqualPower => 0.10,  // was 0.15
```

This keeps it as the "guaranteed nonzero score" fallback when nothing else is appropriate, but it's now always beaten by `FilterFade` (whose minimum is 0.30) and `LoopOut` (0.30).

## Components Touched

### Engine source

- **`engine/src/recipe.rs`:**
  - **Delete struct + Recipe impl for:** `BassSwap`, `EchoOut`, `ThreeBandSwap`, `EchoThrow`, `HighsFirst`, `BreakdownBridge`, `DoubleDrop`, `VocalOverlay`, `BassSwapStemAware`, `BreakdownIntoDrop`. **9 deletions.**
  - **Add struct + impl for:** `RhythmSwap` (~50 lines).
  - **Update `RecipeChoice` enum:** remove the 9 variants above; add `RhythmSwap`. Keeping: `EqualPower`, `HardCut`, `FilterFade`, `FilterKill`, `LoopOut`, `Drop`, plus new `RhythmSwap`. **Total variants: 7.**
  - **Update `recipe_choice_id`:** remove deleted variants, assign id 7 to `RhythmSwap` (preserving stable IDs for kept recipes is nice but the function is internal, so renumbering is OK).
  - **Update `candidates` list** in `pick_recipe` and `pick_recipe_with_scores` to the 7 remaining variants.
  - **Update `recency_hash`** for-loop list to match.
  - **Update `score_recipe`:** delete branches for deleted variants, rewrite `FilterFade` + `EqualPower` per spec, add `RhythmSwap` branch.
  - **`PairScoreInputs`** gains two new fields: `outgoing_drum_density_mean`, `incoming_drum_density_mean`. `from_pair` computes them from `sidecar.stems.drum_density` mean (0.0 when stems absent).

- **`engine/src/recipe_recency.rs`:** if it enumerates RecipeChoice variants, update.

- **`engine/src/main.rs`:** the recipe dispatch match (RecipeChoice → Box\<dyn Recipe\>) needs the 9 arms removed and the `RhythmSwap` arm added. The `--force-recipe` clap-derived enum needs the same removals + addition (clap will auto-kebab-case `RhythmSwap` → `rhythm-swap`).

- **`engine/src/effects.rs`:** **No changes.** `EchoLine` and `LoopBuffer` remain. Add a top-of-file comment:
  ```rust
  // EchoLine is currently unused — both echo-based recipes were dropped in
  // the 2026-05-29 consolidation (audible-character failures). Kept for future
  // echo recipes once the subsystem is investigated. See
  // docs/recipe-feedback-2026-05-29.md.
  ```

- **`engine/src/automation.rs`:** no changes. `AutomationTarget::EchoMix` and `AutomationTarget::LoopGate` remain.

- **`engine/src/recipe_script.rs`:** no changes. `Phase::EchoMix`, `Phase::EchoThrow`, `Phase::LoopGate` variants remain (Phase variants are independent of which recipes use them).

### Engine tests

- **`engine/tests/recipe_test.rs`:**
  - Remove all score-tests for the 9 deleted variants.
  - Update `FilterFade` score tests for the new scoring tiers.
  - Update `EqualPower` score test for value 0.10.
  - Add new `RhythmSwap` score tests (strong, medium, floor cases).
  - Add `PairScoreInputs` tests for new `outgoing_drum_density_mean` / `incoming_drum_density_mean` fields.

- **`engine/tests/script_test.rs`:**
  - Remove all `build_script` tests for the 9 deleted variants.
  - Add `RhythmSwap` build_script test (verifies phase shape).

- **`engine/tests/compile_test.rs`:** no changes (tests the DSL, not the recipes).

- **`engine/tests/scheduler_test.rs`:** review for any references to deleted RecipeChoice variants; update fixtures if needed.

### CLI

- `--force-recipe` accepts the 7 new variant names: `equal-power`, `hard-cut`, `filter-fade`, `filter-kill`, `loop-out`, `drop`, `rhythm-swap`. The 9 deleted values are no longer accepted (clap will error on them — acceptable, the old recipe names are gone).

### Analyzer

- **No analyzer changes.** Sidecar schema is unchanged; the new `RhythmSwap` consumes existing `stems.drum_density` data.

## Error Handling

- **Stems missing on a sidecar but `RhythmSwap` somehow gets picked:** `requires_stems()` returns true, so the scheduler refuses (existing behavior, no change needed).
- **`drum_density` array missing on a sidecar:** PairScoreInputs defaults to 0.0. Recipe scoring then collapses to the 0.05 floor — picker won't choose RhythmSwap. Defensive.
- **`drum_density` array all zeros (no drum content detected):** same outcome — score floor, picker picks something else.

## Testing

### Unit tests

Per-recipe scoring + build_script tests as listed in "Engine tests" above.

### Compile + integration

- `cargo test --manifest-path engine/Cargo.toml` — all tests pass.
- `cargo build --manifest-path engine/Cargo.toml --release` — clean release build (note: requires stopping the running engine binary first, per the binary-lock memory).

### Live verification

1. Restart the engine with new sidecars and no `--force-recipe`. Let it run through several transitions.
2. Confirm `filter-fade` becomes the dominant recipe choice (should fire on most transitions).
3. Confirm `rhythm-swap` fires when conditions are right (both tracks bass + drums heavy, phrase-aligned, length ≥ 16).
4. Confirm `equal-power` only fires when literally nothing else scores well — should be rare.
5. Confirm none of the 8 dropped recipes appear in the logs.

### Picker logging

The existing `transition arming` log emits `scores=...` listing all candidates. After the consolidation, the score list goes from 16 entries to 7. Verify the log line is readable.

## Migration

- No sidecar re-analysis required.
- Engine binary needs rebuild + restart.
- No data migration; the recipe catalog change is purely engine-side code.

## Rollback

- Revert the consolidation commit; rebuild engine.
- The dropped recipes' code is gone — restoring would mean reverting to before this commit OR re-implementing from the spec history. For safety: tag the pre-consolidation commit (`pre-recipe-consolidation`) before merging this work.

## Open Items For Implementation

- Decide the exact name: `RhythmSwap`, `BassDrumsSwap`, or `StemRhythmSwap`. Going with `RhythmSwap` in the spec.
- Confirm the dispatch site in `engine/src/main.rs` (probably a match statement on `RecipeChoice`).
- During implementation, the picker tests will guide the FilterFade tier tuning — if `equal-power` somehow still wins in places `filter-fade` should win, adjust the FilterFade tiers up further.

## Follow-up specs

Captured for later, not in this scope:

1. **Punctuated hard-cut** — `HardCut` variant with outgoing FX (filter sweep / tape-stop) over the last 2 bars before the cut. For genre mismatch.
2. **LoopOut enhancement** — replace equal-power fade-in with either filter-fade-style incoming reveal OR rhythm-stem leak-in (incoming.drums + .bass enter under the loop, then full at the end).
3. **Echo subsystem investigation** — debug `EchoLine` audibility issue. Only needed if reintroducing echo recipes.
