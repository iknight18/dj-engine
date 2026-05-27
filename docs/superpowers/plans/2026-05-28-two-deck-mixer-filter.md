# One-Knob Filter + FilterFade/FilterKill Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a per-deck one-knob filter (LP sweep when knob < 0, HP sweep when knob > 0, bypass near 0), expose it as `AutomationTarget::Filter`, and add two new recipes (`FilterFade`, `FilterKill`) plus a `--force-recipe` CLI flag for manual recipe testing.

**Architecture:** One `Biquad` per channel per deck, recoefficient'd whenever `state.filter_knob` changes (with a small dead-zone around 0 where the biquad is bypassed entirely in the per-sample loop). Log-Hz mapping from knob position to cutoff frequency. Filter sits between the EQ chain and the fader in the signal path.

**Tech Stack:** Rust 2021, existing `Biquad` with `lowpass`/`highpass` + `set_cutoff` in `engine/src/eq.rs`, `clap` for the CLI flag.

**Spec:** `docs/superpowers/specs/2026-05-28-two-deck-mixer-filter-design.md`

**Working branch:** `research/two-deck-mixer` (continues from sub-project 2).

---

## File structure

| Path | Action | Responsibility |
|---|---|---|
| `engine/src/automation.rs` | MODIFY | Add `Filter` variant to `AutomationTarget`; clamp `[-1, +1]` in `apply_value`. |
| `engine/src/deck.rs` | MODIFY | Add `filter_knob` to `DeckState`, 2 biquads + cache to `Deck`, init/reset in `new/cue/unload`, filter step in `pull`. |
| `engine/src/recipe.rs` | MODIFY | Add `FilterFade`, `FilterKill` recipes. Add `RecipeChoice::FilterFade`, `RecipeChoice::FilterKill`. |
| `engine/src/config.rs` | MODIFY | Add `ForceRecipe` enum + `Cli.force_recipe` field. |
| `engine/src/main.rs` | MODIFY | Apply `force_recipe` override, add arm-match arms for new recipes, log override. |
| `engine/tests/automation_test.rs` | MODIFY | `Filter` apply + clamp tests. |
| `engine/tests/deck_test.rs` | MODIFY | `cue` resets knob; `Filter Step +1` attenuates 100 Hz vs 5 kHz sine. |
| `engine/tests/recipe_test.rs` | MODIFY | `FilterFade::build` and `FilterKill::build` tests. |
| `engine/tests/mixer_integration_test.rs` | MODIFY | Continuous-audio integration tests for `FilterFade` and `FilterKill`. |

---

## Tasks

### Task 1: `AutomationTarget::Filter`

**Files:**
- Modify: `engine/src/automation.rs`
- Modify: `engine/tests/automation_test.rs`

- [ ] **Step 1: Write the failing tests**

Append to `engine/tests/automation_test.rs`:

```rust
#[test]
fn filter_step_installs_to_value() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::Filter, 0.0, 1.0, Curve::Step, 0, 1));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.filter_knob - 1.0).abs() < 1e-9);
}

#[test]
fn filter_clamped_above_plus_1() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::Filter, 0.0, 5.0, Curve::Step, 0, 1));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.filter_knob - 1.0).abs() < 1e-9);
}

#[test]
fn filter_clamped_below_minus_1() {
    let mut q = AutomationQueue::new();
    q.schedule(ramp(AutomationTarget::Filter, 0.0, -5.0, Curve::Step, 0, 1));
    let mut s = default_state();
    q.step(1, &mut s);
    assert!((s.filter_knob - (-1.0)).abs() < 1e-9);
}

#[test]
fn filter_linear_midpoint_negative() {
    let a = ramp(AutomationTarget::Filter, 1.0, -1.0, Curve::Linear, 0, 100);
    let v = a.sample_value(50).unwrap();
    assert!((v - 0.0).abs() < 1e-9, "got {v}");
}
```

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test automation_test`
Expected: FAIL — `AutomationTarget::Filter` and `DeckState.filter_knob` don't exist.

- [ ] **Step 3: Add the variant and apply branch**

Edit `engine/src/automation.rs`. Replace the `AutomationTarget` enum:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomationTarget {
    Fader,
    Tempo,
    EqLow,
    EqMid,
    EqHigh,
    Filter,
}
```

