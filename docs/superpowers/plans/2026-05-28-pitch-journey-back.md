# Pitch Journey Back to Natural BPM Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `Phase::TempoRamp` variant and an automatic injection in `build_and_compile` so every recipe with a modest BPM shift (`0.005 ≤ |ratio - 1.0| < 0.05`) gets a 32-bar tempo ramp back to natural BPM after the transition window.

**Architecture:** New variant in `recipe_script.rs::Phase` with serde derives. Compiler gets a new match arm that bypasses the per-bar clamp (the ramp intentionally extends past `length_bars`). Injection lives in `recipe.rs::build_and_compile` via a new private helper `inject_pitch_journey`. No individual recipe is modified — all 10 inherit the behavior automatically. No CLI, sidecar, scheduler, or mixer changes.

**Tech Stack:** Rust 2021, existing `automation::{Automation, AutomationTarget, Curve}`, existing DSL types.

---

## File Structure

| File | New / Modify | Responsibility |
|---|---|---|
| `engine/src/recipe_script.rs` | Modify | Add `Phase::TempoRamp { deck, from, to, span, curve }` variant. Add compile() match arm that emits a `Tempo` Linear automation WITHOUT bar-position clamping. |
| `engine/src/recipe.rs` | Modify | Add `inject_pitch_journey(script, ctx)` private function. Modify `build_and_compile` to call it between `recipe.build_script(ctx)` and `compile(&script, ctx)`. |
| `engine/tests/compile_test.rs` | Modify | 4 new tests for TempoRamp compile behavior. |
| `engine/tests/recipe_test.rs` | Modify | 5 new tests for injection logic. |

---

### Task 1: Add `Phase::TempoRamp` variant

**Files:**
- Modify: `engine/src/recipe_script.rs`

- [ ] **Step 1: Add the variant**

Find the existing `Phase` enum (around the middle of the file). After the existing `TempoAlign` variant, append:

```rust
    /// Linear tempo ramp on a deck. Unlike other phases, the span MAY extend past
    /// `TransitionScript::length_bars` — used by `build_and_compile` to inject a
    /// pitch-journey-back automation that runs during the new master's playback.
    TempoRamp {
        deck: DeckRole,
        from: f64,
        to: f64,
        span: BarSpan,
        curve: Curve,
    },
```

The variant participates in `#[derive(Clone, Debug, Serialize, Deserialize)]` already on the enum, so no further changes needed for serde.

- [ ] **Step 2: Build to confirm the enum compiles**

Run:
```
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --release 2>&1 | Select-Object -Last 5
```

Expected: a compile error in `compile()` because the match on `Phase` is no longer exhaustive (the new variant has no arm yet). That's fine; the next task adds the arm.

If any *other* file (not `recipe_script.rs`) errors with `non-exhaustive patterns` on Phase, add to the issue list and let the controller decide whether to extend match-coverage elsewhere. Currently the only `match` on `Phase` is inside `compile()`.

- [ ] **Step 3: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe_script.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): Phase::TempoRamp variant for pitch-journey-back"
```

---

### Task 2: Implement `TempoRamp` compile arm

**Files:**
- Modify: `engine/src/recipe_script.rs` (the `compile` function).

- [ ] **Step 1: Write a failing compile test first (TDD)**

Append to `engine/tests/compile_test.rs`:

```rust
#[test]
fn tempo_ramp_compiles_to_tempo_linear_automation() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::TempoRamp {
            deck: DeckRole::Incoming,
            from: 0.97,
            to: 1.0,
            span: bars(16.0, 48.0),
            curve: Curve::Linear,
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.incoming_automations.len(), 1);
    let a = &out.incoming_automations[0];
    assert_eq!(a.target, AutomationTarget::Tempo);
    assert_eq!(a.curve, Curve::Linear);
    assert!((a.from - 0.97).abs() < 1e-9);
    assert!((a.to - 1.0).abs() < 1e-9);
    assert_eq!(a.t_start_frames, 16 * 88200);
    assert_eq!(a.t_end_frames, 48 * 88200);
}

#[test]
fn tempo_ramp_span_is_not_clamped_to_length_bars() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::TempoRamp {
            deck: DeckRole::Incoming,
            from: 0.97,
            to: 1.0,
            span: bars(16.0, 64.0), // ends well past length_bars=16
            curve: Curve::Linear,
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.incoming_automations.len(), 1);
    assert_eq!(out.incoming_automations[0].t_end_frames, 64 * 88200);
}

