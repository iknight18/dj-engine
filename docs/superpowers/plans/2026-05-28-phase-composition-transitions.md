# Phase Composition for Transitions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce a declarative `TransitionScript` DSL for recipes (bars/beats), a `compile()` function that produces the existing frame-stamped `Automation`s, faithfully migrate the 7 existing recipes onto the DSL, and add three new recipes (`ThreeBandSwap`, `EchoThrow`, `HighsFirst`) that exercise EqMid/EqHigh — currently dead in transitions.

**Architecture:** New `recipe_script` module with `TransitionScript`, `Phase` (8 variants), `BarPos`, `BarSpan`, `DeckRole`, `EqBand`, plus `compile(script, ctx) -> RecipeOutput`. `Recipe` trait grows a `build_script(ctx) -> TransitionScript` method; existing `fn build(ctx) -> RecipeOutput` becomes a default that calls `compile(self.build_script(ctx), ctx)`. Recipes implement `build_script` only. All call sites unchanged. `RecipeChoice` grows three variants; `score_recipe` grows three branches; `--force-recipe` accepts three new string values.

**Tech Stack:** Rust 2021, `serde`, existing `automation::{Automation, AutomationTarget, Curve}`, existing `recipe::{Recipe, RecipeOutput, RecipeChoice, TransitionContext, DeckInitConfig}`.

---

## File Structure

| File | New / Modify | Responsibility |
|---|---|---|
| `engine/src/recipe_script.rs` | **New** | DSL types (`TransitionScript`, `Phase`, `BarPos`, `BarSpan`, `DeckRole`, `EqBand`), constructor helpers (`bar`, `bars`), `compile()` function, validation/clamping. |
| `engine/src/lib.rs` | Modify | `pub mod recipe_script;` |
| `engine/src/automation.rs` | Modify | Add `#[derive(Serialize, Deserialize)]` to `Curve`. |
| `engine/src/recipe.rs` | Modify | (a) `DeckInitConfig` gains serde derives. (b) `Recipe` trait grows `build_script`; `build` becomes default. (c) All 7 existing recipe impls switch from `build` to `build_script`, expressed as `TransitionScript`. (d) Three new recipe impls. (e) `RecipeChoice` gains 3 variants. (f) `score_recipe` gains 3 branches. (g) `pick_recipe` candidates array grows. (h) `recipe_choice_id` returns ids 8/9/10. (i) `recency_hash` iterates 10 variants. |
| `engine/src/config.rs` | Modify | `--force-recipe` value enum gains `three_band_swap`, `echo_throw`, `highs_first`. |
| `engine/src/main.rs` | Modify (if needed) | Map new `--force-recipe` values to `RecipeChoice` variants. (No other change.) |
| `engine/tests/compile_test.rs` | **New** | Unit tests for `compile()`: frame math, EchoThrow expansion, LoopGate expansion, EqStep, clamping, dropped phases, total duration. |
| `engine/tests/script_test.rs` | **New** | Phase-level assertions on `ThreeBandSwap`, `EchoThrow`, `HighsFirst` build_script outputs. |
| `engine/tests/recipe_test.rs` | Modify | Add scoring tests for the 3 new recipes; add `pick_recipe` tests confirming the new recipes are selectable. |

---

### Task 1: Add Serde derives to `Curve`

**Files:**
- Modify: `engine/src/automation.rs` (lines 5-15 area)
- Verify: `engine/Cargo.toml` already depends on `serde` with the `derive` feature.

- [ ] **Step 1: Verify serde is already in the dependency tree**

Run: `git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine grep -n "^serde" engine/Cargo.toml`

Expected output: a line like `serde = { version = "...", features = ["derive"] }` (the sidecar code already uses serde, so it's there).

If serde-with-derive is missing for some reason, stop and report. Otherwise proceed.

- [ ] **Step 2: Add the derive to `Curve`**

Edit `engine/src/automation.rs` at the `Curve` definition. Current:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    Linear,
    EqualPowerIn,
    EqualPowerOut,
    Step,
}
```

After:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Curve {
    Linear,
    EqualPowerIn,
    EqualPowerOut,
    Step,
}
```

- [ ] **Step 3: Build to confirm**

Run: `cargo build --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml`

Expected: success, no new warnings.

- [ ] **Step 4: Commit**

```bash
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/automation.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): serde derives on Curve (phase composition prep)"
```

---

### Task 2: Add Serde derives to `DeckInitConfig`

**Files:**
- Modify: `engine/src/recipe.rs` lines 19-30 area (the `DeckInitConfig` struct).

- [ ] **Step 1: Add the derive**

Current:

```rust
#[derive(Clone, Debug, Default)]
pub struct DeckInitConfig {
    pub echo_delay_seconds: Option<f64>,
    pub echo_feedback: Option<f64>,
    pub loop_bars: Option<usize>,
    pub loop_bar_seconds: Option<f64>,
}
```

After:

```rust
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct DeckInitConfig {
    pub echo_delay_seconds: Option<f64>,
    pub echo_feedback: Option<f64>,
    pub loop_bars: Option<usize>,
    pub loop_bar_seconds: Option<f64>,
}
```

- [ ] **Step 2: Build**

Run: `cargo build --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml`

Expected: success.

- [ ] **Step 3: Commit**

```bash
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): serde derives on DeckInitConfig (phase composition prep)"
```

---

### Task 3: Create `recipe_script.rs` with DSL types (no compile() yet)

**Files:**
- Create: `engine/src/recipe_script.rs`
- Modify: `engine/src/lib.rs` (add `pub mod recipe_script;`)

- [ ] **Step 1: Create the new module file**

Write `engine/src/recipe_script.rs`:

```rust
//! Phase-composition DSL for transitions.
//!
//! A `TransitionScript` is a declarative description of a transition in musical
//! time (bars/beats). The `compile` function in this module turns a script into
//! a `crate::recipe::RecipeOutput` (frame-stamped automations) that the
//! scheduler consumes.
//!
//! The DSL is serde-derived so a future sub-project can deserialize externally-
//! generated (e.g., AI-generated) scripts and pipe them through `compile`
//! without scheduler changes.

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
pub enum DeckRole {
    Outgoing,
    Incoming,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EqBand {
    Low,
    Mid,
    High,
}

/// Fractional bars from the transition window's start (bar 0).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BarPos {
    pub bars: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BarSpan {
    pub start: BarPos,
    pub end: BarPos,
}

/// Constructor helpers used by recipe `build_script` methods.
pub fn bar(n: f64) -> BarPos {
    BarPos { bars: n }
}

pub fn bars(start: f64, end: f64) -> BarSpan {
    BarSpan {
        start: BarPos { bars: start },
        end: BarPos { bars: end },
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Phase {
    Fader {
        deck: DeckRole,
        from: f64,
        to: f64,
        span: BarSpan,
        curve: Curve,
    },
    EqRamp {
        deck: DeckRole,
        band: EqBand,
        from_db: f64,
        to_db: f64,
        span: BarSpan,
        curve: Curve,
    },
    EqStep {
        deck: DeckRole,
        band: EqBand,
        value_db: f64,
        at: BarPos,
    },
    Filter {
        deck: DeckRole,
        from: f64,
        to: f64,
        span: BarSpan,
        curve: Curve,
    },
    EchoMix {
        deck: DeckRole,
        from: f64,
        to: f64,
        span: BarSpan,
        curve: Curve,
    },
    /// Set EchoMix to 1.0 at `at`, then back to 0.0 at `at + hold_bars`.
    EchoThrow {
        deck: DeckRole,
        at: BarPos,
        hold_bars: f64,
    },
    /// LoopGate on at `on_at`, off at `off_at`.
    LoopGate {
        deck: DeckRole,
        on_at: BarPos,
        off_at: BarPos,
    },
    /// Tempo Step at bar 0. Emitted once per script by `Incoming`.
    TempoAlign {
        deck: DeckRole,
        ratio: f64,
    },
}
```

