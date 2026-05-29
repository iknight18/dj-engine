# RhythmHandoff Recipe Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the `RhythmHandoff` stems-aware recipe that hands off the rhythm section (drums + bass) under outgoing's melody during an 8-bar layering window, then hands off the melody at the phrase boundary. The picker selects it when stems are available, both tracks are phrase-aligned, the transition is ≥8 bars, both tracks have drum-heavy outro/intro, and energy delta is moderate.

**Architecture:** Single new struct `RhythmHandoff` in `engine/src/recipe.rs` implementing the `Recipe` trait via `build_script`. Uses existing `Phase::StemFader` infrastructure (no new automation primitives). Picker integration adds a `RecipeChoice::RhythmHandoff` variant. Scoring adds drum-density signals to `PairScoreInputs`.

**Tech Stack:** Rust 2021, serde, tracing. Tests via `cargo test`.

**Spec:** `docs/superpowers/specs/2026-05-29-rhythm-handoff-recipe-design.md`

---

## Task 1: Extend `PairScoreInputs` with drum-density fields

**Files:**
- Modify: `engine/src/recipe.rs` (`PairScoreInputs` struct + `from_pair` impl)
- Modify: `engine/tests/recipe_test.rs` (add tests for the new fields)

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

    let outgoing = test_loaded_track();  // stems = None
    let incoming = test_loaded_track();
    let plan = test_plan_16_bars();
    let inputs = PairScoreInputs::from_pair(&plan, &outgoing, &incoming);
    assert_eq!(inputs.outgoing_drum_density_mean, 0.0);
    assert_eq!(inputs.incoming_drum_density_mean, 0.0);
}
```

NOTE: `test_loaded_track()` and `test_plan_16_bars()` are existing helpers in `recipe_test.rs`. If they don't exist, find equivalent fixtures in the file and use those. The test file already has many tests against `PairScoreInputs::from_pair` — copy a fixture pattern from there.

- [ ] **Step 2: Run to see fail**

```powershell
cargo test --manifest-path engine/Cargo.toml --test recipe_test pair_score_inputs_drum 2>&1 | Select-Object -First 30
```
Expected: compile error — `PairScoreInputs` has no `outgoing_drum_density_mean` / `incoming_drum_density_mean` fields.

- [ ] **Step 3: Extend the struct and `from_pair`**

In `engine/src/recipe.rs`, the existing `PairScoreInputs` (find by `pub struct PairScoreInputs`):
```rust
#[derive(Clone, Debug)]
pub struct PairScoreInputs {
    pub length_bars: usize,
    pub use_eq_swap: bool,
    pub energy_delta: f64,
    pub outgoing_bass: f64,
    pub incoming_bass: f64,
    pub phrase_aligned: bool,
    pub stems_available: bool,
}
```

Add two fields at the end:
```rust
    /// Mean of the per-bar drum_density array for outgoing. 0.0 when stems absent.
    pub outgoing_drum_density_mean: f64,
    /// Mean of the per-bar drum_density array for incoming. 0.0 when stems absent.
    pub incoming_drum_density_mean: f64,