Add the `Filter` branch in `apply_value`:

```rust
AutomationTarget::Filter => {
    state.filter_knob = v.clamp(-1.0, 1.0);
}
```

(Compilation fails until Task 2 adds the field. Continue.)

- [ ] **Step 4: Commit alongside Task 2 — see Task 2's commit step.**

---

### Task 2: `DeckState.filter_knob` + `Deck` biquads + reset

**Files:**
- Modify: `engine/src/deck.rs`
- Modify: `engine/tests/deck_test.rs`

- [ ] **Step 1: Add the field, constants, helpers, biquads, and resets**

Edit `engine/src/deck.rs`. Replace the `DeckState` struct (append `filter_knob` field after the EQ ones):

```rust
#[derive(Clone, Debug)]
pub struct DeckState {
    pub fader: f32,
    pub tempo: f64,
    pub eq_low_db: f64,
    pub eq_mid_db: f64,
    pub eq_high_db: f64,
    pub filter_knob: f64,
    pub source_path: Option<String>,
    pub play_state: PlayState,
}

impl Default for DeckState {
    fn default() -> Self {
        Self {
            fader: 0.0,
            tempo: 1.0,
            eq_low_db: 0.0,
            eq_mid_db: 0.0,
            eq_high_db: 0.0,
            filter_knob: 0.0,
            source_path: None,
            play_state: PlayState::Stopped,
        }
    }
}
```

Add the filter constants near the EQ constants:

```rust
const FILTER_LP_FREQ_BYPASS:    f64 = 22_000.0;
const FILTER_LP_FREQ_MAX_LEFT:  f64 =    200.0;
const FILTER_HP_FREQ_BYPASS:    f64 =     10.0;
const FILTER_HP_FREQ_MAX_RIGHT: f64 =  8_000.0;
const FILTER_BYPASS_EPSILON:    f64 =    0.01;
```

Add helper functions in `deck.rs` (module-private, outside the impl block):

```rust
fn filter_cutoff_hz(knob: f64) -> (crate::eq::FilterKind, f64) {
    if knob >= 0.0 {
        (
            crate::eq::FilterKind::HighPass,
            log_lerp(FILTER_HP_FREQ_BYPASS, FILTER_HP_FREQ_MAX_RIGHT, knob),
        )
    } else {
        (
            crate::eq::FilterKind::LowPass,
            log_lerp(FILTER_LP_FREQ_BYPASS, FILTER_LP_FREQ_MAX_LEFT, -knob),
        )
    }
}

fn log_lerp(from: f64, to: f64, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    (from.ln() + (to.ln() - from.ln()) * t).exp()
}
```

Add to `Deck`'s field list (after the EQ biquads):

```rust
filter_l: crate::eq::Biquad,
filter_r: crate::eq::Biquad,
last_filter_knob: f64,
```

Edit `Deck::new`. After the EQ biquads, add:

```rust
let filter_l = crate::eq::Biquad::lowpass(sr, FILTER_LP_FREQ_BYPASS);
let filter_r = crate::eq::Biquad::lowpass(sr, FILTER_LP_FREQ_BYPASS);
```

And include `filter_l, filter_r, last_filter_knob: 0.0,` in the struct construction.

Edit `Deck::reset_eq` (the helper extracted by sub-project 2) — rename / extend it to `reset_eq_and_filter`, and ALSO reset the filter state. Or add a parallel `reset_filter` helper called alongside. Pick whichever keeps the file diff smaller; both `cue` and `unload` must reset the filter state.

