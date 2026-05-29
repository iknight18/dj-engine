# Recipe Catalog Consolidation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce the 16-recipe catalog to 7 by dropping 8 underperforming recipes (per listening feedback), merging 2 bass-swap variants into a new stems-aware `RhythmSwap` (gradual rhythm reveal + fast melody swap at phrase boundary), and rebalancing the picker so `FilterFade` is the default crossfade.

**Architecture:** Per-task atomic changes — each RecipeChoice variant addition or removal touches the enum, the candidates list, recipe_choice_id, recency_hash, score_recipe, the dispatch site in main.rs, the clap ForceRecipe enum, and the struct/impl together. Adding RhythmSwap goes first so the new recipe coexists with the soon-to-be-deleted ones (validates incrementally). Then deletions happen in two batches: 8 dropped recipes, then 2 merged bass-swaps.

**Tech Stack:** Rust 2021, serde, tracing, clap (for --force-recipe). Tests via `cargo test`.

**Spec:** `docs/superpowers/specs/2026-05-29-recipe-catalog-consolidation-design.md`

---

## Task 1: Add `drum_density_mean` fields to `PairScoreInputs`

**Files:**
- Modify: `engine/src/recipe.rs` (PairScoreInputs struct + from_pair impl)
- Modify: `engine/tests/recipe_test.rs` (tests)

- [ ] **Step 1: Write the failing test**

Append to `engine/tests/recipe_test.rs`:
```rust
#[test]
fn pair_score_inputs_includes_drum_density_means() {
    use dj_engine::recipe::PairScoreInputs;
    use dj_engine::sidecar::Stems;

    let mut outgoing = test_loaded_track();
    let mut incoming = test_loaded_track();

    outgoing.sidecar.stems = Some(Stems {
        drum_density: vec![0.8, 0.9, 0.7, 0.85],
        bass_density: vec![],
        vocal_density: vec![],
        vocal_segments: vec![],
        energy_per_stem: Default::default(),
        demucs_version: None,
        model: None,
    });
    incoming.sidecar.stems = Some(Stems {
        drum_density: vec![0.2, 0.3, 0.1, 0.2],
        bass_density: vec![],
        vocal_density: vec![],
        vocal_segments: vec![],
        energy_per_stem: Default::default(),
        demucs_version: None,
        model: None,
    });

    let plan = test_plan_16_bars();
    let inputs = PairScoreInputs::from_pair(&plan, &outgoing, &incoming);
    assert!((inputs.outgoing_drum_density_mean - 0.8125).abs() < 1e-9);
    assert!((inputs.incoming_drum_density_mean - 0.2).abs() < 1e-9);
}

#[test]
fn pair_score_inputs_drum_density_defaults_to_zero_when_stems_absent() {
    use dj_engine::recipe::PairScoreInputs;

    let outgoing = test_loaded_track();
    let incoming = test_loaded_track();
    let plan = test_plan_16_bars();
    let inputs = PairScoreInputs::from_pair(&plan, &outgoing, &incoming);
    assert_eq!(inputs.outgoing_drum_density_mean, 0.0);
    assert_eq!(inputs.incoming_drum_density_mean, 0.0);
}
```

NOTE: `test_loaded_track()` and `test_plan_16_bars()` are existing helpers in `recipe_test.rs`. If they don't exist with those exact names, find equivalents (copy a fixture pattern from existing tests).

- [ ] **Step 2: Run to see fail**

```powershell
cargo test --manifest-path engine/Cargo.toml --test recipe_test pair_score_inputs_drum 2>&1 | Select-Object -First 30
```
Expected: compile errors — `PairScoreInputs` has no `outgoing_drum_density_mean` / `incoming_drum_density_mean` fields.

- [ ] **Step 3: Extend the struct and `from_pair`**

In `engine/src/recipe.rs`, find the existing `PairScoreInputs` struct. Add two fields at the end:
```rust
    /// Mean of the per-bar drum_density array for outgoing. 0.0 when stems absent.
    pub outgoing_drum_density_mean: f64,
    /// Mean of the per-bar drum_density array for incoming. 0.0 when stems absent.
    pub incoming_drum_density_mean: f64,
```