```

In `PairScoreInputs::from_pair`, after computing `stems_available`, add:
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

And add both fields to the returned `Self { ... }` literal.

- [ ] **Step 4: Verify tests pass**

```powershell
cargo test --manifest-path engine/Cargo.toml --test recipe_test pair_score_inputs_drum
```
Expected: both new tests PASS.

- [ ] **Step 5: Run full engine suite to confirm no regressions**

```powershell
cargo test --manifest-path engine/Cargo.toml 2>&1 | Select-Object -Last 20
```
Expected: all tests pass.

- [ ] **Step 6: Commit**

```powershell
git add engine/src/recipe.rs engine/tests/recipe_test.rs
git commit -m "feat(engine): PairScoreInputs adds drum_density_mean for outgoing + incoming"
```

---

## Task 2: Add the `RhythmHandoff` recipe (struct + Recipe impl)

**Files:**
- Modify: `engine/src/recipe.rs` (new struct + impl)
- Modify: `engine/tests/script_test.rs` (build_script test)

- [ ] **Step 1: Write the failing test**

Append to `engine/tests/script_test.rs`:
```rust
#[test]
fn rhythm_handoff_emits_expected_stem_phases() {
    use dj_engine::deck::StemKind;
    use dj_engine::recipe::RhythmHandoff;
    use dj_engine::recipe::Recipe;
    use dj_engine::recipe_script::{DeckRole, Phase};

    let outgoing = test_loaded_track();
    let incoming = test_loaded_track();
    let plan = test_plan_16_bars();  // length_bars=16
    let ctx = test_ctx(&outgoing, &incoming, &plan, 0);

    let script = RhythmHandoff.build_script(&ctx);

    // Outgoing drums + bass: Linear ramp from 1→0 over bars [0, 1].
    let out_drums_ramp = script.phases.iter().find(|p| matches!(
        p,
        Phase::StemFader {
            deck: DeckRole::Outgoing,
            stem: StemKind::Drums,
            from,
            to,
            ..
        } if (from - 1.0).abs() < 1e-9 && (to - 0.0).abs() < 1e-9
    ));
    assert!(out_drums_ramp.is_some(), "outgoing.drums Linear ramp missing");

    let out_bass_ramp = script.phases.iter().find(|p| matches!(
        p,
        Phase::StemFader {
            deck: DeckRole::Outgoing,
            stem: StemKind::Bass,
            from,
            to,
            ..
        } if (from - 1.0).abs() < 1e-9 && (to - 0.0).abs() < 1e-9
    ));
    assert!(out_bass_ramp.is_some(), "outgoing.bass Linear ramp missing");

    // Incoming drums + bass: Step 1 at bar 0.
    let in_drums_step = script.phases.iter().any(|p| matches!(
        p,
        Phase::StemFader {
            deck: DeckRole::Incoming,
            stem: StemKind::Drums,
            from,
            to,
            ..
        } if (from - 1.0).abs() < 1e-9 && (to - 1.0).abs() < 1e-9
    ));
    assert!(in_drums_step, "incoming.drums Step missing");

    // Incoming vocals/other: Step 0 at bar 0 (held off).
    let in_vocals_hold = script.phases.iter().any(|p| matches!(
        p,
        Phase::StemFader {
            deck: DeckRole::Incoming,
            stem: StemKind::Vocals,
            from,
            to,
            ..
        } if (from - 0.0).abs() < 1e-9 && (to - 0.0).abs() < 1e-9
    ));
    assert!(in_vocals_hold, "incoming.vocals hold-at-0 missing");

    // At least one phase has its span end at bar 8 (the handoff).
    let any_handoff = script.phases.iter().any(|p| match p {
        Phase::StemFader { span, .. } => (span.end.bars - 8.0).abs() < 1e-9,
        _ => false,
    });
    assert!(any_handoff, "no phase lands at handoff bar 8");
}

#[test]
fn rhythm_handoff_requires_stems_is_true() {
    use dj_engine::recipe::{Recipe, RhythmHandoff};
    assert!(RhythmHandoff.requires_stems());
}
```

- [ ] **Step 2: Run to see fail**

```powershell
cargo test --manifest-path engine/Cargo.toml --test script_test rhythm_handoff 2>&1 | Select-Object -First 30
```
Expected: compile errors — `RhythmHandoff` does not exist.

- [ ] **Step 3: Implement the recipe**

Append to `engine/src/recipe.rs` (after the last existing recipe struct, e.g., after `BreakdownIntoDrop`):

```rust
/// `RhythmHandoff`: stems-aware transition that hands off the rhythm section
/// (drums + bass) under outgoing's melody during an 8-bar layering window,
/// then hands off the melody (vocals + other) at the phrase boundary at bar 8.
///
/// What the listener hears:
/// - Bar 0: outgoing rhythm section fades down (1-bar Linear ramp); incoming
///   rhythm section steps up. Listener hears outgoing's full mix briefly
///   overlapping with incoming drums + bass.
/// - Bars 1–7: outgoing vocals + other (melody) on top, incoming drums + bass
///   underneath. One melody, new rhythm. No melodic/vocal clash.
/// - Bar 8: outgoing vocals + other step to 0; incoming vocals + other step
///   to 1. Melody hand-off on the downbeat. By now the new groove is
///   established, so the melody handoff feels like an arrival.
///
/// Requires stems on both decks. Picker only selects it when
/// `plan.length_bars >= 8`.
pub struct RhythmHandoff;