```rust
fn reset_filter(&mut self) {
    self.state.filter_knob = 0.0;
    self.last_filter_knob = 0.0;
    let sr = SAMPLE_RATE as f64;
    self.filter_l = crate::eq::Biquad::lowpass(sr, FILTER_LP_FREQ_BYPASS);
    self.filter_r = crate::eq::Biquad::lowpass(sr, FILTER_LP_FREQ_BYPASS);
}
```

Call `self.reset_filter();` in BOTH `cue()` and `unload()` (after the existing EQ resets).

Add a `recompute_filter_if_dirty` method on `Deck`:

```rust
fn recompute_filter_if_dirty(&mut self) {
    if (self.state.filter_knob - self.last_filter_knob).abs() <= f64::EPSILON {
        return;
    }
    let sr = SAMPLE_RATE as f64;
    if self.state.filter_knob.abs() < FILTER_BYPASS_EPSILON {
        // Inside the dead zone — biquad won't be evaluated; keep
        // coefficients harmless (a passthrough LP at bypass freq).
        self.filter_l.set_cutoff(sr, crate::eq::FilterKind::LowPass, FILTER_LP_FREQ_BYPASS);
        self.filter_r.set_cutoff(sr, crate::eq::FilterKind::LowPass, FILTER_LP_FREQ_BYPASS);
    } else {
        let (kind, cutoff) = filter_cutoff_hz(self.state.filter_knob);
        self.filter_l.set_cutoff(sr, kind, cutoff);
        self.filter_r.set_cutoff(sr, kind, cutoff);
    }
    self.last_filter_knob = self.state.filter_knob;
}
```

Update `Deck::pull`. After the existing `self.recompute_eq_if_dirty();` call, add:

```rust
self.recompute_filter_if_dirty();
let filter_active = self.state.filter_knob.abs() >= FILTER_BYPASS_EPSILON;
```

In the per-sample loop, after applying the EQ chain (`l = eq_high_l.process(eq_mid_l.process(eq_low_l.process(out[li])))`) and BEFORE multiplying by the fader, add the filter step:

```rust
let l = if filter_active { self.filter_l.process(l) } else { l };
let r = if filter_active { self.filter_r.process(r) } else { r };
```

- [ ] **Step 2: Write failing tests for the filter behavior**

Append to `engine/tests/deck_test.rs`:

```rust
#[tokio::test]
async fn filter_step_plus_1_attenuates_low_frequency() {
    // Filter at +1 (full HP at 8 kHz) should kill a 100 Hz sine and pass a 5 kHz sine.
    // We use a 5 kHz sine fixture for this test. The existing fixture is 440 Hz,
    // which falls between the HP knee (8 kHz) and bypass — 440 Hz with HP at 8 kHz
    // is attenuated, just not by 40 dB. The test below uses the existing 440 Hz
    // fixture and asserts a meaningful attenuation, not a brick-wall kill.
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.play();
    d.test_set_fader(1.0);
    d.schedule(Automation {
        target: AutomationTarget::Filter,
        t_start_frames: 0,
        t_end_frames: 1,
        from: 1.0,
        to: 1.0,
        curve: Curve::Step,
    });
    // Skip the first tick (decoder priming + biquad startup transient).
    let _ = d.pull(4096).await.unwrap();
    let filtered = d.pull(8192).await.unwrap();
    let r_filtered = rms_stereo(&filtered);

    let mut d2 = Deck::new(DeckId::A);
    d2.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d2.play();
    d2.test_set_fader(1.0);
    let _ = d2.pull(4096).await.unwrap();
    let baseline = d2.pull(8192).await.unwrap();
    let r_baseline = rms_stereo(&baseline);

    assert!(
        r_filtered < r_baseline * 0.6,
        "HP at 8 kHz should attenuate 440 Hz; filtered {r_filtered} vs baseline {r_baseline}"
    );
    assert!((d.state().filter_knob - 1.0).abs() < 1e-9);
}

#[tokio::test]
async fn filter_bypass_at_zero_passes_through_unchanged() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.play();
    d.test_set_fader(1.0);
    // No filter automation — knob stays at 0.
    let _ = d.pull(4096).await.unwrap();
    let out_filtered = d.pull(8192).await.unwrap();
    let r_filtered = rms_stereo(&out_filtered);

    let mut d2 = Deck::new(DeckId::A);
    d2.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d2.play();
    d2.test_set_fader(1.0);
    let _ = d2.pull(4096).await.unwrap();
    let baseline = d2.pull(8192).await.unwrap();
    let r_baseline = rms_stereo(&baseline);

    // With knob = 0, output should be byte-identical to baseline (no filter applied).
    assert!(
        (r_filtered - r_baseline).abs() < r_baseline * 0.01,
        "knob=0 should be passthrough; got {r_filtered} vs {r_baseline}"
    );
    assert!(d.state().filter_knob.abs() < f64::EPSILON);
}

#[tokio::test]
async fn cue_resets_filter_knob() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.play();
    d.schedule(Automation {
        target: AutomationTarget::Filter,
        t_start_frames: 0,
        t_end_frames: 1,
        from: 0.0,
        to: 1.0,
        curve: Curve::Step,
    });
    let _ = d.pull(4096).await.unwrap();
    assert!((d.state().filter_knob - 1.0).abs() < 1e-9);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    assert!(d.state().filter_knob.abs() < f64::EPSILON);
}
```