In `PairScoreInputs::from_pair`, after `stems_available`, compute:
```rust
        let outgoing_drum_density_mean = outgoing
            .sidecar
            .stems
            .as_ref()
            .map(|s| {
                if s.drum_density.is_empty() {
                    0.0
                } else {
                    s.drum_density.iter().sum::<f64>() / s.drum_density.len() as f64
                }
            })
            .unwrap_or(0.0);
        let incoming_drum_density_mean = incoming
            .sidecar
            .stems
            .as_ref()
            .map(|s| {
                if s.drum_density.is_empty() {
                    0.0
                } else {
                    s.drum_density.iter().sum::<f64>() / s.drum_density.len() as f64
                }
            })
            .unwrap_or(0.0);
```

Add both fields to the returned `Self { ... }` literal.

- [ ] **Step 4: Verify tests pass**

```powershell
cargo test --manifest-path engine/Cargo.toml --test recipe_test pair_score_inputs_drum
```
Expected: both new tests PASS.

- [ ] **Step 5: Full suite — confirm no regressions**

```powershell
cargo test --manifest-path engine/Cargo.toml 2>&1 | Select-Object -Last 20
```

- [ ] **Step 6: Commit**

```powershell
git add engine/src/recipe.rs engine/tests/recipe_test.rs
git commit -m "feat(engine): PairScoreInputs adds drum_density_mean for outgoing + incoming"
```

---

## Task 2: Add `RhythmSwap` recipe + register in picker + score it

**Files:**
- Modify: `engine/src/recipe.rs` (RhythmSwap struct + impl, RecipeChoice variant, candidates, recipe_choice_id, recency_hash, score_recipe)
- Modify: `engine/src/main.rs` (dispatch site + clap ForceRecipe enum)
- Modify: `engine/tests/script_test.rs` (build_script test)
- Modify: `engine/tests/recipe_test.rs` (scoring tests + variant inclusion test)

This is the biggest single task — adding a new recipe end-to-end. Done atomically because the RecipeChoice enum + score_recipe match must be exhaustive at all commits.

- [ ] **Step 1: Write failing build_script test**

Append to `engine/tests/script_test.rs`:
```rust
#[test]
fn rhythm_swap_emits_gradual_reveal_and_fast_handoff_phases() {
    use dj_engine::deck::StemKind;
    use dj_engine::recipe::{Recipe, RhythmSwap};
    use dj_engine::recipe_script::{DeckRole, Phase};

    let outgoing = test_loaded_track();
    let incoming = test_loaded_track();
    let plan = test_plan_16_bars();  // length_bars=16, so swap_bar=8
    let ctx = test_ctx(&outgoing, &incoming, &plan, 0);

    let script = RhythmSwap.build_script(&ctx);

    // Outgoing drums + bass Linear ramp 1→0 over [0, 8].
    let out_drums_ramp = script.phases.iter().any(|p| matches!(
        p,
        Phase::StemFader {
            deck: DeckRole::Outgoing,
            stem: StemKind::Drums,
            from,
            to,
            ..
        } if (from - 1.0).abs() < 1e-9 && (to - 0.0).abs() < 1e-9
    ));
    assert!(out_drums_ramp, "outgoing drums Linear ramp missing");

    // Incoming drums + bass Linear ramp 0→1 over [0, 8].
    let in_drums_ramp = script.phases.iter().any(|p| matches!(
        p,
        Phase::StemFader {
            deck: DeckRole::Incoming,
            stem: StemKind::Drums,
            from,
            to,
            ..
        } if (from - 0.0).abs() < 1e-9 && (to - 1.0).abs() < 1e-9
    ));
    assert!(in_drums_ramp, "incoming drums Linear ramp missing");

    // Incoming vocals + other muted at bar 0.
    let in_vocals_mute = script.phases.iter().any(|p| matches!(
        p,
        Phase::StemFader {
            deck: DeckRole::Incoming,
            stem: StemKind::Vocals,
            from,
            to,
            span,
            ..
        } if (from - 0.0).abs() < 1e-9 && (to - 0.0).abs() < 1e-9
              && (span.start.bars - 0.0).abs() < 1e-9
    ));
    assert!(in_vocals_mute, "incoming vocals mute Step at bar 0 missing");

    // Vocal handoff at bar 8 (swap_bar). Verify all four vocal/other Steps land at bar 8.
    let outgoing_vocals_off_at_8 = script.phases.iter().any(|p| matches!(
        p,
        Phase::StemFader {
            deck: DeckRole::Outgoing,
            stem: StemKind::Vocals,
            from,
            to,
            span,
            ..
        } if (from - 0.0).abs() < 1e-9 && (to - 0.0).abs() < 1e-9
              && (span.start.bars - 8.0).abs() < 1e-9
    ));
    assert!(outgoing_vocals_off_at_8, "outgoing vocals OFF Step at bar 8 missing");

    let incoming_vocals_on_at_8 = script.phases.iter().any(|p| matches!(
        p,
        Phase::StemFader {
            deck: DeckRole::Incoming,
            stem: StemKind::Vocals,
            from,
            to,
            span,
            ..
        } if (from - 1.0).abs() < 1e-9 && (to - 1.0).abs() < 1e-9
              && (span.start.bars - 8.0).abs() < 1e-9
    ));
    assert!(incoming_vocals_on_at_8, "incoming vocals ON Step at bar 8 missing");
}

#[test]
fn rhythm_swap_requires_stems_is_true() {
    use dj_engine::recipe::{Recipe, RhythmSwap};
    assert!(RhythmSwap.requires_stems());
}
```

