# Event Recipes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add three event recipes (`Drop`, `BreakdownBridge`, `DoubleDrop`) that ship a non-crossfade gesture each, with internal fixed lengths (4 / 4 / 2 bars), scoring branches that gate firing to musically appropriate pairs, and full CLI + log wiring.

**Architecture:** Pure additive recipe-layer work. Three new `Recipe` impls in `recipe.rs`. Three new `RecipeChoice` variants. Three new `score_recipe` arms. Three new `ForceRecipe` entries. No DSL phase additions, no compiler changes, no scheduler changes, no analyzer changes.

**Tech Stack:** Rust 2021, existing `automation::{Curve, AutomationTarget}`, existing `recipe_script::{TransitionScript, Phase, DeckRole, EqBand, bar, bars}`.

---

## File Structure

| File | New / Modify | Responsibility |
|---|---|---|
| `engine/src/recipe.rs` | Modify | Add 3 recipe structs + impls. Extend `RecipeChoice`, `recipe_choice_id`, `recency_hash`, `pick_recipe` candidates, `score_recipe`. |
| `engine/src/config.rs` | Modify | Extend `ForceRecipe` enum + `to_recipe_choice` match. |
| `engine/src/main.rs` | Modify | Extend `recipe_name` + `recipe_choice_name` helpers + scheduler-arm dispatch match. |
| `engine/tests/script_test.rs` | Modify | +3 phase-shape tests. |
| `engine/tests/recipe_test.rs` | Modify | +3 scoring tests. |

---

### Task 1: Add the three Recipe structs and impls

**Files:**
- Modify: `engine/src/recipe.rs`

- [ ] **Step 1: Read `recipe.rs` to find the insertion point**

The new recipes go just before the `pub enum RecipeChoice` declaration, after the existing `HighsFirst` impl (the last recipe before the enum). Use Grep on `pub enum RecipeChoice` to find the right line.

- [ ] **Step 2: Insert `Drop` struct and impl**

Insert the following BEFORE `pub enum RecipeChoice`:

```rust
/// `Drop`: incoming plays for 4 bars under a high-pass filter (treble-only
/// build-up), then hard-cuts to full bandwidth on the next downbeat while
/// outgoing fades to silence. The HP-filter sweeps from 0.85 → 0.55 over the
/// build, giving the listener a "riser" — gradually revealing the mid-band —
/// followed by a hard release. Outgoing plays untouched until bar 4.
pub struct Drop;

impl Recipe for Drop {
    fn name(&self) -> &'static str {
        "drop"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = 4.0;

        TransitionScript {
            length_bars: 4,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(l, l),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.7,
                    to: 0.7,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.7,
                    to: 1.0,
                    span: bars(l, l),
                    curve: Curve::Step,
                },
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 0.85,
                    to: 0.85,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 0.85,
                    to: 0.55,
                    span: bars(0.0, l),
                    curve: Curve::Linear,
                },
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 0.0,
                    span: bars(l, l),
                    curve: Curve::Step,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}
```

- [ ] **Step 3: Insert `BreakdownBridge` struct and impl**

Append right after `Drop`'s impl:

```rust
/// `BreakdownBridge`: outgoing's mids + bass are killed over the first 3 bars
/// (the breakdown — only outgoing's highs ring through), then at bar 4 the
/// outgoing hard-cuts to silence and the incoming hard-cuts in at full volume.
/// The contrast between thin-outgoing and full-incoming is the gesture.
pub struct BreakdownBridge;

impl Recipe for BreakdownBridge {
    fn name(&self) -> &'static str {
        "breakdown_bridge"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = 4.0;

        TransitionScript {
            length_bars: 4,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::EqRamp {
                    deck: DeckRole::Outgoing,
                    band: EqBand::Low,
                    from_db: 0.0,
                    to_db: -40.0,
                    span: bars(0.0, 3.0),
                    curve: Curve::Linear,
                },
                Phase::EqRamp {
                    deck: DeckRole::Outgoing,
                    band: EqBand::Mid,
                    from_db: 0.0,
                    to_db: -40.0,
                    span: bars(0.0, 3.0),
                    curve: Curve::Linear,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(l, l),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 0.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(l, l),
                    curve: Curve::Step,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}
```