- [ ] **Step 3: Verify the tests pass**

Run: `cd engine && cargo test --test automation_test --test deck_test`
Expected: existing tests + 3 new automation tests + 3 new deck tests all pass.

- [ ] **Step 4: Commit Tasks 1 + 2 together**

```
git add engine/src/automation.rs engine/src/deck.rs engine/tests/automation_test.rs engine/tests/deck_test.rs
git commit -m "feat(engine): Filter automation target + per-deck filter biquad (sub-project 3, tasks 1-2)"
```

---

### Task 3: `FilterFade` and `FilterKill` recipes

**Files:**
- Modify: `engine/src/recipe.rs`
- Modify: `engine/tests/recipe_test.rs`

- [ ] **Step 1: Write the failing tests**

Append to `engine/tests/recipe_test.rs`:

```rust
use dj_engine::recipe::{FilterFade, FilterKill};

#[test]
fn filter_fade_emits_fader_pair_plus_incoming_filter_sweep() {
    let outgoing = track(120.0);
    let incoming = track(120.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
    };
    let ctx = ctx_for(&outgoing, &incoming, &plan);
    let r = FilterFade.build(&ctx);
    // Outgoing: just the Fader (no filter on outgoing in FilterFade).
    assert_eq!(r.outgoing_automations.len(), 1);
    assert_eq!(r.outgoing_automations[0].target, AutomationTarget::Fader);
    assert_eq!(r.outgoing_automations[0].curve, Curve::EqualPowerOut);
    // Incoming: Fader, Tempo Step, Filter Step (init at +1), Filter Linear (+1→0).
    assert_eq!(r.incoming_automations.len(), 4);
    let filter_init = r.incoming_automations.iter()
        .find(|a| a.target == AutomationTarget::Filter && a.curve == Curve::Step)
        .expect("incoming filter init");
    assert_eq!(filter_init.from, 1.0);
    assert_eq!(filter_init.to, 1.0);
    let filter_sweep = r.incoming_automations.iter()
        .find(|a| a.target == AutomationTarget::Filter && a.curve == Curve::Linear)
        .expect("incoming filter sweep");
    assert_eq!(filter_sweep.from, 1.0);
    assert_eq!(filter_sweep.to, 0.0);
}

#[test]
fn filter_kill_emits_outgoing_filter_sweep_to_plus_1_over_60_percent() {
    let outgoing = track(120.0);
    let incoming = track(120.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 200.0,
        incoming_crossfade_start_seconds: 16.0,
        length_bars: 16,
        use_eq_swap: false,
    };
    let ctx = ctx_for(&outgoing, &incoming, &plan);
    let r = FilterKill.build(&ctx);
    // Outgoing: Fader + Filter Linear 0→+1 over first 60% of L.
    assert_eq!(r.outgoing_automations.len(), 2);
    let out_filter = r.outgoing_automations.iter()
        .find(|a| a.target == AutomationTarget::Filter && a.curve == Curve::Linear)
        .expect("outgoing filter sweep");
    assert_eq!(out_filter.from, 0.0);
    assert_eq!(out_filter.to, 1.0);
    let bar_seconds = (60.0_f64 / 120.0) * 4.0;
    let l_frames = (plan.length_bars as f64 * bar_seconds * SR as f64) as u64;
    let expected_end = (0.6 * l_frames as f64) as u64;
    assert!(
        (out_filter.t_end_frames as i64 - expected_end as i64).abs() < 100,
        "expected end near {expected_end}, got {}", out_filter.t_end_frames
    );
    // Incoming: Fader + Tempo Step. No filter automation on incoming.
    assert_eq!(r.incoming_automations.len(), 2);
}
```