- [ ] **Step 2: Register the module in `lib.rs`**

Find the module declarations near the top of `engine/src/lib.rs` (other `pub mod X;` lines). Add:

```rust
pub mod recipe_script;
```

Keep alphabetical order if existing entries are alphabetized; otherwise add at the bottom of that block.

- [ ] **Step 3: Build**

Run: `cargo build --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml`

Expected: builds with no errors. Unused-import warnings on `serde`/`Curve`/`DeckInitConfig` are fine since `compile` isn't here yet — proceed.

- [ ] **Step 4: Commit**

```bash
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe_script.rs engine/src/lib.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): TransitionScript / Phase DSL types (phase composition)"
```

---

### Task 4: Write `compile()` tests (TDD — they should fail)

**Files:**
- Create: `engine/tests/compile_test.rs`

- [ ] **Step 1: Write the failing tests**

Create `engine/tests/compile_test.rs`:

```rust
use dj_engine::automation::{AutomationTarget, Curve};
use dj_engine::library::LoadedTrack;
use dj_engine::recipe::{DeckId, TransitionContext};
use dj_engine::recipe_script::{
    bar, bars, compile, BarPos, BarSpan, DeckRole, EqBand, Phase, TransitionScript,
};
use dj_engine::sidecar::Sidecar;
use dj_engine::transition::TransitionPlan;
use std::path::PathBuf;

const SR: u32 = 44100;

fn sidecar_120() -> Sidecar {
    Sidecar {
        schema_version: 1,
        analyzer_version: "test".into(),
        source_path: "test.mp3".into(),
        source_mtime: 0.0,
        source_size_bytes: 0,
        duration_seconds: Some(300.0),
        sample_rate: Some(SR),
        bpm: Some(120.0),
        bpm_confidence: Some(1.0),
        beats_seconds: vec![],
        downbeats_seconds: vec![],
        key: None,
        key_label: None,
        key_confidence: None,
        energy_curve: vec![],
        energy_mean: Some(0.5),
        band_energy_means: None,
        cue_in_seconds: Some(0.0),
        cue_out_seconds: Some(280.0),
        intro_end_seconds: Some(16.0),
        outro_start_seconds: Some(200.0),
        is_mixable: Some(true),
        mixable_reasons: vec![],
        error: None,
        phrase_boundaries_seconds: None,
        phrase_confidence: None,
        band_energy_curves: None,
        is_beatmatchable: None,
        is_phrase_aligned: None,
    }
}

fn track() -> LoadedTrack {
    LoadedTrack {
        sidecar_path: PathBuf::new(),
        sidecar: sidecar_120(),
    }
}

fn make_ctx<'a>(
    outgoing: &'a LoadedTrack,
    incoming: &'a LoadedTrack,
    plan: &'a TransitionPlan,
    t0: u64,
) -> TransitionContext<'a> {
    TransitionContext {
        outgoing,
        incoming,
        plan,
        master_clock_frames: t0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    }
}

fn plan_16() -> TransitionPlan {
    TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
    }
}

#[test]
fn bar_to_frame_basic_math() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 1_000_000);

    // 120 BPM, 4/4: bar = (60/120) * 4 = 2.0s. At 44.1 kHz: 88200 frames/bar.
    // 4 bars from t0=1_000_000 → 1_000_000 + 4 * 88200 = 1_352_800.
    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::Fader {
            deck: DeckRole::Outgoing,
            from: 1.0,
            to: 0.0,
            span: bars(0.0, 4.0),
            curve: Curve::Linear,
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.outgoing_automations.len(), 1);
    let a = &out.outgoing_automations[0];
    assert_eq!(a.target, AutomationTarget::Fader);
    assert_eq!(a.t_start_frames, 1_000_000);
    assert_eq!(a.t_end_frames, 1_352_800);
    assert!((a.from - 1.0).abs() < 1e-12);
    assert!((a.to - 0.0).abs() < 1e-12);
}

#[test]
fn eq_step_emits_one_frame_step() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::EqStep {
            deck: DeckRole::Incoming,
            band: EqBand::Low,
            value_db: -40.0,
            at: bar(8.0),
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.incoming_automations.len(), 1);
    let a = &out.incoming_automations[0];
    assert_eq!(a.target, AutomationTarget::EqLow);
    assert_eq!(a.curve, Curve::Step);
    assert_eq!(a.t_end_frames, a.t_start_frames + 1);
    assert!((a.from - (-40.0)).abs() < 1e-12);
    assert!((a.to - (-40.0)).abs() < 1e-12);
    assert_eq!(a.t_start_frames, 8 * 88200);
}

#[test]
fn echo_throw_expands_to_two_step_automations() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::EchoThrow {
            deck: DeckRole::Outgoing,
            at: bar(14.0),
            hold_bars: 1.0,
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.outgoing_automations.len(), 2);
    let on = &out.outgoing_automations[0];
    let off = &out.outgoing_automations[1];
    assert_eq!(on.target, AutomationTarget::EchoMix);
    assert_eq!(on.curve, Curve::Step);
    assert!((on.from - 1.0).abs() < 1e-12);
    assert!((on.to - 1.0).abs() < 1e-12);
    assert_eq!(on.t_start_frames, 14 * 88200);
    assert_eq!(on.t_end_frames, 14 * 88200 + 1);
    assert_eq!(off.target, AutomationTarget::EchoMix);
    assert_eq!(off.curve, Curve::Step);
    assert!((off.from - 0.0).abs() < 1e-12);
    assert!((off.to - 0.0).abs() < 1e-12);
    assert_eq!(off.t_start_frames, 15 * 88200);
    assert_eq!(off.t_end_frames, 15 * 88200 + 1);
}

#[test]
fn loop_gate_expands_to_two_step_automations() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::LoopGate {
            deck: DeckRole::Outgoing,
            on_at: bar(0.0),
            off_at: bar(16.0),
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.outgoing_automations.len(), 2);
    assert_eq!(out.outgoing_automations[0].target, AutomationTarget::LoopGate);
    assert_eq!(out.outgoing_automations[0].curve, Curve::Step);
    assert_eq!(out.outgoing_automations[1].curve, Curve::Step);
    assert_eq!(out.outgoing_automations[1].t_start_frames, 16 * 88200);
}

#[test]
fn tempo_align_emits_one_frame_step_at_t0() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 500_000);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::TempoAlign {
            deck: DeckRole::Incoming,
            ratio: 0.9677,
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.incoming_automations.len(), 1);
    let a = &out.incoming_automations[0];
    assert_eq!(a.target, AutomationTarget::Tempo);
    assert_eq!(a.curve, Curve::Step);
    assert_eq!(a.t_start_frames, 500_000);
    assert_eq!(a.t_end_frames, 500_001);
    assert!((a.from - 0.9677).abs() < 1e-12);
    assert!((a.to - 0.9677).abs() < 1e-12);
}

#[test]
fn fader_span_with_equal_start_end_emits_one_frame_step() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 0,
        phases: vec![
            Phase::Fader {
                deck: DeckRole::Outgoing,
                from: 1.0,
                to: 0.0,
                span: bars(0.0, 0.0),
                curve: Curve::Step,
            },
            Phase::Fader {
                deck: DeckRole::Incoming,
                from: 0.0,
                to: 1.0,
                span: bars(0.0, 0.0),
                curve: Curve::Step,
            },
        ],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.outgoing_automations.len(), 1);
    assert_eq!(out.outgoing_automations[0].t_end_frames, 1);
    assert_eq!(out.outgoing_automations[0].curve, Curve::Step);
    assert_eq!(out.incoming_automations.len(), 1);
    assert_eq!(out.incoming_automations[0].t_end_frames, 1);
}

#[test]
fn out_of_range_barpos_is_clamped() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::EqStep {
            deck: DeckRole::Outgoing,
            band: EqBand::Mid,
            value_db: -10.0,
            at: bar(47.0), // way past length_bars
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.outgoing_automations.len(), 1);
    // Clamped to length_bars=16 → 16 * 88200 = 1_411_200.
    assert_eq!(out.outgoing_automations[0].t_start_frames, 1_411_200);
}

#[test]
fn eq_db_value_is_clamped_to_min_40() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::EqStep {
            deck: DeckRole::Outgoing,
            band: EqBand::Low,
            value_db: -55.0,
            at: bar(2.0),
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.outgoing_automations.len(), 1);
    assert!((out.outgoing_automations[0].from - (-40.0)).abs() < 1e-12);
    assert!((out.outgoing_automations[0].to - (-40.0)).abs() < 1e-12);
}

#[test]
fn inverted_span_is_dropped() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::Fader {
            deck: DeckRole::Outgoing,
            from: 1.0,
            to: 0.0,
            span: BarSpan {
                start: BarPos { bars: 5.0 },
                end: BarPos { bars: 3.0 }, // end < start → drop
            },
            curve: Curve::Linear,
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.outgoing_automations.len(), 0);
}

#[test]
fn total_duration_matches_length_bars() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    // 16 bars * 88200 frames/bar = 1_411_200
    assert_eq!(out.total_duration_frames, 1_411_200);
}

#[test]
fn hardcut_zero_length_total_duration_zero() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 0,
        phases: vec![],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.total_duration_frames, 0);
}

#[test]
fn init_configs_pass_through() {
    use dj_engine::recipe::DeckInitConfig;
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let init = DeckInitConfig {
        echo_delay_seconds: Some(0.5),
        echo_feedback: Some(0.75),
        loop_bars: None,
        loop_bar_seconds: None,
    };
    let script = TransitionScript {
        length_bars: 8,
        phases: vec![],
        outgoing_init: Some(init.clone()),
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    let out_init = out.outgoing_init.expect("outgoing_init should pass through");
    assert_eq!(out_init.echo_delay_seconds, Some(0.5));
    assert_eq!(out_init.echo_feedback, Some(0.75));
    assert!(out.incoming_init.is_none());
}
```