#[test]
fn tempo_ramp_inverted_span_is_dropped() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::TempoRamp {
            deck: DeckRole::Incoming,
            from: 0.97,
            to: 1.0,
            span: BarSpan {
                start: BarPos { bars: 48.0 },
                end: BarPos { bars: 16.0 },
            },
            curve: Curve::Linear,
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.incoming_automations.len(), 0);
}

#[test]
fn tempo_ramp_zero_length_span_emits_one_frame_step() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::TempoRamp {
            deck: DeckRole::Incoming,
            from: 0.97,
            to: 1.0,
            span: bars(16.0, 16.0),
            curve: Curve::Linear,
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    // end == start → emit a single-frame automation (matches the behavior of
    // other phases with collapsed spans).
    assert_eq!(out.incoming_automations.len(), 1);
    assert_eq!(out.incoming_automations[0].t_end_frames, out.incoming_automations[0].t_start_frames + 1);
}
```

Run:
```
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --test compile_test 2>&1 | Select-Object -Last 10
```

Expected: compile error on the `match` in `compile()` (variant has no arm).

- [ ] **Step 2: Add the compile arm**

Find the match on `*phase` inside `compile()`. Add a new arm (place it after the existing `TempoAlign` arm to keep grouping):

```rust
            Phase::TempoRamp {
                deck,
                from,
                to,
                span,
                curve,
            } => {
                if !span.end.bars.is_finite() || !span.start.bars.is_finite() {
                    warn!("compile: TempoRamp has non-finite span, dropping phase");
                } else if span.end.bars < span.start.bars {
                    warn!(
                        "compile: TempoRamp end={} < start={}, dropping phase",
                        span.end.bars, span.start.bars
                    );
                } else {
                    // Direct frame conversion — do NOT clamp to length_bars.
                    // The journey intentionally extends past the transition window.
                    let t_start = t0 + (span.start.bars * frames_per_bar) as u64;
                    let t_end_raw = t0 + (span.end.bars * frames_per_bar) as u64;
                    let t_end = if t_end_raw <= t_start {
                        t_start + 1
                    } else {
                        t_end_raw
                    };
                    push!(
                        deck,
                        Automation {
                            target: AutomationTarget::Tempo,
                            t_start_frames: t_start,
                            t_end_frames: t_end,
                            from,
                            to,
                            curve,
                        }
                    );
                }
            }
```

**Note about `push!` vs closure:** the existing compile() uses a `push!` macro for the push pattern (per the implementer's earlier refactor noted in the commit history). Match the style — if the surrounding code uses `push!`, use it; if it uses a closure, use the closure. Read the surrounding TempoAlign arm and copy its idiom.

If `frames_per_bar` or `t0` are inside a closure scope that the new arm can't access directly, you may need to lift the computation into the variable bindings at the top of the function. They should already be in scope based on the spec — the existing `bar_to_frame` closure uses them.

- [ ] **Step 3: Run the failing tests; they should now pass**

```
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --test compile_test 2>&1 | Select-Object -Last 10
```

Expected: all 4 new tests pass. Old compile_test tests still pass.

- [ ] **Step 4: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe_script.rs engine/tests/compile_test.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): compile TempoRamp to Tempo Linear automation (no clamp)"
```

---

### Task 3: Add `inject_pitch_journey` and wire `build_and_compile`

**Files:**
- Modify: `engine/src/recipe.rs`

- [ ] **Step 1: Write failing tests for the injection logic (TDD)**

Append to `engine/tests/recipe_test.rs` (use the existing `sidecar_with`, `track` helpers if available; otherwise mirror their structure):