- [ ] **Step 2: Verify failure**

Run: `cd engine && cargo test --test recipe_test`
Expected: FAIL — `FilterFade` and `FilterKill` don't exist.

- [ ] **Step 3: Implement the recipes**

Append to `engine/src/recipe.rs`:

```rust
/// `FilterFade`: incoming track filters in from the top (treble first).
/// outgoing.Fader equal-power fade-out; incoming.Fader equal-power fade-in;
/// incoming.Filter sweeps from +1.0 (full HP, treble only) to 0.0 (bypass).
pub struct FilterFade;

impl Recipe for FilterFade {
    fn name(&self) -> &'static str { "filter_fade" }

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
        let incoming_filter_init = Automation {
            target: AutomationTarget::Filter,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: 1.0,
            to: 1.0,
            curve: Curve::Step,
        };
        let incoming_filter_sweep = Automation {
            target: AutomationTarget::Filter,
            t_start_frames: t0,
            t_end_frames: t1,
            from: 1.0,
            to: 0.0,
            curve: Curve::Linear,
        };

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader],
            incoming_automations: vec![
                incoming_fader,
                incoming_tempo_step,
                incoming_filter_init,
                incoming_filter_sweep,
            ],
            total_duration_frames: length_frames,
        }
    }
}

/// `FilterKill`: outgoing filters out to silence (HP sweep) before fading.
/// outgoing.Filter sweeps 0 → +1 over the first 60% of the crossfade;
/// outgoing.Fader equal-power fade-out over the full crossfade;
/// incoming.Fader equal-power fade-in over the full crossfade (no filter).
pub struct FilterKill;

impl Recipe for FilterKill {
    fn name(&self) -> &'static str { "filter_kill" }

    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let sr = ctx.sample_rate as f64;
        let length_frames = (ctx.plan.length_bars as f64 * bar_seconds * sr) as u64;
        let t0 = ctx.master_clock_frames;
        let t1 = t0 + length_frames;
        let filter_end = t0 + ((length_frames as f64) * 0.6) as u64;

        let outgoing_fader = Automation {
            target: AutomationTarget::Fader,
            t_start_frames: t0,
            t_end_frames: t1,
            from: 1.0,
            to: 0.0,
            curve: Curve::EqualPowerOut,
        };
        let outgoing_filter_sweep = Automation {
            target: AutomationTarget::Filter,
            t_start_frames: t0,
            t_end_frames: filter_end,
            from: 0.0,
            to: 1.0,
            curve: Curve::Linear,
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

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader, outgoing_filter_sweep],
            incoming_automations: vec![incoming_fader, incoming_tempo_step],
            total_duration_frames: length_frames,
        }
    }
}
```

Extend `RecipeChoice`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecipeChoice {
    EqualPower,
    HardCut,
    BassSwap,
    FilterFade,
    FilterKill,
}
```

(`pick_recipe`'s body is unchanged — these recipes aren't auto-picked yet.)

- [ ] **Step 4: Verify the tests pass**

Run: `cd engine && cargo test --test recipe_test`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/recipe.rs engine/tests/recipe_test.rs
git commit -m "feat(engine): FilterFade and FilterKill recipes (sub-project 3, task 3)"
```