impl Recipe for RhythmHandoff {
    fn name(&self) -> &'static str {
        "rhythm_handoff"
    }

    fn requires_stems(&self) -> bool {
        true
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        use crate::deck::StemKind;

        let ratio = ctx.ratio;
        let handoff = 8.0_f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                // Outgoing rhythm section dies over the first bar.
                Phase::StemFader {
                    deck: DeckRole::Outgoing,
                    stem: StemKind::Drums,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, 1.0),
                    curve: Curve::Linear,
                },
                Phase::StemFader {
                    deck: DeckRole::Outgoing,
                    stem: StemKind::Bass,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, 1.0),
                    curve: Curve::Linear,
                },
                // Incoming mixed fader to 1 at bar 0 (cue_stems leaves fader=0).
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 1.0,
                    to: 1.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                // Incoming drums + bass enter at bar 0.
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Drums,
                    from: 1.0,
                    to: 1.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Bass,
                    from: 1.0,
                    to: 1.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                // Incoming melody held off at bar 0.
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
                // Phrase handoff: outgoing melody dies, incoming melody enters.
                Phase::StemFader {
                    deck: DeckRole::Outgoing,
                    stem: StemKind::Vocals,
                    from: 0.0,
                    to: 0.0,
                    span: bars(handoff, handoff),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Outgoing,
                    stem: StemKind::Other,
                    from: 0.0,
                    to: 0.0,
                    span: bars(handoff, handoff),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Vocals,
                    from: 1.0,
                    to: 1.0,
                    span: bars(handoff, handoff),
                    curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming,
                    stem: StemKind::Other,
                    from: 1.0,
                    to: 1.0,
                    span: bars(handoff, handoff),
                    curve: Curve::Step,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}
```

- [ ] **Step 4: Verify tests pass**

```powershell
cargo test --manifest-path engine/Cargo.toml --test script_test rhythm_handoff
```
Expected: both new tests PASS.

- [ ] **Step 5: Run full engine suite**

```powershell
cargo test --manifest-path engine/Cargo.toml 2>&1 | Select-Object -Last 20
```

- [ ] **Step 6: Commit**

```powershell
git add engine/src/recipe.rs engine/tests/script_test.rs
git commit -m "feat(engine): RhythmHandoff recipe — stems-aware rhythm-section handoff"
```

---

## Task 3: Register `RhythmHandoff` in the picker

**Files:**
- Modify: `engine/src/recipe.rs` (`RecipeChoice` enum, `pick_recipe`, `pick_recipe_with_scores`, `recipe_choice_id`)
- Modify: `engine/src/recipe_recency.rs` (if it enumerates RecipeChoice variants)

- [ ] **Step 1: Write the failing test**

Append to `engine/tests/recipe_test.rs`:
```rust
#[test]
fn pick_recipe_with_scores_includes_rhythm_handoff() {
    use dj_engine::recipe::{pick_recipe_with_scores, RecipeChoice};
    use dj_engine::recipe_recency::RecipeRecency;

    let outgoing = test_loaded_track();
    let incoming = test_loaded_track();
    let plan = test_plan_16_bars();
    let recency = RecipeRecency::default();

    let (_, scored) = pick_recipe_with_scores(&plan, &outgoing, &incoming, &recency);
    let names: Vec<_> = scored.iter().map(|(c, _)| *c).collect();
    assert!(names.contains(&RecipeChoice::RhythmHandoff));
}
```

- [ ] **Step 2: Run to see fail**

```powershell
cargo test --manifest-path engine/Cargo.toml --test recipe_test pick_recipe_with_scores_includes_rhythm_handoff 2>&1 | Select-Object -First 30
```
Expected: `RecipeChoice::RhythmHandoff` doesn't exist.

- [ ] **Step 3: Add the variant**

In `engine/src/recipe.rs`, find `pub enum RecipeChoice` and append the new variant after `BreakdownIntoDrop`:
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
    ThreeBandSwap,
    EchoThrow,
    HighsFirst,
    Drop,
    BreakdownBridge,
    DoubleDrop,
    VocalOverlay,
    BassSwapStemAware,
    BreakdownIntoDrop,
    RhythmHandoff,
}
```

- [ ] **Step 4: Extend `recipe_choice_id`**

In the same file, find `fn recipe_choice_id(c: RecipeChoice) -> u64` and add:
```rust
        RecipeChoice::RhythmHandoff => 17,
```

- [ ] **Step 5: Add to `candidates` lists**

In both `pick_recipe` and `pick_recipe_with_scores`, the `candidates` array currently lists 16 variants. Append `RecipeChoice::RhythmHandoff` at the end of each array.

- [ ] **Step 6: Add to `recency_hash` enumeration**

In the same file, find `fn recency_hash` and append `RecipeChoice::RhythmHandoff` to its for-loop list.

- [ ] **Step 7: Audit `recipe_recency.rs`**

```powershell
Get-Content engine/src/recipe_recency.rs
```
If the file enumerates RecipeChoice variants anywhere (likely yes — for the "contains" lookup), add `RhythmHandoff`. If it uses a HashSet keyed on RecipeChoice, no change is needed (PartialEq+Eq+Hash on RecipeChoice handles it).