- [ ] **Step 2: Run tests to confirm they fail (compile function not implemented)**

Run: `cargo test --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml --test compile_test`

Expected: compilation error — `compile` is not defined in `recipe_script`. That's the "test fails because target doesn't exist yet" signal in Rust TDD.

- [ ] **Step 3: Commit the failing tests**

```bash
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/tests/compile_test.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "test(engine): failing tests for recipe_script::compile (TDD)"
```

---

### Task 5: Implement `compile()` to make Task 4 tests pass

**Files:**
- Modify: `engine/src/recipe_script.rs` (append to bottom).

- [ ] **Step 1: Append the `compile` function and helpers**

Add to `engine/src/recipe_script.rs` (below the type definitions):

```rust
use crate::automation::{Automation, AutomationTarget};
use crate::recipe::{RecipeOutput, TransitionContext};
use log::warn;

/// Compile a `TransitionScript` to a `RecipeOutput` (frame-stamped automations).
///
/// Clamps invalid inputs and logs a warn; never panics. Drops phases with
/// `BarSpan::end < BarSpan::start`. A span with `end == start` is emitted as a
/// 1-frame Step automation (the canonical "step at this BarPos" idiom).
pub fn compile(script: &TransitionScript, ctx: &TransitionContext<'_>) -> RecipeOutput {
    let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0).max(1.0);
    let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
    let frames_per_bar = bar_seconds * (ctx.sample_rate as f64);
    let length_bars_f = script.length_bars as f64;
    let t0 = ctx.master_clock_frames;

    let total_duration_frames = (length_bars_f * frames_per_bar) as u64;

    let bar_to_frame = |b: BarPos| -> u64 {
        let mut bars = b.bars;
        if !bars.is_finite() {
            warn!(
                "compile: non-finite BarPos.bars={}, clamping to 0",
                bars
            );
            bars = 0.0;
        }
        if bars < 0.0 {
            warn!(
                "compile: BarPos.bars={} < 0, clamping to 0",
                bars
            );
            bars = 0.0;
        }
        if bars > length_bars_f {
            warn!(
                "compile: BarPos.bars={} > length_bars={}, clamping",
                bars, length_bars_f
            );
            bars = length_bars_f;
        }
        t0 + (bars * frames_per_bar) as u64
    };

    let mut outgoing_automations: Vec<Automation> = Vec::new();
    let mut incoming_automations: Vec<Automation> = Vec::new();

    let push = |role: DeckRole,
                a: Automation,
                outgoing: &mut Vec<Automation>,
                incoming: &mut Vec<Automation>| match role {
        DeckRole::Outgoing => outgoing.push(a),
        DeckRole::Incoming => incoming.push(a),
    };

    let eq_target = |band: EqBand| match band {
        EqBand::Low => AutomationTarget::EqLow,
        EqBand::Mid => AutomationTarget::EqMid,
        EqBand::High => AutomationTarget::EqHigh,
    };

    let clamp01 = |v: f64| v.clamp(0.0, 1.0);
    let clamp_filter = |v: f64| v.clamp(-1.0, 1.0);
    let clamp_db = |v: f64| v.clamp(-40.0, 6.0);

    let emit_ramp = |deck: DeckRole,
                     target: AutomationTarget,
                     from: f64,
                     to: f64,
                     span: BarSpan,
                     curve: Curve,
                     outgoing: &mut Vec<Automation>,
                     incoming: &mut Vec<Automation>| {
        if !span.end.bars.is_finite() || !span.start.bars.is_finite() {
            warn!("compile: non-finite span, dropping phase");
            return;
        }
        if span.end.bars < span.start.bars {
            warn!(
                "compile: span end={} < start={}, dropping phase",
                span.end.bars, span.start.bars
            );
            return;
        }
        let t_start = bar_to_frame(span.start);
        let t_end = if (span.end.bars - span.start.bars).abs() < f64::EPSILON {
            t_start + 1
        } else {
            let raw = bar_to_frame(span.end);
            if raw <= t_start {
                t_start + 1
            } else {
                raw
            }
        };
        push(
            deck,
            Automation {
                target,
                t_start_frames: t_start,
                t_end_frames: t_end,
                from,
                to,
                curve,
            },
            outgoing,
            incoming,
        );
    };

    for phase in &script.phases {
        match *phase {
            Phase::Fader {
                deck,
                from,
                to,
                span,
                curve,
            } => emit_ramp(
                deck,
                AutomationTarget::Fader,
                clamp01(from),
                clamp01(to),
                span,
                curve,
                &mut outgoing_automations,
                &mut incoming_automations,
            ),
            Phase::EqRamp {
                deck,
                band,
                from_db,
                to_db,
                span,
                curve,
            } => emit_ramp(
                deck,
                eq_target(band),
                clamp_db(from_db),
                clamp_db(to_db),
                span,
                curve,
                &mut outgoing_automations,
                &mut incoming_automations,
            ),
            Phase::EqStep {
                deck,
                band,
                value_db,
                at,
            } => {
                let v = clamp_db(value_db);
                let t = bar_to_frame(at);
                push(
                    deck,
                    Automation {
                        target: eq_target(band),
                        t_start_frames: t,
                        t_end_frames: t + 1,
                        from: v,
                        to: v,
                        curve: Curve::Step,
                    },
                    &mut outgoing_automations,
                    &mut incoming_automations,
                );
            }
            Phase::Filter {
                deck,
                from,
                to,
                span,
                curve,
            } => emit_ramp(
                deck,
                AutomationTarget::Filter,
                clamp_filter(from),
                clamp_filter(to),
                span,
                curve,
                &mut outgoing_automations,
                &mut incoming_automations,
            ),
            Phase::EchoMix {
                deck,
                from,
                to,
                span,
                curve,
            } => emit_ramp(
                deck,
                AutomationTarget::EchoMix,
                clamp01(from),
                clamp01(to),
                span,
                curve,
                &mut outgoing_automations,
                &mut incoming_automations,
            ),
            Phase::EchoThrow {
                deck,
                at,
                hold_bars,
            } => {
                let t_on = bar_to_frame(at);
                let t_off = bar_to_frame(BarPos {
                    bars: at.bars + hold_bars.max(0.0),
                });
                push(
                    deck,
                    Automation {
                        target: AutomationTarget::EchoMix,
                        t_start_frames: t_on,
                        t_end_frames: t_on + 1,
                        from: 1.0,
                        to: 1.0,
                        curve: Curve::Step,
                    },
                    &mut outgoing_automations,
                    &mut incoming_automations,
                );
                push(
                    deck,
                    Automation {
                        target: AutomationTarget::EchoMix,
                        t_start_frames: t_off,
                        t_end_frames: t_off + 1,
                        from: 0.0,
                        to: 0.0,
                        curve: Curve::Step,
                    },
                    &mut outgoing_automations,
                    &mut incoming_automations,
                );
            }
            Phase::LoopGate {
                deck,
                on_at,
                off_at,
            } => {
                let t_on = bar_to_frame(on_at);
                let t_off = bar_to_frame(off_at);
                push(
                    deck,
                    Automation {
                        target: AutomationTarget::LoopGate,
                        t_start_frames: t_on,
                        t_end_frames: t_on + 1,
                        from: 1.0,
                        to: 1.0,
                        curve: Curve::Step,
                    },
                    &mut outgoing_automations,
                    &mut incoming_automations,
                );
                push(
                    deck,
                    Automation {
                        target: AutomationTarget::LoopGate,
                        t_start_frames: t_off,
                        t_end_frames: t_off + 1,
                        from: 0.0,
                        to: 0.0,
                        curve: Curve::Step,
                    },
                    &mut outgoing_automations,
                    &mut incoming_automations,
                );
            }
            Phase::TempoAlign { deck, ratio } => {
                push(
                    deck,
                    Automation {
                        target: AutomationTarget::Tempo,
                        t_start_frames: t0,
                        t_end_frames: t0 + 1,
                        from: ratio,
                        to: ratio,
                        curve: Curve::Step,
                    },
                    &mut outgoing_automations,
                    &mut incoming_automations,
                );
            }
        }
    }

    RecipeOutput {
        outgoing_automations,
        incoming_automations,
        outgoing_init: script.outgoing_init.clone(),
        incoming_init: script.incoming_init.clone(),
        total_duration_frames,
    }
}
```