---

### Task 4: `--force-recipe` CLI flag

**Files:**
- Modify: `engine/src/config.rs`
- Modify: `engine/src/main.rs`

- [ ] **Step 1: Add `ForceRecipe` enum + CLI field**

Edit `engine/src/config.rs`. Add at the bottom:

```rust
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ForceRecipe {
    EqualPower,
    HardCut,
    BassSwap,
    FilterFade,
    FilterKill,
}

impl ForceRecipe {
    pub fn to_recipe_choice(self) -> crate::recipe::RecipeChoice {
        match self {
            ForceRecipe::EqualPower => crate::recipe::RecipeChoice::EqualPower,
            ForceRecipe::HardCut => crate::recipe::RecipeChoice::HardCut,
            ForceRecipe::BassSwap => crate::recipe::RecipeChoice::BassSwap,
            ForceRecipe::FilterFade => crate::recipe::RecipeChoice::FilterFade,
            ForceRecipe::FilterKill => crate::recipe::RecipeChoice::FilterKill,
        }
    }
}
```

Extend the `Cli` struct (add a new field at the end, before the closing brace):

```rust
/// Force every transition to use this recipe, overriding pick_recipe.
/// Mainly for manual testing of new recipes.
#[arg(long, value_enum)]
pub force_recipe: Option<ForceRecipe>,
```

- [ ] **Step 2: Apply the override + new arms in `main.rs`**

Edit `engine/src/main.rs`. Update the choice computation just before the arming log:

```rust
let natural_choice = dj_engine::recipe::pick_recipe(&plan);
let choice = cli
    .force_recipe
    .map(|fr| fr.to_recipe_choice())
    .unwrap_or(natural_choice);
if cli.force_recipe.is_some() && choice != natural_choice {
    info!(forced = ?choice, natural = ?natural_choice, "force_recipe override active");
}
```

Update the `recipe_name` block to include the new variants:

```rust
let recipe_name: &str = match choice {
    dj_engine::recipe::RecipeChoice::EqualPower => "equal_power_crossfade",
    dj_engine::recipe::RecipeChoice::HardCut => "hard_cut",
    dj_engine::recipe::RecipeChoice::BassSwap => "bass_swap_crossfade",
    dj_engine::recipe::RecipeChoice::FilterFade => "filter_fade",
    dj_engine::recipe::RecipeChoice::FilterKill => "filter_kill",
};
```

Update the `let handle = match choice { ... }` block to add the two new arms (mirroring the existing `BassSwap` arm):

```rust
dj_engine::recipe::RecipeChoice::FilterFade => {
    dj_engine::scheduler::TransitionScheduler::arm(
        &mut mixer,
        current_deck,
        other_deck,
        &current,
        &next_track,
        &plan,
        &dj_engine::recipe::FilterFade,
    )
    .await?
}
dj_engine::recipe::RecipeChoice::FilterKill => {
    dj_engine::scheduler::TransitionScheduler::arm(
        &mut mixer,
        current_deck,
        other_deck,
        &current,
        &next_track,
        &plan,
        &dj_engine::recipe::FilterKill,
    )
    .await?
}
```

- [ ] **Step 3: Verify build + tests**

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
git commit -m "feat(engine): --force-recipe CLI flag + FilterFade/FilterKill arm match (sub-project 3, task 4)"
```

---

### Task 5: Integration tests for FilterFade and FilterKill

**Files:**
- Modify: `engine/tests/mixer_integration_test.rs`

- [ ] **Step 1: Add two `#[tokio::test]` functions modeled on the existing BassSwap test**

Open `engine/tests/mixer_integration_test.rs`. Add the imports:

```rust
use dj_engine::recipe::{FilterFade, FilterKill};
```