- [ ] **Step 8: Add to selector / build dispatch**

```powershell
Select-String -Path engine/src -Pattern "RecipeChoice::" -SimpleMatch | Select-Object Path -Unique
```
Find every site that pattern-matches on RecipeChoice (likely a `match` statement that maps each variant to its concrete `Recipe` impl for the `build` call). The site is probably in `engine/src/main.rs` or in a helper. Add the `RhythmHandoff` arm:
```rust
        RecipeChoice::RhythmHandoff => Box::new(RhythmHandoff),
```
(Or the equivalent dispatch pattern — match the existing style.)

If you can't find the dispatch site after the grep above, search for the existing match arm for `BreakdownIntoDrop` — the new variant goes right alongside it.

- [ ] **Step 9: Add to --force-recipe clap enum (if it exists)**

```powershell
Select-String -Path engine/src/main.rs -Pattern "force_recipe|force-recipe" -SimpleMatch -Context 0,5
```
If there's a clap-derived enum mapping `RecipeChoice` to CLI flag values, add `RhythmHandoff` (it auto-becomes `rhythm-handoff` via clap's kebab-case rename).

- [ ] **Step 10: Build + run all tests**

```powershell
cargo build --manifest-path engine/Cargo.toml 2>&1 | Select-Object -Last 20
cargo test --manifest-path engine/Cargo.toml 2>&1 | Select-Object -Last 20
```
Expected: clean build, all tests pass (including the new variant test).

- [ ] **Step 11: Commit**

```powershell
git add engine/src/recipe.rs engine/src/recipe_recency.rs engine/src/main.rs engine/tests/recipe_test.rs
git commit -m "feat(engine): register RhythmHandoff in picker + recency + dispatch"
```

---

## Task 4: Score `RhythmHandoff`

**Files:**
- Modify: `engine/src/recipe.rs` (`score_recipe` function)
- Modify: `engine/tests/recipe_test.rs` (scoring tests)

- [ ] **Step 1: Write failing tests**

Append to `engine/tests/recipe_test.rs`:
```rust
#[test]
fn score_rhythm_handoff_drums_heavy_phrase_aligned_returns_strong() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};

    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.05,
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
        stems_available: true,
        outgoing_drum_density_mean: 0.75,
        incoming_drum_density_mean: 0.80,
    };
    let s = score_recipe(RecipeChoice::RhythmHandoff, &inputs);
    assert!((s - 0.88).abs() < 1e-9, "expected 0.88, got {s}");
}

#[test]
fn score_rhythm_handoff_drums_light_returns_floor() {
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
    let s = score_recipe(RecipeChoice::RhythmHandoff, &inputs);
    assert!((s - 0.30).abs() < 1e-9, "expected 0.30 (weak fallback), got {s}");
}

#[test]
fn score_rhythm_handoff_short_length_returns_floor() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};

    let inputs = PairScoreInputs {
        length_bars: 4,
        use_eq_swap: true,
        energy_delta: 0.05,
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
        stems_available: true,
        outgoing_drum_density_mean: 0.75,
        incoming_drum_density_mean: 0.80,
    };
    let s = score_recipe(RecipeChoice::RhythmHandoff, &inputs);
    assert!((s - 0.05).abs() < 1e-9, "expected 0.05 (length too short), got {s}");
}

#[test]
fn score_rhythm_handoff_no_stems_returns_floor() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};

    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.05,
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
        stems_available: false,
        outgoing_drum_density_mean: 0.75,
        incoming_drum_density_mean: 0.80,
    };
    let s = score_recipe(RecipeChoice::RhythmHandoff, &inputs);
    assert!((s - 0.05).abs() < 1e-9, "expected 0.05 (no stems), got {s}");
}

#[test]
fn score_rhythm_handoff_big_energy_delta_returns_floor() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};

    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.35,  // big ramp — wrong recipe for this
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
        stems_available: true,
        outgoing_drum_density_mean: 0.75,
        incoming_drum_density_mean: 0.80,
    };
    let s = score_recipe(RecipeChoice::RhythmHandoff, &inputs);
    // Drums-heavy, length OK, stems OK, phrase aligned — but energy delta is large.
    // Falls to the medium tier 0.30 (not strong, but better than 0.05 floor since
    // stems_available and length >= 8).
    assert!((s - 0.30).abs() < 1e-9, "expected 0.30 (energy delta too big), got {s}");
}
```

- [ ] **Step 2: Run to see fail**

```powershell
cargo test --manifest-path engine/Cargo.toml --test recipe_test score_rhythm_handoff 2>&1 | Select-Object -First 30
```
Expected: missing match arm in `score_recipe`.