- [ ] **Step 2: Write failing scoring tests**

Append to `engine/tests/recipe_test.rs`:
```rust
#[test]
fn score_rhythm_swap_strong_drums_heavy_similar_and_aligned() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};

    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.05,
        outgoing_bass: 0.7,
        incoming_bass: 0.7,
        phrase_aligned: true,
        stems_available: true,
        outgoing_drum_density_mean: 0.75,
        incoming_drum_density_mean: 0.80,
    };
    let s = score_recipe(RecipeChoice::RhythmSwap, &inputs);
    assert!((s - 0.92).abs() < 1e-9, "expected 0.92, got {s}");
}

#[test]
fn score_rhythm_swap_drums_similar_but_bass_light_returns_0_75() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};

    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.05,
        outgoing_bass: 0.3,  // not heavy
        incoming_bass: 0.3,
        phrase_aligned: true,
        stems_available: true,
        outgoing_drum_density_mean: 0.75,
        incoming_drum_density_mean: 0.80,
    };
    let s = score_recipe(RecipeChoice::RhythmSwap, &inputs);
    assert!((s - 0.75).abs() < 1e-9, "expected 0.75, got {s}");
}

#[test]
fn score_rhythm_swap_drums_heavy_but_dissimilar_returns_0_40() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};

    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.05,
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
        stems_available: true,
        outgoing_drum_density_mean: 0.55,
        incoming_drum_density_mean: 0.95, // delta = 0.40, > 0.15 threshold
    };
    let s = score_recipe(RecipeChoice::RhythmSwap, &inputs);
    assert!((s - 0.40).abs() < 1e-9, "expected 0.40, got {s}");
}

#[test]
fn score_rhythm_swap_drums_light_returns_0_20() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};

    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.05,
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
        stems_available: true,
        outgoing_drum_density_mean: 0.30,
        incoming_drum_density_mean: 0.30,
    };
    let s = score_recipe(RecipeChoice::RhythmSwap, &inputs);
    assert!((s - 0.20).abs() < 1e-9, "expected 0.20, got {s}");
}

#[test]
fn score_rhythm_swap_short_length_returns_floor() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};

    let inputs = PairScoreInputs {
        length_bars: 4,
        use_eq_swap: true,
        energy_delta: 0.05,
        outgoing_bass: 0.7,
        incoming_bass: 0.7,
        phrase_aligned: true,
        stems_available: true,
        outgoing_drum_density_mean: 0.75,
        incoming_drum_density_mean: 0.80,
    };
    let s = score_recipe(RecipeChoice::RhythmSwap, &inputs);
    assert!((s - 0.05).abs() < 1e-9, "expected 0.05, got {s}");
}

#[test]
fn score_rhythm_swap_no_stems_returns_floor() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};

    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.05,
        outgoing_bass: 0.7,
        incoming_bass: 0.7,
        phrase_aligned: true,
        stems_available: false,
        outgoing_drum_density_mean: 0.75,
        incoming_drum_density_mean: 0.80,
    };
    let s = score_recipe(RecipeChoice::RhythmSwap, &inputs);
    assert!((s - 0.05).abs() < 1e-9, "expected 0.05, got {s}");
}
```

- [ ] **Step 3: Run to see fail**