- [ ] **Step 2: Build**

Run: `cargo build --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml`

Expected: builds. If `log::warn` isn't a usable import, replace `use log::warn;` with `use tracing::warn;` (the codebase uses `tracing` — verify by grepping `tracing::` vs `log::` in `engine/src/`). Project uses `tracing` based on `logging_setup.rs` naming; if `tracing` is the actual dep, use `use tracing::warn;`.

- [ ] **Step 3: Run the compile tests**

Run: `cargo test --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml --test compile_test`

Expected: all 12 tests pass.

- [ ] **Step 4: Commit**

```bash
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe_script.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): compile() function for TransitionScript (TDD green)"
```

---

### Task 6: Add `build_script` to `Recipe` trait; migrate the 7 existing recipes

This is the breaking change. The trait gains a `build_script` method; `build` becomes a default method that compiles the script. Each existing recipe is rewritten to implement `build_script` (deleting its old `build`). Existing integration tests assert `RecipeOutput`, which they get via the default `build`; they must pass with at most 1-frame drift.

**Files:**
- Modify: `engine/src/recipe.rs` (the `Recipe` trait + 7 recipe `impl` blocks at lines ~40-570).

- [ ] **Step 1: Update the trait**

Find in `engine/src/recipe.rs`:

```rust
pub trait Recipe {
    fn name(&self) -> &'static str;
    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput;
}
```

Replace with:

```rust
pub trait Recipe {
    fn name(&self) -> &'static str;
    fn build_script(&self, ctx: &TransitionContext<'_>) -> crate::recipe_script::TransitionScript;
    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        crate::recipe_script::compile(&self.build_script(ctx), ctx)
    }
}
```

- [ ] **Step 2: Add a use-block for the DSL helpers near the top of recipe.rs**

Just under the existing `use crate::automation::{AutomationTarget, Curve};` line, add:

```rust
use crate::recipe_script::{
    bar, bars, DeckRole, EqBand, Phase, TransitionScript,
};
```

- [ ] **Step 3: Migrate `EqualPowerCrossfade`**

Delete the existing `impl Recipe for EqualPowerCrossfade` block (lines 49-103) and replace with:

```rust
impl Recipe for EqualPowerCrossfade {
    fn name(&self) -> &'static str {
        "equal_power_crossfade"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = ctx.plan.length_bars as f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}
```

- [ ] **Step 4: Migrate `HardCut`**

Replace the existing `impl Recipe for HardCut` with:

```rust
impl Recipe for HardCut {
    fn name(&self) -> &'static str {
        "hard_cut"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);

        TransitionScript {
            length_bars: 0,
            phases: vec![
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}
```

- [ ] **Step 5: Migrate `BassSwap`**

Replace the existing `impl Recipe for BassSwap` with:

```rust
impl Recipe for BassSwap {
    fn name(&self) -> &'static str {
        "bass_swap_crossfade"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = ctx.plan.length_bars as f64;

        let mut phases = vec![
            Phase::Fader {
                deck: DeckRole::Outgoing,
                from: 1.0,
                to: 0.0,
                span: bars(0.0, l),
                curve: Curve::EqualPowerOut,
            },
            Phase::Fader {
                deck: DeckRole::Incoming,
                from: 0.0,
                to: 1.0,
                span: bars(0.0, l),
                curve: Curve::EqualPowerIn,
            },
            Phase::TempoAlign {
                deck: DeckRole::Incoming,
                ratio,
            },
        ];

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

        // Both bass-light → no EQ work.
        if outgoing_bass < 0.3 && incoming_bass < 0.3 {
            return TransitionScript {
                length_bars: ctx.plan.length_bars,
                phases,
                outgoing_init: None,
                incoming_init: None,
            };
        }

        let (kill_start, kill_end, rel_start, rel_end) =
            if outgoing_bass > 0.5 && incoming_bass > 0.5 {
                (0.30, 0.48, 0.52, 0.70)
            } else {
                (0.35, 0.48, 0.52, 0.65)
            };

        phases.push(Phase::EqStep {
            deck: DeckRole::Incoming,
            band: EqBand::Low,
            value_db: -40.0,
            at: bar(0.0),
        });
        phases.push(Phase::EqRamp {
            deck: DeckRole::Outgoing,
            band: EqBand::Low,
            from_db: 0.0,
            to_db: -40.0,
            span: bars(kill_start * l, kill_end * l),
            curve: Curve::Linear,
        });
        phases.push(Phase::EqRamp {
            deck: DeckRole::Incoming,
            band: EqBand::Low,
            from_db: -40.0,
            to_db: 0.0,
            span: bars(rel_start * l, rel_end * l),
            curve: Curve::Linear,
        });

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases,
            outgoing_init: None,
            incoming_init: None,
        }
    }
}
```

- [ ] **Step 6: Migrate `FilterFade`**

Replace `impl Recipe for FilterFade` with:

```rust
impl Recipe for FilterFade {
    fn name(&self) -> &'static str {
        "filter_fade"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = ctx.plan.length_bars as f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                // Initial filter Step (1-frame "init" via end==start).
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 1.0,
                    to: 1.0,
                    span: bars(0.0, 0.0),
                    curve: Curve::Step,
                },
                Phase::Filter {
                    deck: DeckRole::Incoming,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::Linear,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}
```

- [ ] **Step 7: Migrate `FilterKill`**

Replace `impl Recipe for FilterKill` with:

```rust
impl Recipe for FilterKill {
    fn name(&self) -> &'static str {
        "filter_kill"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = ctx.plan.length_bars as f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Filter {
                    deck: DeckRole::Outgoing,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, 0.6 * l),
                    curve: Curve::Linear,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}
```

- [ ] **Step 8: Migrate `EchoOut`**

Replace `impl Recipe for EchoOut` with:

```rust
impl Recipe for EchoOut {
    fn name(&self) -> &'static str {
        "echo_out"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let beat_seconds = bar_seconds / 4.0;
        let l = ctx.plan.length_bars as f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::EchoMix {
                    deck: DeckRole::Outgoing,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l / 2.0),
                    curve: Curve::Linear,
                },
            ],
            outgoing_init: Some(DeckInitConfig {
                echo_delay_seconds: Some(beat_seconds),
                echo_feedback: Some(0.5),
                loop_bars: None,
                loop_bar_seconds: None,
            }),
            incoming_init: None,
        }
    }
}
```

- [ ] **Step 9: Migrate `LoopOut`**

Replace `impl Recipe for LoopOut` with:

```rust
impl Recipe for LoopOut {
    fn name(&self) -> &'static str {
        "loop_out"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let l = ctx.plan.length_bars as f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::LoopGate {
                    deck: DeckRole::Outgoing,
                    on_at: bar(0.0),
                    off_at: bar(l),
                },
            ],
            outgoing_init: Some(DeckInitConfig {
                echo_delay_seconds: None,
                echo_feedback: None,
                loop_bars: Some(4),
                loop_bar_seconds: Some(bar_seconds),
            }),
            incoming_init: None,
        }
    }
}
```

- [ ] **Step 10: Build**

Run: `cargo build --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml`

Expected: builds. Address any compile errors (likely unused imports — clean them up).

- [ ] **Step 11: Run the full test suite to confirm no behavioral drift**

Run: `cargo test --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml`

Expected:
- `compile_test`: all pass.
- `recipe_test`: all existing tests pass. If one or two fail with a 1-frame drift on `t_start_frames` or `t_end_frames`, that's the documented acceptable rounding sliding. Update the test's expected frame to match the new value. If a test fails with a larger drift or a missing automation, the migration mapping is wrong — fix the recipe.
- `mixer_integration_test`, `scheduler_test`, `automation_test`, `eq_test`, `effects_test`, `crossfade_test`, `phrase_test`, `transition_test`, `deck_test`, `recipe_recency_test`, `library_test`, `selector_test`, `sidecar_test`, `recency_test`, `camelot_test`, `align_test`: all pass.

If any non-trivial failures occur, stop and report.

- [ ] **Step 12: Commit**

```bash
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe.rs engine/tests/
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "refactor(engine): migrate 7 recipes to TransitionScript / build_script

Recipe trait gains build_script(ctx) -> TransitionScript; build now
defaults to compile(build_script(ctx), ctx). All recipes rewritten to
emit phase-DSL scripts. RecipeOutput is produced via the trait's
default build, so call sites and existing integration tests are
unchanged. Any 1-frame drift updates absorbed into test expected
values."
```

---

### Task 7: Add `ThreeBandSwap` recipe and its phase-level test

**Files:**
- Modify: `engine/src/recipe.rs` (add a new `pub struct ThreeBandSwap` + `impl Recipe`).
- Create: `engine/tests/script_test.rs`

- [ ] **Step 1: Add the recipe struct and impl to recipe.rs**