- [ ] **Step 3: Add the score branch**

In `engine/src/recipe.rs`, find the `score_recipe` function and add the `RhythmHandoff` arm at the end of the match (after `BreakdownIntoDrop`):
```rust
        RecipeChoice::RhythmHandoff => {
            let drums_heavy = inputs.outgoing_drum_density_mean > 0.5
                && inputs.incoming_drum_density_mean > 0.5;
            let layering_long_enough = inputs.length_bars >= 8;
            let energy_moderate = inputs.energy_delta.abs() < 0.20;
            let strong = inputs.stems_available
                && inputs.phrase_aligned
                && layering_long_enough
                && drums_heavy
                && energy_moderate;
            if strong {
                0.88
            } else if inputs.stems_available && layering_long_enough {
                0.30
            } else {
                0.05
            }
        }
```

- [ ] **Step 4: Verify tests pass**

```powershell
cargo test --manifest-path engine/Cargo.toml --test recipe_test score_rhythm_handoff
```
Expected: all 5 scoring tests PASS.

- [ ] **Step 5: Run full engine suite**

```powershell
cargo test --manifest-path engine/Cargo.toml 2>&1 | Select-Object -Last 20
```
Expected: all tests PASS.

- [ ] **Step 6: Commit**

```powershell
git add engine/src/recipe.rs engine/tests/recipe_test.rs
git commit -m "feat(engine): score RhythmHandoff — strong on drums-heavy + long phrase-aligned"
```

---

## Task 5: Rebuild release + live force-recipe verification

**Files:**
- No code changes; side-effect: engine binary rebuilt + restarted.

- [ ] **Step 1: Stop the running engine**

```powershell
Get-Process -Name dj-engine -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1
```

- [ ] **Step 2: Rebuild release**

```powershell
cargo build --manifest-path engine/Cargo.toml --release
```
Expected: clean build.

- [ ] **Step 3: Restart engine with --force-recipe rhythm-handoff**

```powershell
$env:ICECAST_URL = 'icecast://localhost:8000/dj.mp3'
$env:ICECAST_USER = 'source'
$env:ICECAST_PASSWORD = 'hackme'
& engine/target/release/dj-engine.exe --sidecars analyzed-real --music-root data --log-format pretty --force-recipe rhythm-handoff
```

(If `--force-recipe` doesn't accept `rhythm-handoff`, try `rhythm_handoff` or check the clap derive in `engine/src/main.rs` for the actual flag format.)

Run in background or another shell so the user can listen.

- [ ] **Step 4: Watch the log for `rhythm_handoff` arming**

The first transition should emit:
```
recipe="rhythm_handoff" transition_type=...
```

If it doesn't (the picker still chooses a different recipe), `--force-recipe` may not be wired correctly. Investigate by reading `engine/src/main.rs`.

- [ ] **Step 5: Listen for the gesture**

What to expect:
- Outgoing's drums fade out smoothly over the first bar.
- Outgoing's vocals/melody continue.
- Incoming's drums + bass enter underneath (steady groove).
- At bar 8, outgoing's vocals/melody hand off to incoming's.

If you hear:
- Outgoing's drums NOT fading out → outgoing.drums stem fader isn't being applied (revisit tick-order issue from the handoff doc).
- Incoming's vocals leaking in early → incoming.vocals stem fader isn't being applied at bar 0.
- Two melodies overlapping during the layering window → outgoing.vocals is at 0 too early, or incoming.vocals is at 1 too early.

Report findings.

- [ ] **Step 6: No commit for this task** — observation only. If you discover follow-up work, capture it.

---

## Spec-Coverage Self-Check

- ✅ Add `RhythmHandoff` struct + `Recipe` impl → Task 2
- ✅ Phase composition (drum/bass ramps, vocals/other holds, handoff steps) → Task 2
- ✅ `requires_stems()` returns true → Task 2 (covered by test)
- ✅ Extend `PairScoreInputs` with drum_density means → Task 1
- ✅ Add `RecipeChoice::RhythmHandoff` variant + register in candidates + recipe_choice_id + recency → Task 3
- ✅ Score function with strong/medium/floor tiers → Task 4
- ✅ Tests for build_script, scoring, score floor cases → Tasks 2 + 4
- ✅ Live verification via --force-recipe → Task 5

Open items resolved during execution:
- The exact `--force-recipe` flag name (clap kebab-case) — confirmed live in Task 5.
- Whether `recipe_recency.rs` needs editing — checked in Task 3 Step 7.
- Whether the dispatch site is in main.rs or a helper — Task 3 Step 8 covers both.