- [ ] **Step 4: Insert `DoubleDrop` struct and impl**

Append right after `BreakdownBridge`'s impl:

```rust
/// `DoubleDrop`: outgoing fades to silence over half a bar; both decks held
/// silent for half a bar (anticipation gap); both hard-cut to full volume on
/// the next downbeat; both play together for 1 bar; outgoing hard-cuts out
/// on bar 2 leaving incoming as the new master.
pub struct DoubleDrop;

impl Recipe for DoubleDrop {
    fn name(&self) -> &'static str {
        "double_drop"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = 2.0;

        TransitionScript {
            length_bars: 2,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, 0.5),
                    curve: Curve::Linear,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 0.0,
                    to: 1.0,
                    span: bars(1.0, 1.0),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(l, l),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 0.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(1.0, 1.0),
                    curve: Curve::Step,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}
```

- [ ] **Step 5: Build to confirm the structs compile**

Run:
```
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --release 2>&1 | Select-Object -Last 5
```

Expected: clean build. Probable failure mode: cargo complains the `RecipeChoice` enum doesn't have variants for the new recipes — that's fine, Task 2 fixes it.

If `Drop`/`BreakdownBridge`/`DoubleDrop` clash with any existing identifier, rename to `EventDrop`/etc.

- [ ] **Step 6: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): Drop / BreakdownBridge / DoubleDrop event recipes"
```

---

### Task 2: Wire `RecipeChoice` and scoring

**Files:**
- Modify: `engine/src/recipe.rs`

- [ ] **Step 1: Extend `RecipeChoice` enum**

Find:

```rust
pub enum RecipeChoice {
    // ... 10 existing variants ...
    HighsFirst,
}
```

Replace closing `HighsFirst,\n}` with:

```rust
    HighsFirst,
    Drop,
    BreakdownBridge,
    DoubleDrop,
}
```

- [ ] **Step 2: Update `recipe_choice_id`**

Find `fn recipe_choice_id`. Append three arms:

```rust
        RecipeChoice::Drop => 11,
        RecipeChoice::BreakdownBridge => 12,
        RecipeChoice::DoubleDrop => 13,
```

- [ ] **Step 3: Update `recency_hash`**

Find `fn recency_hash`. The array of variants needs to grow to 13. Append the three new variants to the existing array (in order: Drop, BreakdownBridge, DoubleDrop).

- [ ] **Step 4: Update `pick_recipe` candidates**

Find the `let candidates = [...]` array inside `pick_recipe` and `pick_recipe_with_scores`. Append the three new variants to BOTH arrays:

```rust
        RecipeChoice::HighsFirst,
        RecipeChoice::Drop,
        RecipeChoice::BreakdownBridge,
        RecipeChoice::DoubleDrop,
    ];
```

- [ ] **Step 5: Add scoring branches**

Find `pub fn score_recipe`. Add three arms between `RecipeChoice::HighsFirst => { ... }` and the closing `}`:

```rust
        RecipeChoice::Drop => {
            let big_ramp = inputs.energy_delta > 0.20 && inputs.phrase_aligned;
            let modest_ramp = inputs.energy_delta > 0.10 && inputs.phrase_aligned;
            if big_ramp {
                0.75
            } else if modest_ramp {
                0.40
            } else {
                0.10
            }
        }
        RecipeChoice::BreakdownBridge => {
            let big_winddown = inputs.energy_delta < -0.20 && inputs.phrase_aligned;
            let modest_winddown = inputs.energy_delta < -0.10 && inputs.phrase_aligned;
            if big_winddown {
                0.70
            } else if modest_winddown {
                0.40
            } else {
                0.10
            }
        }
        RecipeChoice::DoubleDrop => {
            let jackpot = inputs.length_bars >= 16
                && inputs.energy_delta.abs() < 0.05
                && inputs.phrase_aligned;
            if jackpot {
                0.80
            } else {
                0.05
            }
        }
```

- [ ] **Step 6: Build**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --release 2>&1 | Select-Object -Last 5
```