```rust
#[test]
fn build_and_compile_injects_tempo_ramp_for_modest_ratio() {
    use dj_engine::recipe::{build_and_compile, EqualPowerCrossfade};
    use dj_engine::automation::{AutomationTarget, Curve};

    // 120 BPM outgoing, 126 BPM incoming → ratio ≈ 0.9524 — in the journey band.
    let outgoing = track_with_bpm(120.0);
    let incoming = track_with_bpm(126.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
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
    let out = build_and_compile(&EqualPowerCrossfade, &ctx);

    let tempo_ramps: Vec<_> = out.incoming_automations
        .iter()
        .filter(|a| a.target == AutomationTarget::Tempo && a.curve == Curve::Linear)
        .collect();
    assert_eq!(tempo_ramps.len(), 1, "expected exactly one Tempo Linear automation");
    let ramp = tempo_ramps[0];
    assert!((ramp.from - (120.0 / 126.0)).abs() < 1e-6);
    assert!((ramp.to - 1.0).abs() < 1e-9);
}

#[test]
fn build_and_compile_does_not_inject_for_identical_bpm() {
    use dj_engine::recipe::{build_and_compile, EqualPowerCrossfade};
    use dj_engine::automation::{AutomationTarget, Curve};

    let outgoing = track_with_bpm(120.0);
    let incoming = track_with_bpm(120.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
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
    let out = build_and_compile(&EqualPowerCrossfade, &ctx);

    let tempo_ramps: Vec<_> = out.incoming_automations
        .iter()
        .filter(|a| a.target == AutomationTarget::Tempo && a.curve == Curve::Linear)
        .collect();
    assert_eq!(tempo_ramps.len(), 0, "should not inject when ratio == 1.0");
}

#[test]
fn build_and_compile_does_not_inject_for_large_ratio() {
    use dj_engine::recipe::{build_and_compile, EqualPowerCrossfade};
    use dj_engine::automation::{AutomationTarget, Curve};

    // 120 BPM → 152 BPM = ratio 0.789 — well outside the journey band.
    let outgoing = track_with_bpm(120.0);
    let incoming = track_with_bpm(152.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
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
    let out = build_and_compile(&EqualPowerCrossfade, &ctx);

    let tempo_ramps: Vec<_> = out.incoming_automations
        .iter()
        .filter(|a| a.target == AutomationTarget::Tempo && a.curve == Curve::Linear)
        .collect();
    assert_eq!(tempo_ramps.len(), 0, "should not inject when ratio is too far from 1.0");
}

#[test]
fn build_and_compile_does_not_inject_when_skip_tempo_align() {
    use dj_engine::recipe::{build_and_compile, EqualPowerCrossfade};
    use dj_engine::automation::{AutomationTarget, Curve};

    let outgoing = track_with_bpm(120.0);
    let incoming = track_with_bpm(125.0);  // modest ratio in normal circumstances
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 0,
        use_eq_swap: false,
        skip_tempo_align: true,  // the planner decided no-sync
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
    let out = build_and_compile(&dj_engine::recipe::HardCut, &ctx);

    let tempo_ramps: Vec<_> = out.incoming_automations
        .iter()
        .filter(|a| a.target == AutomationTarget::Tempo && a.curve == Curve::Linear)
        .collect();
    assert_eq!(tempo_ramps.len(), 0, "skip_tempo_align must suppress the journey");
}

#[test]
fn inject_pitch_journey_is_idempotent() {
    use dj_engine::recipe::{inject_pitch_journey, EqualPowerCrossfade, Recipe};
    use dj_engine::recipe_script::{DeckRole, Phase};

    let outgoing = track_with_bpm(120.0);
    let incoming = track_with_bpm(126.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
        skip_tempo_align: false,
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
    let mut script = EqualPowerCrossfade.build_script(&ctx);
    inject_pitch_journey(&mut script, &ctx);
    inject_pitch_journey(&mut script, &ctx);  // second call should be a no-op

    let ramps: Vec<_> = script.phases.iter()
        .filter(|p| matches!(p, Phase::TempoRamp { deck: DeckRole::Incoming, .. }))
        .collect();
    assert_eq!(ramps.len(), 1, "second injection should not duplicate the ramp");
}
```

You'll need a helper `track_with_bpm(bpm: f64) -> LoadedTrack` if not already in scope. If recipe_test.rs already has `track(bpm: f64)` (from earlier tests) or just `track()`, look at existing test helpers; otherwise add:

```rust
fn track_with_bpm(bpm: f64) -> LoadedTrack {
    LoadedTrack {
        sidecar_path: PathBuf::new(),
        sidecar: sidecar_with(bpm, 280.0),
    }
}
```

Make `inject_pitch_journey` `pub` so the idempotency test can call it directly. (The other tests use the public `build_and_compile`.)

- [ ] **Step 2: Run failing tests**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --test recipe_test 2>&1 | Select-Object -Last 15
```

Expected: 5 new test failures (either compile errors on `inject_pitch_journey` not being in scope, or runtime failures since the injection isn't there yet).

- [ ] **Step 3: Implement `inject_pitch_journey` and wire `build_and_compile`**

Find `build_and_compile` in `engine/src/recipe.rs`. Currently it should look like:

```rust
pub fn build_and_compile(recipe: &dyn Recipe, ctx: &TransitionContext<'_>) -> RecipeOutput {
    let script = recipe.build_script(ctx);
    crate::recipe_script::compile(&script, ctx)
}
```

If `build_and_compile` doesn't exist (it should, per the spec — sub-project trait migration added it), add it. If it exists with the body above, modify it to:

```rust
pub fn build_and_compile(recipe: &dyn Recipe, ctx: &TransitionContext<'_>) -> RecipeOutput {
    let mut script = recipe.build_script(ctx);
    inject_pitch_journey(&mut script, ctx);
    crate::recipe_script::compile(&script, ctx)
}