```powershell
cargo test --manifest-path engine/Cargo.toml --test script_test rhythm_swap 2>&1 | Select-Object -First 20
cargo test --manifest-path engine/Cargo.toml --test recipe_test score_rhythm_swap 2>&1 | Select-Object -First 20
```
Expected: `RhythmSwap` not defined; `RecipeChoice::RhythmSwap` variant missing.

- [ ] **Step 4: Add the `RhythmSwap` struct + impl**

Append to `engine/src/recipe.rs` (after the last existing recipe struct, currently `BreakdownIntoDrop`):

```rust
/// `RhythmSwap`: stems-aware transition. Two-phase gesture:
/// 1. Bars [0, L/2]: subtle rhythm reveal. Outgoing's drums + bass fade out;
///    incoming's drums + bass fade in simultaneously. Outgoing's vocals + other
///    play on top throughout (incoming's vocals + other held muted).
/// 2. Bar L/2 (phrase boundary): fast melody handoff. Outgoing's vocals + other
///    Step → 0; incoming's vocals + other Step → 1. The rhythm section has
///    already become incoming's, so the melody handoff feels like an arrival.
///
/// Only ONE melody is audible at any time (outgoing's during the reveal,
/// incoming's after the swap). Avoids the layering-clash failure mode by
/// construction.
///
/// Replaces both `BassSwap` (EQ-based) and `BassSwapStemAware` (bass-only).
pub struct RhythmSwap;

impl Recipe for RhythmSwap {
    fn name(&self) -> &'static str {
        "rhythm_swap"
    }

    fn requires_stems(&self) -> bool {
        true
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        use crate::deck::StemKind;

        let ratio = ctx.ratio;
        let l = ctx.plan.length_bars as f64;
        let swap_bar = l / 2.0;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                // Incoming mixed fader to 1 at bar 0 — cue_stems leaves fader=0.
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 1.0,
                    to: 1.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                // Outgoing mixed fader fades over the second half (cleanup;
                // outgoing stems all = 0 after swap_bar anyway).
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(swap_bar, l),
                    curve: Curve::EqualPowerOut,
                },
                // Subtle rhythm reveal — bars [0, swap_bar].
                Phase::StemFader {
                    deck: DeckRole::Outgoing,
                    stem: StemKind::Drums,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, swap_bar),
                    curve: Curve::Linear,
                },
                Phase::StemFader {
                    deck: DeckRole::Outgoing,
                    stem: StemKind::Bass,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, swap_bar),
                    curve: Curve::Linear,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Drums,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, swap_bar),
                    curve: Curve::Linear,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Bass,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, swap_bar),
                    curve: Curve::Linear,
                },
                // Incoming melody held off during the reveal.
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Vocals,
                    from: 0.0,
                    to: 0.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Other,
                    from: 0.0,
                    to: 0.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                // Fast melody handoff at swap_bar (phrase boundary).
                Phase::StemFader {
                    deck: DeckRole::Outgoing,
                    stem: StemKind::Vocals,
                    from: 0.0,
                    to: 0.0,
                    span: bars(swap_bar, swap_bar),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Outgoing,
                    stem: StemKind::Other,
                    from: 0.0,
                    to: 0.0,
                    span: bars(swap_bar, swap_bar),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Vocals,
                    from: 1.0,
                    to: 1.0,
                    span: bars(swap_bar, swap_bar),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Other,
                    from: 1.0,
                    to: 1.0,
                    span: bars(swap_bar, swap_bar),
                    curve: Curve::Step,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}
```

- [ ] **Step 5: Register `RecipeChoice::RhythmSwap` variant**

In `engine/src/recipe.rs`:

(a) Add to `RecipeChoice` enum (after `BreakdownIntoDrop`):
```rust
    RhythmSwap,
```

(b) Add to `recipe_choice_id` function:
```rust
        RecipeChoice::RhythmSwap => 17,
```

(c) Add to BOTH `pick_recipe` AND `pick_recipe_with_scores` candidates arrays (append at end of the existing array literal):
```rust
        RecipeChoice::RhythmSwap,
```

(d) Add to `recency_hash` for-loop list (append at end):
```rust
        RecipeChoice::RhythmSwap,
```

(e) Add the score branch in `score_recipe` (at the end of the match, before the closing brace):
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
                0.40
            } else if inputs.stems_available && layering_long_enough {
                0.20
            } else {
                0.05
            }
        }
```

- [ ] **Step 6: Register in main.rs**

In `engine/src/main.rs`, find the dispatch site (look for `match.*RecipeChoice::` with multiple arms returning `Box<dyn Recipe>` or similar). Add the new arm:
```rust
        RecipeChoice::RhythmSwap => Box::new(RhythmSwap),