Add two new tests, modeled on the existing `bass_swap_transition_outputs_continuous_audio`:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn filter_fade_transition_outputs_continuous_audio() {
    let cap = (SAMPLE_RATE as usize) * 2 * 10;
    let (producer, consumer) = make_ring(cap);
    let mut mixer = Mixer::new(producer);

    let outgoing = track("sine_440_6s.wav", 6.0);
    let incoming = track("sine_660_6s.wav", 6.0);
    {
        let d = mixer.deck(DeckId::A);
        d.cue(outgoing.clone(), 0.0, 1.0).await.unwrap();
        d.play();
        d.test_set_fader(1.0);
    }

    let drain = drain_to_vec(consumer);
    tick_until(&mut mixer, 22_050).await.unwrap();

    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.5,
        incoming_crossfade_start_seconds: 1.0,
        length_bars: 2,
        use_eq_swap: false,
    };
    let handle = TransitionScheduler::arm(
        &mut mixer,
        DeckId::A,
        DeckId::B,
        &outgoing,
        &incoming,
        &plan,
        &FilterFade,
    )
    .await
    .unwrap();
    tick_until(&mut mixer, handle.end_master_clock_frames).await.unwrap();

    let all = drain.await.unwrap();
    assert!(!all.is_empty());
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn filter_kill_transition_outputs_continuous_audio() {
    let cap = (SAMPLE_RATE as usize) * 2 * 10;
    let (producer, consumer) = make_ring(cap);
    let mut mixer = Mixer::new(producer);

    let outgoing = track("sine_440_6s.wav", 6.0);
    let incoming = track("sine_660_6s.wav", 6.0);
    {
        let d = mixer.deck(DeckId::A);
        d.cue(outgoing.clone(), 0.0, 1.0).await.unwrap();
        d.play();
        d.test_set_fader(1.0);
    }

    let drain = drain_to_vec(consumer);
    tick_until(&mut mixer, 22_050).await.unwrap();

    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.5,
        incoming_crossfade_start_seconds: 1.0,
        length_bars: 2,
        use_eq_swap: false,
    };
    let handle = TransitionScheduler::arm(
        &mut mixer,
        DeckId::A,
        DeckId::B,
        &outgoing,
        &incoming,
        &plan,
        &FilterKill,
    )
    .await
    .unwrap();
    tick_until(&mut mixer, handle.end_master_clock_frames).await.unwrap();

    let all = drain.await.unwrap();
    assert!(!all.is_empty());
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

(Note: if `drain_to_vec` isn't defined as a free function, the existing `bass_swap_*` test already inlines the drain pattern; copy it once and factor into a helper inside this file if convenient. If you do factor it, refactor only this file — don't touch the existing tests' structure beyond extracting the helper.)

- [ ] **Step 2: Verify**

Run: `cd engine && cargo test --test mixer_integration_test`
Expected: all four integration tests pass.

- [ ] **Step 3: Commit**

```
git add engine/tests/mixer_integration_test.rs
git commit -m "test(engine): integration tests for FilterFade and FilterKill (sub-project 3, task 5)"
```

---

### Task 6: Final verification

**Files:** none — verification only.

- [ ] **Step 1: `cargo fmt`**

Run: `cd engine && cargo fmt --check`
If output: `cargo fmt && git add -A && git commit -m "chore(engine): fmt after sub-project 3"`.

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
- Spec Component 1 (`AutomationTarget::Filter`) → Task 1.
- Spec Component 2 (`DeckState.filter_knob` + biquads + reset + helpers) → Task 2.
- Spec Component 3 (`Deck::pull` filter step) → Task 2.
- Spec Component 4 (`FilterFade` recipe) → Task 3.
- Spec Component 5 (`FilterKill` recipe) → Task 3.
- Spec Component 6 (`RecipeChoice` extension + CLI flag + override) → Tasks 3+4.
- Spec Component 7 (`transition_type_label` unchanged) → no task needed.

No placeholders. No "implement later". Each code step shows full source.