pub fn inject_pitch_journey(
    script: &mut crate::recipe_script::TransitionScript,
    ctx: &TransitionContext<'_>,
) {
    use crate::automation::Curve;
    use crate::recipe_script::{bars, DeckRole, Phase};

    if ctx.plan.skip_tempo_align {
        return;
    }

    // Skip if a journey ramp already exists (idempotency).
    let already_injected = script.phases.iter().any(|p| {
        matches!(
            p,
            Phase::TempoRamp { deck: DeckRole::Incoming, .. }
        )
    });
    if already_injected {
        return;
    }

    // Find the TempoAlign on Incoming; its `ratio` is what we journey back from.
    let ratio = script.phases.iter().find_map(|p| match p {
        Phase::TempoAlign {
            deck: DeckRole::Incoming,
            ratio,
        } => Some(*ratio),
        _ => None,
    });
    let Some(ratio) = ratio else { return; };

    let delta = (ratio - 1.0).abs();
    if !(0.005..0.05).contains(&delta) {
        return;
    }

    let l = script.length_bars as f64;
    script.phases.push(Phase::TempoRamp {
        deck: DeckRole::Incoming,
        from: ratio,
        to: 1.0,
        span: bars(l, l + 32.0),
        curve: Curve::Linear,
    });
}
```

Note: `inject_pitch_journey` is `pub` so the idempotency test can call it. The function is in the `recipe` module — test imports as `dj_engine::recipe::inject_pitch_journey`.

- [ ] **Step 4: Run all engine tests**

```
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test 2>&1 | Select-Object -Last 30
```

Expected: all tests pass, including the 5 new injection tests. Existing integration tests may add 1 to their Tempo automation count on the incoming deck for any pair whose ratio falls in the journey band. If a test asserts on the exact count and now fails, investigate:

- If the test pair has a ratio in [0.005, 0.05) → update the test's expected count to include the new ramp.
- If the test pair has ratio = 1.0 → no journey should fire, but the assertion may have been incomplete; investigate the failure carefully.

- [ ] **Step 5: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe.rs engine/tests/recipe_test.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): inject pitch-journey-back ramp in build_and_compile (modest-ratio band)"
```

---

### Task 4: cargo fmt + clippy + final sweep

- [ ] **Step 1: fmt**

```
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine
& "$env:USERPROFILE\.cargo\bin\cargo.exe" fmt
```

- [ ] **Step 2: clippy**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" clippy --all-targets -- -D warnings
```

Expected: no warnings. Fix any fires at the source.

- [ ] **Step 3: Full test sweep**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test
```

Expected: all tests pass.

- [ ] **Step 4: Commit if fmt/clippy made changes**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine status
```

If there are tracked changes, commit them:

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add -u
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "chore(engine): fmt + clippy after pitch-journey-back"
```

---

## Self-review notes

- **Spec coverage:**
  - `Phase::TempoRamp` variant → Task 1.
  - Compile arm with no clamping → Task 2.
  - `inject_pitch_journey` + `build_and_compile` wire-in → Task 3.
  - Idempotency check → Task 3.
  - Bypass when `skip_tempo_align` → Task 3.
  - All 4 compile tests + 5 injection tests → Tasks 2 & 3.
- **Placeholders:** None — every step contains complete code or an exact command.
- **Type consistency:** `Phase::TempoRamp` fields match `Phase::Fader` (deck, from, to, span, curve). `inject_pitch_journey` signature uses `&mut TransitionScript` consistently. Tests use the correct `pub fn` form.
- **Risk: ratio threshold.** `0.005..0.05` chosen per spec. Any test pair right on the boundary may flicker — make sure test pairs are well inside or well outside (the existing tests use 120 vs 126 BPM → 0.0476, just below 0.05 — safe; and 120 vs 152 → 0.211, well above).
- **Pre-existing test impact.** `recipe_test.rs::equal_power_diff_bpm_no_ramp_only_step` asserts "no Tempo Linear automation" with pair 120 → 124 BPM (ratio 0.9677, delta 0.0323 — in journey band). That test was correct BEFORE this change but will now fail because the injection adds exactly the ramp it asserts absent. Action: update the test. Rename to `equal_power_diff_bpm_has_journey_ramp_only_when_in_band` or similar, change assertions to expect the ramp. The "no ramp" assertion was protecting against a different historical behavior; reintroducing the ramp under conditions is the explicit goal.