```

Also find the clap `ForceRecipe` enum (derive `clap::ValueEnum`) — search for `enum ForceRecipe` or `RecipeChoice` annotated with clap. Add the variant:
```rust
    RhythmSwap,
```
clap will auto-kebab-case to `rhythm-swap`.

- [ ] **Step 7: Verify tests**

```powershell
cargo test --manifest-path engine/Cargo.toml --test script_test rhythm_swap
cargo test --manifest-path engine/Cargo.toml --test recipe_test score_rhythm_swap
```
Expected: all 6+ new tests PASS.

- [ ] **Step 8: Full engine suite**

```powershell
cargo test --manifest-path engine/Cargo.toml 2>&1 | Select-Object -Last 30
```
Expected: clean build, all tests PASS.

- [ ] **Step 9: Commit**

```powershell
git add engine/src/recipe.rs engine/src/main.rs engine/tests/script_test.rs engine/tests/recipe_test.rs
git commit -m "feat(engine): RhythmSwap recipe — gradual rhythm reveal + fast melody swap"
```

---

## Task 3: Promote `FilterFade` scoring; demote `EqualPower`

**Files:**
- Modify: `engine/src/recipe.rs` (score_recipe match arms for FilterFade and EqualPower)
- Modify: `engine/tests/recipe_test.rs` (existing FilterFade + EqualPower tests need updated expected values)

- [ ] **Step 1: Update the scoring**

In `engine/src/recipe.rs::score_recipe`, find the existing `FilterFade` arm:
```rust
        RecipeChoice::FilterFade => {
            if inputs.energy_delta > 0.20      { 0.75 }
            else if inputs.energy_delta > 0.05 { 0.40 }
            else                               { 0.0 }
        }
```

Replace with:
```rust
        RecipeChoice::FilterFade => {
            // Always-selectable default crossfade. Higher score on energy-up,
            // but never falls to 0 — beats EqualPower's 0.10 floor.
            if inputs.energy_delta > 0.20       { 0.85 }
            else if inputs.energy_delta > 0.05  { 0.70 }
            else if inputs.energy_delta > -0.05 { 0.55 }
            else                                { 0.30 }
        }
```

Find the existing `EqualPower` arm:
```rust
        RecipeChoice::EqualPower => 0.15,
```

Replace with:
```rust
        RecipeChoice::EqualPower => 0.10,
```

- [ ] **Step 2: Update affected existing tests**

Find any tests in `engine/tests/recipe_test.rs` that assert specific FilterFade or EqualPower score values. Update the expected values to match the new tiers. Common cases:

(a) An EqualPower test that previously asserted 0.15:
```rust
assert!((score_recipe(RecipeChoice::EqualPower, &inputs) - 0.10).abs() < 1e-9);
```

(b) FilterFade tests for the four new tiers — add if missing:
```rust
#[test]
fn score_filter_fade_strong_energy_up_returns_0_85() {
    let inputs = make_inputs_with_energy_delta(0.25);
    assert!((score_recipe(RecipeChoice::FilterFade, &inputs) - 0.85).abs() < 1e-9);
}

#[test]
fn score_filter_fade_moderate_energy_up_returns_0_70() {
    let inputs = make_inputs_with_energy_delta(0.10);
    assert!((score_recipe(RecipeChoice::FilterFade, &inputs) - 0.70).abs() < 1e-9);
}

#[test]
fn score_filter_fade_flat_energy_returns_0_55() {
    let inputs = make_inputs_with_energy_delta(0.0);
    assert!((score_recipe(RecipeChoice::FilterFade, &inputs) - 0.55).abs() < 1e-9);
}