Append to `engine/src/recipe.rs` (after `LoopOut`'s impl, before `RecipeChoice`):

```rust
/// `ThreeBandSwap`: staged EQ swap on integer bar positions.
/// - Incoming highs ramp in over bars [0, L/8].
/// - Mids cross in the middle quarter (bars [3L/8, 5L/8]).
/// - Bass swaps on a single downbeat at bar 7L/8.
/// Equal-power fader across the whole window. For L=16 the bass swap
/// lands on bar 14; for L=32 on bar 28; for L=8 on bar 7.
pub struct ThreeBandSwap;

impl Recipe for ThreeBandSwap {
    fn name(&self) -> &'static str {
        "three_band_swap"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = ctx.plan.length_bars as f64;
        let swap_at = 7.0 * l / 8.0;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                // Highs first.
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::High,
                    value_db: -40.0,
                    at: bar(0.0),
                },
                Phase::EqRamp {
                    deck: DeckRole::Incoming,
                    band: EqBand::High,
                    from_db: -40.0,
                    to_db: 0.0,
                    span: bars(0.0, l / 8.0),
                    curve: Curve::Linear,
                },
                // Mids cross in the middle quarter.
                Phase::EqRamp {
                    deck: DeckRole::Outgoing,
                    band: EqBand::Mid,
                    from_db: 0.0,
                    to_db: -40.0,
                    span: bars(3.0 * l / 8.0, 5.0 * l / 8.0),
                    curve: Curve::Linear,
                },
                Phase::EqRamp {
                    deck: DeckRole::Incoming,
                    band: EqBand::Mid,
                    from_db: -40.0,
                    to_db: 0.0,
                    span: bars(3.0 * l / 8.0, 5.0 * l / 8.0),
                    curve: Curve::Linear,
                },
                // Bass swap on a single downbeat near the end.
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::Low,
                    value_db: -40.0,
                    at: bar(0.0),
                },
                Phase::EqStep {
                    deck: DeckRole::Outgoing,
                    band: EqBand::Low,
                    value_db: -40.0,
                    at: bar(swap_at),
                },
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::Low,
                    value_db: 0.0,
                    at: bar(swap_at),
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}
```

- [ ] **Step 2: Create the phase-level test**

Create `engine/tests/script_test.rs`:

```rust
use dj_engine::automation::Curve;
use dj_engine::library::LoadedTrack;
use dj_engine::recipe::{
    DeckId, Recipe, ThreeBandSwap, TransitionContext,
};
use dj_engine::recipe_script::{DeckRole, EqBand, Phase};
use dj_engine::sidecar::Sidecar;
use dj_engine::transition::TransitionPlan;
use std::path::PathBuf;

const SR: u32 = 44100;

fn sidecar() -> Sidecar {
    Sidecar {
        schema_version: 1,
        analyzer_version: "test".into(),
        source_path: "test.mp3".into(),
        source_mtime: 0.0,
        source_size_bytes: 0,
        duration_seconds: Some(300.0),
        sample_rate: Some(SR),
        bpm: Some(120.0),
        bpm_confidence: Some(1.0),
        beats_seconds: vec![],
        downbeats_seconds: vec![],
        key: None,
        key_label: None,
        key_confidence: None,
        energy_curve: vec![],
        energy_mean: Some(0.5),
        band_energy_means: None,
        cue_in_seconds: Some(0.0),
        cue_out_seconds: Some(280.0),
        intro_end_seconds: Some(16.0),
        outro_start_seconds: Some(200.0),
        is_mixable: Some(true),
        mixable_reasons: vec![],
        error: None,
        phrase_boundaries_seconds: None,
        phrase_confidence: None,
        band_energy_curves: None,
        is_beatmatchable: None,
        is_phrase_aligned: None,
    }
}

fn track() -> LoadedTrack {
    LoadedTrack {
        sidecar_path: PathBuf::new(),
        sidecar: sidecar(),
    }
}

fn plan(length_bars: usize) -> TransitionPlan {
    TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars,
        use_eq_swap: false,
    }
}

#[test]
fn three_band_swap_16_bar_emits_expected_phase_shape() {
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
    let script = ThreeBandSwap.build_script(&ctx);
    assert_eq!(script.length_bars, 16);
    assert!(script.outgoing_init.is_none());
    assert!(script.incoming_init.is_none());

    // Expect: 10 phases (TempoAlign + 2 Fader + 1 EqStep highs-init + 1 EqRamp highs-in
    //         + 2 EqRamp mids-cross + 1 EqStep low-init + 2 EqStep bass-swap).
    assert_eq!(script.phases.len(), 10, "phase count");

    // Spot-check key phases.
    let has_high_init = script.phases.iter().any(|p| matches!(p,
        Phase::EqStep { deck: DeckRole::Incoming, band: EqBand::High, value_db, at }
            if (*value_db - (-40.0)).abs() < 1e-9 && (at.bars - 0.0).abs() < 1e-9));
    assert!(has_high_init, "highs init step at bar 0");

    let has_mids_outgoing = script.phases.iter().any(|p| matches!(p,
        Phase::EqRamp { deck: DeckRole::Outgoing, band: EqBand::Mid, from_db, to_db, span, .. }
            if (*from_db - 0.0).abs() < 1e-9 && (*to_db - (-40.0)).abs() < 1e-9
               && (span.start.bars - 6.0).abs() < 1e-9 && (span.end.bars - 10.0).abs() < 1e-9));
    assert!(has_mids_outgoing, "outgoing mids ramp on [6, 10]");

    let has_bass_swap_outgoing = script.phases.iter().any(|p| matches!(p,
        Phase::EqStep { deck: DeckRole::Outgoing, band: EqBand::Low, value_db, at }
            if (*value_db - (-40.0)).abs() < 1e-9 && (at.bars - 14.0).abs() < 1e-9));
    assert!(has_bass_swap_outgoing, "outgoing bass-kill at bar 14");

    let has_bass_swap_incoming = script.phases.iter().any(|p| matches!(p,
        Phase::EqStep { deck: DeckRole::Incoming, band: EqBand::Low, value_db, at }
            if (*value_db - 0.0).abs() < 1e-9 && (at.bars - 14.0).abs() < 1e-9));
    assert!(has_bass_swap_incoming, "incoming bass-release at bar 14");
}

#[test]
fn three_band_swap_8_bar_swap_lands_at_bar_7() {
    let outgoing = track();
    let incoming = track();
    let p = plan(8);
    let ctx = TransitionContext {
        outgoing: &outgoing,
        incoming: &incoming,
        plan: &p,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A,
        incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let script = ThreeBandSwap.build_script(&ctx);

    let has_bass_swap = script.phases.iter().any(|p| matches!(p,
        Phase::EqStep { deck: DeckRole::Outgoing, band: EqBand::Low, value_db, at }
            if (*value_db - (-40.0)).abs() < 1e-9 && (at.bars - 7.0).abs() < 1e-9));
    assert!(has_bass_swap, "outgoing bass-kill at bar 7 for L=8");
}
```

- [ ] **Step 3: Build and run the new test**

Run:
```
cargo test --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml --test script_test
```

Expected: both tests pass.

- [ ] **Step 4: Commit**

```bash
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe.rs engine/tests/script_test.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): ThreeBandSwap recipe (staged highs/mids/bass on downbeats)"
```

---

### Task 8: Add `EchoThrow` recipe and its phase-level test

**Files:**
- Modify: `engine/src/recipe.rs` (add `pub struct EchoThrow` + impl).
- Modify: `engine/tests/script_test.rs` (append a test).

- [ ] **Step 1: Add the recipe**

Append to `engine/src/recipe.rs` (after `ThreeBandSwap`):

```rust
/// `EchoThrow`: equal-power crossfade with a 1-bar one-shot echo flourish
/// on the outgoing deck at bar L-2. Echo delay is 1 beat at the outgoing
/// tempo; feedback is 0.75. The throw fires when the outgoing fader is
/// already near -6 dB, so the tail rings into the incoming.
pub struct EchoThrow;

impl Recipe for EchoThrow {
    fn name(&self) -> &'static str {
        "echo_throw"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let beat_seconds = 60.0 / outgoing_bpm;
        let l = ctx.plan.length_bars as f64;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerIn,
                },
                Phase::EchoThrow {
                    deck: DeckRole::Outgoing,
                    at: bar((l - 2.0).max(0.0)),
                    hold_bars: 1.0,
                },
            ],
            outgoing_init: Some(DeckInitConfig {
                echo_delay_seconds: Some(beat_seconds),
                echo_feedback: Some(0.75),
                loop_bars: None,
                loop_bar_seconds: None,
            }),
            incoming_init: None,
        }
    }
}
```

- [ ] **Step 2: Add the test**

Append to `engine/tests/script_test.rs`:

```rust
#[test]
fn echo_throw_16_bar_emits_throw_at_bar_14() {
    use dj_engine::recipe::EchoThrow;
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
    let script = EchoThrow.build_script(&ctx);
    assert_eq!(script.length_bars, 16);

    let init = script.outgoing_init.expect("outgoing_init for echo");
    // 120 BPM → 0.5s per beat.
    assert!((init.echo_delay_seconds.unwrap() - 0.5).abs() < 1e-9);
    assert!((init.echo_feedback.unwrap() - 0.75).abs() < 1e-9);

    let has_throw = script.phases.iter().any(|p| matches!(p,
        Phase::EchoThrow { deck: DeckRole::Outgoing, at, hold_bars }
            if (at.bars - 14.0).abs() < 1e-9 && (*hold_bars - 1.0).abs() < 1e-9));
    assert!(has_throw, "echo throw at bar 14 holding 1 bar");
}
```

- [ ] **Step 3: Build and run**

Run:
```
cargo test --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml --test script_test
```

Expected: all 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe.rs engine/tests/script_test.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): EchoThrow recipe (1-bar one-shot echo flourish on exit)"
```

---

### Task 9: Add `HighsFirst` recipe and its phase-level test

**Files:**
- Modify: `engine/src/recipe.rs` (add `pub struct HighsFirst` + impl).
- Modify: `engine/tests/script_test.rs` (append).

- [ ] **Step 1: Add the recipe**

Append to `engine/src/recipe.rs` (after `EchoThrow`):

```rust
/// `HighsFirst`: incoming intro with bass and mids killed at start.
/// - Incoming fader rises fast (equal-power over bars [0, L/8]).
/// - Mids fade in over bars [L/4, L/2].
/// - Synchronized bass swap on bar 3L/4 (outgoing low → -40dB, incoming low → 0dB).
/// Used when incoming is markedly higher energy than outgoing.
pub struct HighsFirst;