Expected: builds. Any "non-exhaustive match" errors in other files (e.g., `main.rs`, `config.rs`) signal where Task 3 needs to add coverage. Note them and proceed to Task 3.

- [ ] **Step 7: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): RecipeChoice + score_recipe gain 3 event recipes"
```

---

### Task 3: Wire CLI + main.rs dispatch

**Files:**
- Modify: `engine/src/config.rs`
- Modify: `engine/src/main.rs`

- [ ] **Step 1: Extend `ForceRecipe` enum in `config.rs`**

Find `pub enum ForceRecipe`. Append:

```rust
    Drop,
    BreakdownBridge,
    DoubleDrop,
```

Find the `to_recipe_choice` method. Append three arms:

```rust
            ForceRecipe::Drop => RecipeChoice::Drop,
            ForceRecipe::BreakdownBridge => RecipeChoice::BreakdownBridge,
            ForceRecipe::DoubleDrop => RecipeChoice::DoubleDrop,
```

- [ ] **Step 2: Extend `main.rs` helpers and dispatch**

Find the helper `recipe_choice_name` (or `recipe_name` — whichever maps `RecipeChoice` to a string). Add three arms:

```rust
        RecipeChoice::Drop => "drop",
        RecipeChoice::BreakdownBridge => "breakdown_bridge",
        RecipeChoice::DoubleDrop => "double_drop",
```

Apply to **both** mapping helpers if both exist (`recipe_name` and `recipe_choice_name` — one feeds the `transition arming` log's `recipe` field, the other feeds the `scores=` compact list).

Find the scheduler-arm match (`match choice { ... }` that calls `TransitionScheduler::arm`). Add three arms (mirror the existing variants' arm calls):

```rust
            dj_engine::recipe::RecipeChoice::Drop => {
                dj_engine::scheduler::TransitionScheduler::arm(
                    &mut mixer,
                    current_deck,
                    other_deck,
                    &current,
                    &next_track,
                    &plan,
                    &dj_engine::recipe::Drop,
                )
                .await?
            }
            dj_engine::recipe::RecipeChoice::BreakdownBridge => {
                dj_engine::scheduler::TransitionScheduler::arm(
                    &mut mixer,
                    current_deck,
                    other_deck,
                    &current,
                    &next_track,
                    &plan,
                    &dj_engine::recipe::BreakdownBridge,
                )
                .await?
            }
            dj_engine::recipe::RecipeChoice::DoubleDrop => {
                dj_engine::scheduler::TransitionScheduler::arm(
                    &mut mixer,
                    current_deck,
                    other_deck,
                    &current,
                    &next_track,
                    &plan,
                    &dj_engine::recipe::DoubleDrop,
                )
                .await?
            }
```

- [ ] **Step 3: Build**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --release 2>&1 | Select-Object -Last 5
```

If `cargo build --release` fails because `dj-engine.exe` is locked by the running stream, use `cargo check --all-targets` instead — same exhaustiveness check without overwriting the binary.

Expected: clean build (or check). If `config.rs::ForceRecipe` derives `clap::ValueEnum` and the variants need kebab-case rename hints, default clap behavior produces `drop`, `breakdown-bridge`, `double-drop` — acceptable.

- [ ] **Step 4: Verify `--force-recipe` parses the new values**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run --release -- --help 2>&1 | Select-String 'force-recipe' -Context 0,3
```

If `dj-engine.exe` is locked, skip this verification step. The build check above is sufficient.

- [ ] **Step 5: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/config.rs engine/src/main.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): --force-recipe accepts 3 event recipes + main.rs dispatch"
```

---

### Task 4: Tests — phase shapes and scoring

**Files:**
- Modify: `engine/tests/script_test.rs`
- Modify: `engine/tests/recipe_test.rs`

- [ ] **Step 1: Append phase-shape tests to `script_test.rs`**

Append to `engine/tests/script_test.rs`:

```rust

#[test]
fn drop_emits_filter_buildup_and_release() {
    use dj_engine::recipe::Drop;
    let outgoing = track();
    let incoming = track();
    let p = plan(16);
    let ctx = TransitionContext {
        outgoing: &outgoing,
        incoming: &incoming,
        plan: &p,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let script = Drop.build_script(&ctx);
    assert_eq!(script.length_bars, 4);

    // Filter sweep 0.85 → 0.55 over [0, 4].
    let has_buildup = script.phases.iter().any(|ph| matches!(ph,
        Phase::Filter { deck: DeckRole::Incoming, from, to, span, curve: Curve::Linear }
            if (*from - 0.85).abs() < 1e-9 && (*to - 0.55).abs() < 1e-9
               && (span.start.bars - 0.0).abs() < 1e-9 && (span.end.bars - 4.0).abs() < 1e-9));
    assert!(has_buildup, "filter build-up sweep on [0, 4]");

    // Filter snap to 0.0 at bar 4.
    let has_release = script.phases.iter().any(|ph| matches!(ph,
        Phase::Filter { deck: DeckRole::Incoming, from, to, span, curve: Curve::Step }
            if (*from - 0.0).abs() < 1e-9 && (*to - 0.0).abs() < 1e-9
               && (span.start.bars - 4.0).abs() < 1e-9 && (span.end.bars - 4.0).abs() < 1e-9));
    assert!(has_release, "filter release step at bar 4");
}

#[test]
fn breakdown_bridge_kills_outgoing_mids_and_bass() {
    use dj_engine::recipe::BreakdownBridge;
    let outgoing = track();
    let incoming = track();
    let p = plan(16);
    let ctx = TransitionContext {
        outgoing: &outgoing,
        incoming: &incoming,
        plan: &p,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let script = BreakdownBridge.build_script(&ctx);
    assert_eq!(script.length_bars, 4);

    let has_low_kill = script.phases.iter().any(|ph| matches!(ph,
        Phase::EqRamp { deck: DeckRole::Outgoing, band: EqBand::Low, from_db, to_db, .. }
            if (*from_db - 0.0).abs() < 1e-9 && (*to_db - (-40.0)).abs() < 1e-9));
    assert!(has_low_kill, "outgoing low kill");

    let has_mid_kill = script.phases.iter().any(|ph| matches!(ph,
        Phase::EqRamp { deck: DeckRole::Outgoing, band: EqBand::Mid, from_db, to_db, .. }
            if (*from_db - 0.0).abs() < 1e-9 && (*to_db - (-40.0)).abs() < 1e-9));
    assert!(has_mid_kill, "outgoing mid kill");

    let has_incoming_drop = script.phases.iter().any(|ph| matches!(ph,
        Phase::Fader { deck: DeckRole::Incoming, from, to, span, curve: Curve::Step }
            if (*from - 0.0).abs() < 1e-9 && (*to - 1.0).abs() < 1e-9
               && (span.start.bars - 4.0).abs() < 1e-9));
    assert!(has_incoming_drop, "incoming hard-cut in at bar 4");
}

#[test]
fn double_drop_holds_silence_then_both_play() {
    use dj_engine::recipe::DoubleDrop;
    let outgoing = track();
    let incoming = track();
    let p = plan(16);
    let ctx = TransitionContext {
        outgoing: &outgoing,
        incoming: &incoming,
        plan: &p,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let script = DoubleDrop.build_script(&ctx);
    assert_eq!(script.length_bars, 2);

    // Outgoing fades to silence over [0, 0.5].
    let has_initial_fadeout = script.phases.iter().any(|ph| matches!(ph,
        Phase::Fader { deck: DeckRole::Outgoing, from, to, span, curve: Curve::Linear }
            if (*from - 1.0).abs() < 1e-9 && (*to - 0.0).abs() < 1e-9
               && (span.start.bars - 0.0).abs() < 1e-9 && (span.end.bars - 0.5).abs() < 1e-9));
    assert!(has_initial_fadeout, "outgoing initial fade to silence");

    // Both decks come back at bar 1.
    let has_outgoing_return = script.phases.iter().any(|ph| matches!(ph,
        Phase::Fader { deck: DeckRole::Outgoing, from, to, span, curve: Curve::Step }
            if (*from - 0.0).abs() < 1e-9 && (*to - 1.0).abs() < 1e-9
               && (span.start.bars - 1.0).abs() < 1e-9));
    assert!(has_outgoing_return, "outgoing returns at bar 1");

    let has_incoming_return = script.phases.iter().any(|ph| matches!(ph,
        Phase::Fader { deck: DeckRole::Incoming, from, to, span, curve: Curve::Step }
            if (*from - 0.0).abs() < 1e-9 && (*to - 1.0).abs() < 1e-9
               && (span.start.bars - 1.0).abs() < 1e-9));
    assert!(has_incoming_return, "incoming hard-cuts in at bar 1");
}
```