#[test]
fn score_filter_fade_energy_down_returns_0_30() {
    let inputs = make_inputs_with_energy_delta(-0.20);
    assert!((score_recipe(RecipeChoice::FilterFade, &inputs) - 0.30).abs() < 1e-9);
}
```

`make_inputs_with_energy_delta` is a helper — if it doesn't exist, inline the PairScoreInputs literal (using sensible defaults for the other fields).

- [ ] **Step 3: Build + run tests**

```powershell
cargo test --manifest-path engine/Cargo.toml 2>&1 | Select-Object -Last 20
```
Expected: all PASS.

- [ ] **Step 4: Commit**

```powershell
git add engine/src/recipe.rs engine/tests/recipe_test.rs
git commit -m "feat(engine): promote FilterFade to default crossfade (0.30-0.85); demote EqualPower to 0.10"
```

---

## Task 4: Delete the 8 dropped recipes

**Files:**
- Modify: `engine/src/recipe.rs` (delete 8 struct + impl blocks, 8 RecipeChoice variants, 8 score branches, 8 candidates entries, 8 recipe_choice_id entries, 8 recency_hash entries)
- Modify: `engine/src/main.rs` (delete 8 dispatch arms + 8 clap ForceRecipe variants)
- Modify: `engine/tests/recipe_test.rs` (delete tests for the 8 dropped recipes)
- Modify: `engine/tests/script_test.rs` (delete build_script tests for the 8 dropped recipes)

The 8 recipes to delete: `EchoOut`, `EchoThrow`, `ThreeBandSwap`, `HighsFirst`, `BreakdownBridge`, `DoubleDrop`, `VocalOverlay`, `BreakdownIntoDrop`.

- [ ] **Step 1: Delete struct + Recipe impl blocks**

In `engine/src/recipe.rs`, find and delete each of these struct definitions and their `impl Recipe for X` blocks:
- `pub struct EchoOut;` and `impl Recipe for EchoOut { ... }`
- `pub struct EchoThrow;` and `impl Recipe for EchoThrow { ... }`
- `pub struct ThreeBandSwap;` and `impl Recipe for ThreeBandSwap { ... }`
- `pub struct HighsFirst;` and `impl Recipe for HighsFirst { ... }`
- `pub struct BreakdownBridge;` and `impl Recipe for BreakdownBridge { ... }`
- `pub struct DoubleDrop;` and `impl Recipe for DoubleDrop { ... }`
- `pub struct VocalOverlay;` and `impl Recipe for VocalOverlay { ... }`
- `pub struct BreakdownIntoDrop;` and `impl Recipe for BreakdownIntoDrop { ... }`

- [ ] **Step 2: Delete RecipeChoice variants**

In the `RecipeChoice` enum, delete:
```rust
    EchoOut,
    LoopOut,         // KEEP — this stays
    ThreeBandSwap,
    EchoThrow,
    HighsFirst,
    Drop,            // KEEP — this stays
    BreakdownBridge,
    DoubleDrop,
    VocalOverlay,
    BreakdownIntoDrop,
```

Actually DO NOT delete `LoopOut` or `Drop` — they're kept. Final variants kept: `EqualPower`, `HardCut`, `BassSwap` (will be deleted in Task 5), `FilterFade`, `FilterKill`, `EchoOut` (delete), `LoopOut` (keep), `ThreeBandSwap` (delete), `EchoThrow` (delete), `HighsFirst` (delete), `Drop` (keep), `BreakdownBridge` (delete), `DoubleDrop` (delete), `VocalOverlay` (delete), `BassSwapStemAware` (will be deleted in Task 5), `BreakdownIntoDrop` (delete), `RhythmSwap` (added in Task 2).

So in this task, delete exactly 8 variants: `EchoOut`, `ThreeBandSwap`, `EchoThrow`, `HighsFirst`, `BreakdownBridge`, `DoubleDrop`, `VocalOverlay`, `BreakdownIntoDrop`.

- [ ] **Step 3: Delete from `recipe_choice_id`**

Delete 8 arms:
```rust
        RecipeChoice::EchoOut => 6,
        RecipeChoice::ThreeBandSwap => 8,
        RecipeChoice::EchoThrow => 9,
        RecipeChoice::HighsFirst => 10,
        RecipeChoice::BreakdownBridge => 12,
        RecipeChoice::DoubleDrop => 13,
        RecipeChoice::VocalOverlay => 14,
        RecipeChoice::BreakdownIntoDrop => 16,
```

- [ ] **Step 4: Delete from candidates arrays**

In both `pick_recipe` and `pick_recipe_with_scores`, delete the 8 corresponding entries from the candidates array. After this task and Task 5, the array should have 7 entries: `EqualPower`, `HardCut`, `FilterFade`, `FilterKill`, `LoopOut`, `Drop`, `RhythmSwap`.

- [ ] **Step 5: Delete from `recency_hash` for-loop list**

Delete the 8 corresponding entries.

- [ ] **Step 6: Delete from `score_recipe` match**

Delete 8 match arms — one per dropped recipe. The match must remain exhaustive after the deletions.

- [ ] **Step 7: Delete from `engine/src/main.rs`**

Find the dispatch match — delete the 8 arms for the dropped recipes:
```rust
        RecipeChoice::EchoOut => Box::new(EchoOut),
        // ... etc for the other 7