impl Recipe for HighsFirst {
    fn name(&self) -> &'static str {
        "highs_first"
    }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = ctx.plan.length_bars as f64;
        let swap_at = 3.0 * l / 4.0;

        TransitionScript {
            length_bars: ctx.plan.length_bars,
            phases: vec![
                Phase::TempoAlign {
                    deck: DeckRole::Incoming,
                    ratio,
                },
                Phase::Fader {
                    deck: DeckRole::Outgoing,
                    from: 1.0,
                    to: 0.0,
                    span: bars(0.0, l),
                    curve: Curve::EqualPowerOut,
                },
                Phase::Fader {
                    deck: DeckRole::Incoming,
                    from: 0.0,
                    to: 1.0,
                    span: bars(0.0, l / 8.0),
                    curve: Curve::EqualPowerIn,
                },
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::Low,
                    value_db: -40.0,
                    at: bar(0.0),
                },
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::Mid,
                    value_db: -40.0,
                    at: bar(0.0),
                },
                Phase::EqRamp {
                    deck: DeckRole::Incoming,
                    band: EqBand::Mid,
                    from_db: -40.0,
                    to_db: 0.0,
                    span: bars(l / 4.0, l / 2.0),
                    curve: Curve::Linear,
                },
                Phase::EqStep {
                    deck: DeckRole::Outgoing,
                    band: EqBand::Low,
                    value_db: -40.0,
                    at: bar(swap_at),
                },
                Phase::EqStep {
                    deck: DeckRole::Incoming,
                    band: EqBand::Low,
                    value_db: 0.0,
                    at: bar(swap_at),
                },
            ],
            outgoing_init: None,
            incoming_init: None,
        }
    }
}
```

- [ ] **Step 2: Add the test**

Append to `engine/tests/script_test.rs`:

```rust
#[test]
fn highs_first_16_bar_bass_swap_at_bar_12() {
    use dj_engine::recipe::HighsFirst;
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
    let script = HighsFirst.build_script(&ctx);
    assert_eq!(script.length_bars, 16);

    // Incoming fader fast-rise over [0, 2].
    let has_fast_fader = script.phases.iter().any(|p| matches!(p,
        Phase::Fader { deck: DeckRole::Incoming, from, to, span, curve: Curve::EqualPowerIn }
            if (*from - 0.0).abs() < 1e-9 && (*to - 1.0).abs() < 1e-9
               && (span.start.bars - 0.0).abs() < 1e-9 && (span.end.bars - 2.0).abs() < 1e-9));
    assert!(has_fast_fader, "incoming fader fast-rise on [0, 2]");

    // Bass swap on bar 12.
    let has_bass_swap_out = script.phases.iter().any(|p| matches!(p,
        Phase::EqStep { deck: DeckRole::Outgoing, band: EqBand::Low, value_db, at }
            if (*value_db - (-40.0)).abs() < 1e-9 && (at.bars - 12.0).abs() < 1e-9));
    let has_bass_swap_in = script.phases.iter().any(|p| matches!(p,
        Phase::EqStep { deck: DeckRole::Incoming, band: EqBand::Low, value_db, at }
            if (*value_db - 0.0).abs() < 1e-9 && (at.bars - 12.0).abs() < 1e-9));
    assert!(has_bass_swap_out && has_bass_swap_in, "bass swap at bar 12");

    // Mids ramp on [4, 8].
    let has_mids_ramp = script.phases.iter().any(|p| matches!(p,
        Phase::EqRamp { deck: DeckRole::Incoming, band: EqBand::Mid, from_db, to_db, span, .. }
            if (*from_db - (-40.0)).abs() < 1e-9 && (*to_db - 0.0).abs() < 1e-9
               && (span.start.bars - 4.0).abs() < 1e-9 && (span.end.bars - 8.0).abs() < 1e-9));
    assert!(has_mids_ramp, "mids ramp on [4, 8]");
}
```

- [ ] **Step 3: Build and run**

Run:
```
cargo test --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml --test script_test
```

Expected: all 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe.rs engine/tests/script_test.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): HighsFirst recipe (incoming layered build-in)"
```

---

### Task 10: Extend `RecipeChoice` with 3 new variants and `score_recipe` with 3 branches

**Files:**
- Modify: `engine/src/recipe.rs` (enum, score function, candidates array, recipe_choice_id, recency_hash).

- [ ] **Step 1: Add new variants**

Find:

```rust
pub enum RecipeChoice {
    EqualPower,
    HardCut,
    BassSwap,
    FilterFade,
    FilterKill,
    EchoOut,
    LoopOut,
}
```

Replace with:

```rust
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
}
```

- [ ] **Step 2: Update `recipe_choice_id`**

Find `fn recipe_choice_id` and replace with:

```rust
fn recipe_choice_id(c: RecipeChoice) -> u64 {
    match c {
        RecipeChoice::EqualPower => 1,
        RecipeChoice::HardCut => 2,
        RecipeChoice::BassSwap => 3,
        RecipeChoice::FilterFade => 4,
        RecipeChoice::FilterKill => 5,
        RecipeChoice::EchoOut => 6,
        RecipeChoice::LoopOut => 7,
        RecipeChoice::ThreeBandSwap => 8,
        RecipeChoice::EchoThrow => 9,
        RecipeChoice::HighsFirst => 10,
    }
}
```

- [ ] **Step 3: Update `recency_hash`**

Find `fn recency_hash` and replace its variant list with:

```rust
fn recency_hash(recency: &crate::recipe_recency::RecipeRecency) -> u64 {
    let mut h: u64 = 0x9e3779b97f4a7c15;
    for c in [
        RecipeChoice::EqualPower,
        RecipeChoice::HardCut,
        RecipeChoice::BassSwap,
        RecipeChoice::FilterFade,
        RecipeChoice::FilterKill,
        RecipeChoice::EchoOut,
        RecipeChoice::LoopOut,
        RecipeChoice::ThreeBandSwap,
        RecipeChoice::EchoThrow,
        RecipeChoice::HighsFirst,
    ] {
        let bit = if recency.contains(c) { 1 } else { 0 };
        h = h
            .wrapping_mul(0x100000001b3)
            .wrapping_add(recipe_choice_id(c) * 31 + bit);
    }
    h
}
```

- [ ] **Step 4: Update `pick_recipe`'s candidates array**

Find the `let candidates = [...];` line inside `pick_recipe` and replace with:

```rust
    let candidates = [
        RecipeChoice::EqualPower,
        RecipeChoice::BassSwap,
        RecipeChoice::FilterFade,
        RecipeChoice::FilterKill,
        RecipeChoice::EchoOut,
        RecipeChoice::LoopOut,
        RecipeChoice::ThreeBandSwap,
        RecipeChoice::EchoThrow,
        RecipeChoice::HighsFirst,
    ];
```

- [ ] **Step 5: Add new branches to `score_recipe`**

Find `pub fn score_recipe(...)` and add three new branches inside the `match` (between `RecipeChoice::LoopOut => 0.30,` and the closing `}`):

```rust
        RecipeChoice::ThreeBandSwap => {
            let both_heavy =
                inputs.outgoing_bass > 0.5 && inputs.incoming_bass > 0.5;
            let any_heavy =
                inputs.outgoing_bass > 0.5 || inputs.incoming_bass > 0.5;
            let long_enough = inputs.length_bars >= 16;
            if inputs.phrase_aligned && both_heavy && long_enough {
                0.90
            } else if inputs.phrase_aligned && both_heavy {
                0.70
            } else if inputs.phrase_aligned && any_heavy {
                0.50
            } else {
                0.20
            }
        }
        RecipeChoice::EchoThrow => {
            let smooth =
                inputs.energy_delta.abs() < 0.10 && inputs.length_bars >= 16;
            let mild =
                inputs.energy_delta.abs() < 0.15 && inputs.length_bars >= 8;
            if smooth {
                0.65
            } else if mild {
                0.55
            } else {
                0.20
            }
        }
        RecipeChoice::HighsFirst => {
            let big_ramp = inputs.energy_delta > 0.20
                && inputs.phrase_aligned
                && inputs.length_bars >= 16;
            let med_ramp =
                inputs.energy_delta > 0.10 && inputs.phrase_aligned;
            let any_ramp = inputs.energy_delta > 0.05;
            if big_ramp {
                0.85
            } else if med_ramp {
                0.55
            } else if any_ramp {
                0.30
            } else {
                0.10
            }
        }
```

- [ ] **Step 6: Build**

Run: `cargo build --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml`