- [ ] **Step 2: Append scoring tests to `recipe_test.rs`**

Append to `engine/tests/recipe_test.rs`:

```rust

#[test]
fn score_drop_big_ramp_up() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 8,
        use_eq_swap: false,
        energy_delta: 0.30,
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
    };
    assert!((score_recipe(RecipeChoice::Drop, &inputs) - 0.75).abs() < 1e-9);
}

#[test]
fn score_breakdown_bridge_winddown() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 8,
        use_eq_swap: false,
        energy_delta: -0.25,
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
    };
    assert!((score_recipe(RecipeChoice::BreakdownBridge, &inputs) - 0.70).abs() < 1e-9);
}

#[test]
fn score_double_drop_jackpot() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.02,
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
    };
    assert!((score_recipe(RecipeChoice::DoubleDrop, &inputs) - 0.80).abs() < 1e-9);
}

#[test]
fn score_double_drop_misses_when_energy_too_far() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.20, // outside the < 0.05 gate
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
    };
    assert!((score_recipe(RecipeChoice::DoubleDrop, &inputs) - 0.05).abs() < 1e-9);
}
```

- [ ] **Step 3: Run the full test suite**

```
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test 2>&1 | Select-Object -Last 15
```

Expected: all tests pass. If a pre-existing `pick_recipe` test fails because the new variants edged out the previous winner with jitter, inspect — most likely you need to assert "any of these top recipes" instead of one specific recipe. Fix the test to be tolerant of the new variants.

- [ ] **Step 4: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/tests/script_test.rs engine/tests/recipe_test.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "test(engine): phase-shape + scoring tests for event recipes"
```

---

### Task 5: cargo fmt + clippy + final sweep

- [ ] **Step 1: fmt**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" fmt
```

- [ ] **Step 2: clippy**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" clippy --all-targets -- -D warnings
```

Fix any warnings inline.

- [ ] **Step 3: Final test sweep**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test
```

Expected: all green.

- [ ] **Step 4: Commit if fmt/clippy made changes**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine status
```

If anything changed:

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add -u
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "chore(engine): fmt + clippy after event recipes"
```

---

## Self-review notes

- **Spec coverage:**
  - 3 Recipe impls → Task 1.
  - RecipeChoice + scoring wiring → Task 2.
  - CLI + main.rs dispatch → Task 3.
  - Phase shape + scoring tests → Task 4.
  - Final polish → Task 5.
- **Placeholders:** none.
- **Type consistency:** Each recipe's struct name (`Drop`, `BreakdownBridge`, `DoubleDrop`) matches its `RecipeChoice` variant, its `ForceRecipe` variant, and its `recipe_name` / `recipe_choice_name` string. The string-name pattern matches existing recipes (snake_case for log strings; PascalCase for variant names).
- **Risk: `Drop` shadowing.** `Drop` is Rust's prelude trait. Naming the recipe `Drop` shadows it locally. If the recipe module already uses `std::ops::Drop` (it doesn't, per inspection of `recipe.rs`), rename to `EventDrop`. Otherwise `Drop` is fine — it's used as a struct, not a trait.
- **Risk: `recipe_choice_name` may not exist as a separate helper.** Per the prior implementer's report on the logging fix, `recipe_choice_name` was added alongside `recipe_name`. If you find only one helper, update that one. If you find neither (very unlikely), the `scores=` log field uses inline match. Read `main.rs` and adapt.
- **The running engine has `dj-engine.exe` locked.** Task 3 Step 3 will fail at `cargo build --release` because Windows holds the binary. Use `cargo check --all-targets` for verification; the binary build happens after the user rebuilds + restarts the engine.