```

Find the clap `ForceRecipe` enum — delete the 8 variants. (Clap will no longer accept `echo-out`, `echo-throw`, etc. — that's the intended behavior.)

- [ ] **Step 8: Delete tests**

In `engine/tests/recipe_test.rs`:
- Delete any `score_*` test for one of the 8 dropped recipes.
- Delete any test that asserts a dropped variant is in the candidates list.

In `engine/tests/script_test.rs`:
- Delete any `_emits_*_phases` test for one of the 8 dropped recipes.

Search for the deleted recipe names to find all references:
```powershell
Select-String -Path engine/tests/*.rs,engine/src/*.rs -Pattern 'EchoOut|EchoThrow|ThreeBandSwap|HighsFirst|BreakdownBridge|DoubleDrop|VocalOverlay|BreakdownIntoDrop' -SimpleMatch
```
Every remaining reference (after structs are deleted) needs to be removed.

- [ ] **Step 9: Build + run all tests**

```powershell
cargo build --manifest-path engine/Cargo.toml 2>&1 | Select-Object -Last 20
cargo test --manifest-path engine/Cargo.toml 2>&1 | Select-Object -Last 20
```
Expected: clean build, all tests PASS.

- [ ] **Step 10: Commit**

```powershell
git add engine/src/recipe.rs engine/src/main.rs engine/tests/recipe_test.rs engine/tests/script_test.rs
git commit -m "feat(engine): drop 8 underperforming recipes (echo-out, echo-throw, three-band-swap, highs-first, breakdown-bridge, double-drop, vocal-overlay, breakdown-into-drop)"
```

---

## Task 5: Delete `BassSwap` + `BassSwapStemAware` (merged into RhythmSwap)

**Files:**
- Modify: `engine/src/recipe.rs`
- Modify: `engine/src/main.rs`
- Modify: `engine/tests/recipe_test.rs`
- Modify: `engine/tests/script_test.rs`

Same pattern as Task 4, but for 2 recipes that are functionally replaced by `RhythmSwap`.

- [ ] **Step 1: Delete the structs + impls**

Delete from `engine/src/recipe.rs`:
- `pub struct BassSwap;` and `impl Recipe for BassSwap { ... }`
- `pub struct BassSwapStemAware;` and `impl Recipe for BassSwapStemAware { ... }`

- [ ] **Step 2: Delete from RecipeChoice + candidates + recipe_choice_id + recency_hash + score_recipe**

Same mechanical pattern as Task 4, for `BassSwap` and `BassSwapStemAware`.

After this task, RecipeChoice has exactly 7 variants: `EqualPower`, `HardCut`, `FilterFade`, `FilterKill`, `LoopOut`, `Drop`, `RhythmSwap`.

- [ ] **Step 3: Delete from main.rs dispatch + clap ForceRecipe**

Delete the 2 arms in the dispatch match and the 2 variants in the clap enum.

- [ ] **Step 4: Delete tests**

Search and remove:
```powershell
Select-String -Path engine/tests/*.rs,engine/src/*.rs -Pattern 'BassSwap|BassSwapStemAware' -SimpleMatch
```

Special care: `RhythmSwap` contains the substring `Swap` — make sure not to grep for raw `Swap` and accidentally hit RhythmSwap. Use the full names.

- [ ] **Step 5: Build + run all tests**

```powershell
cargo build --manifest-path engine/Cargo.toml
cargo test --manifest-path engine/Cargo.toml 2>&1 | Select-Object -Last 20
```
Expected: clean build, all tests PASS.

Confirm the final RecipeChoice variant count is exactly 7 by inspection.

- [ ] **Step 6: Commit**

```powershell
git add engine/src/recipe.rs engine/src/main.rs engine/tests/recipe_test.rs engine/tests/script_test.rs
git commit -m "feat(engine): drop bass-swap and bass-swap-stem-aware (merged into rhythm-swap)"
```

---

## Task 6: Dead-code comment for `EchoLine`

**Files:**
- Modify: `engine/src/effects.rs`

- [ ] **Step 1: Add the comment**

At the top of `engine/src/effects.rs`, after any existing module docstring, add:
```rust
// NOTE: EchoLine is currently unused — both echo-based recipes (echo-out,
// echo-throw) were dropped in the 2026-05-29 catalog consolidation after
// listening feedback found the echo character not audibly perceptible. The
// implementation is kept here for future echo recipes once the audibility
// is investigated. See docs/recipe-feedback-2026-05-29.md and
// docs/superpowers/specs/2026-05-29-recipe-catalog-consolidation-design.md.
```

If `cargo build` produces dead-code warnings about EchoLine, add `#[allow(dead_code)]` on the impl block:
```rust
#[allow(dead_code)]
impl EchoLine {
    // ...
}
```

- [ ] **Step 2: Build to verify**

```powershell
cargo build --manifest-path engine/Cargo.toml
```
Expected: no new warnings.

- [ ] **Step 3: Commit**

```powershell
git add engine/src/effects.rs
git commit -m "chore(engine): mark EchoLine as unused pending echo subsystem investigation"
```

---

## Task 7: Release build + live verification

**Files:**
- Side-effect only: engine binary rebuilt, listening session.

- [ ] **Step 1: Stop the running engine**

```powershell
Get-Process -Name dj-engine -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1
```

- [ ] **Step 2: Release build**

```powershell
cargo build --manifest-path engine/Cargo.toml --release
```
Expected: clean release build.

- [ ] **Step 3: Restart engine (natural picker, no --force-recipe)**

```powershell
$env:ICECAST_URL = 'icecast://localhost:8000/dj.mp3'
$env:ICECAST_USER = 'source'
$env:ICECAST_PASSWORD = 'hackme'
& engine/target/release/dj-engine.exe --sidecars analyzed-real --music-root data --log-format pretty
```

Run in background so the user can listen and observe logs concurrently.

- [ ] **Step 4: Watch for the natural recipe distribution**

Tail the log over several transitions:
```powershell
Get-Content -Wait <log-path> | Select-String 'transition arming' | ForEach-Object { $_ -replace '\x1b\[[\d;]*m','' }
```

Confirm in the log:
- The `scores=...` field lists exactly 7 candidates (the new catalog).
- The most-picked recipe across several transitions is `filter_fade` (the new default).
- `rhythm_swap` fires occasionally on pairs that satisfy its scoring conditions (drums-heavy, drums-similar, phrase-aligned, length ≥ 8).
- `equal_power_crossfade` is rare (only when nothing else scores well).
- None of the 8 dropped recipes appear.
- None of `bass_swap_crossfade` or `bass_swap_stem_aware` appear.

- [ ] **Step 5: Force-test RhythmSwap once**

To verify the recipe works end-to-end:
```powershell
Stop-Process -Name dj-engine -Force
& engine/target/release/dj-engine.exe --sidecars analyzed-real --music-root data --log-format pretty --force-recipe rhythm-swap
```
Listen to the first forced transition. Verify:
- Outgoing's drums + bass audibly fade out over the first half while incoming's drums + bass fade in.
- At bar L/2, outgoing's vocals/melody snap-cut to incoming's vocals/melody on a downbeat.
- One melody at a time throughout (no "two melodies" ambiguity).

- [ ] **Step 6: No commit** — observation only.

---

## Spec-Coverage Self-Check

- ✅ PairScoreInputs gets drum_density_mean fields → Task 1
- ✅ RhythmSwap struct + Recipe impl (gradual reveal + fast handoff) → Task 2
- ✅ RhythmSwap registered in picker (enum + candidates + recipe_choice_id + recency_hash + dispatch + clap) → Task 2
- ✅ RhythmSwap scoring with drums-similar gate → Task 2
- ✅ FilterFade promoted to default (0.30–0.85) → Task 3
- ✅ EqualPower demoted to 0.10 → Task 3
- ✅ 8 dropped recipes deleted → Task 4
- ✅ bass-swap + bass-swap-stem-aware deleted → Task 5
- ✅ EchoLine marked dead-code (kept) → Task 6
- ✅ Live verification of new catalog → Task 7

Open items resolved during execution:
- Whether `LoopGate` automation target needs comment (NO — `LoopOut` still uses it).
- Whether `EchoMix` automation target needs comment (NO — kept in the codebase, no recipes use it but it's a passive primitive).
- Final RecipeChoice variant count verification (= 7, in Task 5).