Expected: success. If any match-exhaustiveness warning fires elsewhere (e.g., in `main.rs`'s dispatch on `RecipeChoice`), find it and add three new arms calling `ThreeBandSwap.build(&ctx)`, `EchoThrow.build(&ctx)`, `HighsFirst.build(&ctx)`. Most likely main.rs has a `match choice { ... }` dispatch — extend it.

- [ ] **Step 7: Add scoring tests to recipe_test.rs**

Append to `engine/tests/recipe_test.rs`:

```rust
#[test]
fn score_three_band_swap_both_heavy_phrase_aligned_long() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.0,
        outgoing_bass: 0.7,
        incoming_bass: 0.6,
        phrase_aligned: true,
    };
    assert!((score_recipe(RecipeChoice::ThreeBandSwap, &inputs) - 0.90).abs() < 1e-9);
}

#[test]
fn score_three_band_swap_both_heavy_short_window() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 8,
        use_eq_swap: true,
        energy_delta: 0.0,
        outgoing_bass: 0.7,
        incoming_bass: 0.6,
        phrase_aligned: true,
    };
    assert!((score_recipe(RecipeChoice::ThreeBandSwap, &inputs) - 0.70).abs() < 1e-9);
}

#[test]
fn score_echo_throw_smooth_long() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: false,
        energy_delta: 0.05,
        outgoing_bass: 0.4,
        incoming_bass: 0.4,
        phrase_aligned: true,
    };
    assert!((score_recipe(RecipeChoice::EchoThrow, &inputs) - 0.65).abs() < 1e-9);
}

#[test]
fn score_highs_first_big_ramp_long() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: false,
        energy_delta: 0.30,
        outgoing_bass: 0.4,
        incoming_bass: 0.4,
        phrase_aligned: true,
    };
    assert!((score_recipe(RecipeChoice::HighsFirst, &inputs) - 0.85).abs() < 1e-9);
}

#[test]
fn score_highs_first_no_ramp_low_score() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: false,
        energy_delta: 0.0,
        outgoing_bass: 0.4,
        incoming_bass: 0.4,
        phrase_aligned: true,
    };
    assert!((score_recipe(RecipeChoice::HighsFirst, &inputs) - 0.10).abs() < 1e-9);
}
```

- [ ] **Step 8: Run test suite**

Run: `cargo test --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml`

Expected: all tests pass, including the 5 new scoring tests.

- [ ] **Step 9: Commit**

```bash
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe.rs engine/tests/recipe_test.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): RecipeChoice + score_recipe gain 3 new recipes"
```

---

### Task 11: Extend `--force-recipe` CLI to accept the 3 new values + wire main.rs dispatch

**Files:**
- Modify: `engine/src/config.rs` (the `ForceRecipe` enum / its parse impl).
- Modify: `engine/src/main.rs` (if not already updated in Task 10's main.rs match fix).

- [ ] **Step 1: Read config.rs around the force-recipe area**

Run:
```
Grep for "force_recipe" or "ForceRecipe" in engine/src/config.rs and engine/src/main.rs.
```

If `--force-recipe` is parsed as a string and converted to `RecipeChoice` via a helper (e.g., `to_recipe_choice` or a `FromStr` impl), find that helper and extend it. Likely candidates:
- A `#[derive(clap::ValueEnum)]` enum with variants matching recipe names.
- A `match` on a `String` argument.

Identify the actual structure by reading the file.

- [ ] **Step 2: Add the three new values**

Extend whichever pattern is in place. If it's a clap `ValueEnum`:

```rust
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum ForceRecipe {
    EqualPower,
    HardCut,
    BassSwap,
    FilterFade,
    FilterKill,
    EchoOut,
    LoopOut,
    ThreeBandSwap,   // NEW
    EchoThrow,       // NEW
    HighsFirst,      // NEW
}

impl ForceRecipe {
    pub fn to_recipe_choice(self) -> RecipeChoice {
        match self {
            ForceRecipe::EqualPower => RecipeChoice::EqualPower,
            ForceRecipe::HardCut => RecipeChoice::HardCut,
            ForceRecipe::BassSwap => RecipeChoice::BassSwap,
            ForceRecipe::FilterFade => RecipeChoice::FilterFade,
            ForceRecipe::FilterKill => RecipeChoice::FilterKill,
            ForceRecipe::EchoOut => RecipeChoice::EchoOut,
            ForceRecipe::LoopOut => RecipeChoice::LoopOut,
            ForceRecipe::ThreeBandSwap => RecipeChoice::ThreeBandSwap,
            ForceRecipe::EchoThrow => RecipeChoice::EchoThrow,
            ForceRecipe::HighsFirst => RecipeChoice::HighsFirst,
        }
    }
}
```

clap turns each variant into a kebab-case CLI value automatically: `three-band-swap`, `echo-throw`, `highs-first`. (If the spec's snake_case values are required, use `#[value(name = "three_band_swap")]` per variant.)

- [ ] **Step 3: Wire main.rs dispatch on RecipeChoice**

Find the `match choice { ... }` block in `engine/src/main.rs` (the place that calls `<Recipe>.build(&ctx)`). Add three arms:

```rust
RecipeChoice::ThreeBandSwap => dj_engine::recipe::ThreeBandSwap.build(&ctx),
RecipeChoice::EchoThrow => dj_engine::recipe::EchoThrow.build(&ctx),
RecipeChoice::HighsFirst => dj_engine::recipe::HighsFirst.build(&ctx),
```

- [ ] **Step 4: Build**

Run: `cargo build --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml`

Expected: clean build. If a match-exhaustiveness warning fires anywhere else (e.g., in scheduler.rs or supervisor.rs), follow it and add the three arms there.

- [ ] **Step 5: Smoke-test --force-recipe with one new value**

The DJ-Engine doesn't have a hello-world target; the simplest smoke is to ensure the CLI's --help renders. Run:

```
cargo run --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml -- --help
```

Expected: --help text shows `--force-recipe` accepts the new values among its possible values.

If running --help is intractable (large build), skip and rely on the test sweep below.

- [ ] **Step 6: Run the full test suite**

Run: `cargo test --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml`

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/config.rs engine/src/main.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): --force-recipe accepts 3 new recipes + main.rs dispatch"
```

---

### Task 12: Final cargo fmt + clippy + full sweep

**Files:** all touched.

- [ ] **Step 1: cargo fmt**

Run: `cargo fmt --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml`

Expected: silent success.

- [ ] **Step 2: cargo clippy**

Run:
```
cargo clippy --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml --all-targets -- -D warnings
```

Expected: no warnings. If any fire (e.g., `clippy::needless_borrow`, `clippy::large_enum_variant`), fix them at the source.

- [ ] **Step 3: cargo test --all**

Run: `cargo test --manifest-path C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/engine/Cargo.toml`

Expected: all tests across all suites pass.

- [ ] **Step 4: Commit (if fmt/clippy made changes)**

```bash
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine status
```

If there are tracked changes:

```bash
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add -u
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "chore(engine): fmt + clippy after phase composition"
```

If no changes, skip this step.

---

## Self-review notes

- **Spec coverage:** Every section of the spec has a task:
  - DSL types → Task 3.
  - Serde derives → Tasks 1, 2, 3.
  - `compile()` → Tasks 4 + 5.
  - `Recipe::build_script` trait change + 7-recipe migration → Task 6.
  - 3 new recipes → Tasks 7, 8, 9.
  - Picker + scoring → Task 10.
  - CLI `--force-recipe` → Task 11.
  - Testing → Tasks 4, 5, 7, 8, 9, 10 each carry their own tests.
- **Placeholders:** None — every step contains actual code or an exact command.
- **Type consistency:** `build_script` / `build` / `compile` / `TransitionScript` / `Phase` / `BarPos` / `BarSpan` / `DeckRole` / `EqBand` / `bar()` / `bars()` are used consistently across all tasks.
- **Trait shape refinement vs. spec:** The spec says `Recipe::build` returns `TransitionScript`; the plan refines that to `Recipe::build_script` (the method recipes implement) + a default `build` that compiles. The visible architectural goal (recipes express in bar-time, compile in one place) is identical; this avoids churning existing tests/call sites.
