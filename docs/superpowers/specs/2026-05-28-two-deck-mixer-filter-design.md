# Two-Deck Mixer — One-Knob Filter + Filter Recipes (Sub-project 3)

**Date:** 2026-05-28
**Status:** Approved for planning
**Branch:** `research/two-deck-mixer`
**Sub-project:** 3 of 5

## Summary

Add a per-deck **one-knob filter** (the standard DJ-mixer move: turn left for low-pass sweep, center for bypass, turn right for high-pass sweep) and two new recipes that drive it: `FilterFade` (incoming filters in from the top, treble first) and `FilterKill` (outgoing filters out to silence as the incoming enters normally). Because `pick_recipe` is still plan-driven (sub-project 5 will make selection scoring-driven), this sub-project also adds a `--force-recipe` CLI flag so the new recipes can be exercised manually for verification.

## Goals

- Each `Deck` exposes a `filter_knob: f64` in `[-1.0, +1.0]` (default 0.0 = bypass).
- A single biquad per channel (2 per deck) operates as low-pass when `knob < 0`, high-pass when `knob > 0`, and is skipped entirely when `|knob| < 0.01` so a deck with no active filter has zero filter overhead.
- New `AutomationTarget::Filter` lets recipes sweep the knob over time.
- New recipes `FilterFade` and `FilterKill`, each producing the standard equal-power fader pair plus a `Filter` automation on the appropriate deck.
- New CLI flag `--force-recipe {equal_power, hard_cut, bass_swap, filter_fade, filter_kill}` overrides `pick_recipe`'s choice for every transition. Lets the operator hand-pick a recipe to test it on real audio without waiting for the planner to choose it.
- Existing behavior unchanged when `--force-recipe` is not set: `pick_recipe` still routes `length_bars=0 → HardCut`, `use_eq_swap → BassSwap`, else `EqualPowerCrossfade`. The filter recipes stay dormant until either an operator forces them or sub-project 5 makes the selector pick them.

## Non-Goals (this sub-project)

- No selector changes. The selector still returns just a track; the new recipes only run via `--force-recipe`.
- No filter resonance / Q parameter; biquads stay Butterworth `Q = 1/√2` for stable, non-resonant sweeps.
- No mid-band one-knob mode (DJ filters typically don't have one). If we ever want band-pass behavior we'd reach for the per-deck EQ from sub-project 2.
- No state persistence of the knob across deck role swaps — `cue()` and `unload()` reset it to 0.

## Audible behavior

| Recipe | What you hear |
|---|---|
| `FilterFade` | Outgoing fades out normally (equal-power cosine). Incoming starts inaudible (fader=0) **and** filtered to its treble band (filter_knob=+1, HP at 8 kHz). Over the crossfade, fader rises 0→1 *and* knob falls +1→0, so the incoming "filters in" — first you hear hi-hats and cymbals, then the mids fill in, then the bass arrives. Common build-up move. |
| `FilterKill` | Incoming fades in normally. Outgoing's fader fades 1→0 across the full crossfade, **and** its filter_knob sweeps 0→+1 across the first ~60% of the crossfade, so the outgoing's low end is gone before its mids are gone before the fader takes the rest. The outgoing "filters out" to silence. Common breakdown/wipe move. |

Both recipes are equal-power on the fader side — total power stays roughly constant across the crossfade.

## Components

### Component 1 — `Filter` automation target

```rust
pub enum AutomationTarget {
    Fader,
    Tempo,
    EqLow,
    EqMid,
    EqHigh,
    Filter,           // NEW: range [-1.0, +1.0], 0 = bypass
}
```

`apply_value` clamps to `[-1.0, +1.0]`.

### Component 2 — DeckState + Deck biquads

Add to `DeckState`:

```rust
pub filter_knob: f64,        // default 0.0; clamped [-1.0, +1.0]
```

Add to `Deck` (private fields):

```rust
filter_l: Biquad,
filter_r: Biquad,
last_filter_knob: f64,       // cached for the "recompute if changed" guard
```

Module-private constants for the sweep mapping:

```rust
// Low-pass: knob 0 → 22 kHz (passthrough), knob -1 → 200 Hz
const FILTER_LP_FREQ_BYPASS:    f64 = 22_000.0;
const FILTER_LP_FREQ_MAX_LEFT:  f64 =    200.0;

// High-pass: knob 0 → 10 Hz (passthrough), knob +1 → 8 kHz
const FILTER_HP_FREQ_BYPASS:    f64 =     10.0;
const FILTER_HP_FREQ_MAX_RIGHT: f64 =  8_000.0;

// Below this absolute knob value, skip the filter entirely (no biquad evaluation).
const FILTER_BYPASS_EPSILON:    f64 =    0.01;
```

Mapping (log-Hz lerp) — implemented as a private helper inside `deck.rs`:

```rust
fn filter_cutoff_hz(knob: f64) -> (FilterKind, f64) {
    if knob >= 0.0 {
        (FilterKind::HighPass, log_lerp(FILTER_HP_FREQ_BYPASS, FILTER_HP_FREQ_MAX_RIGHT, knob))
    } else {
        (FilterKind::LowPass, log_lerp(FILTER_LP_FREQ_BYPASS, FILTER_LP_FREQ_MAX_LEFT, -knob))
    }
}

fn log_lerp(from: f64, to: f64, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    (from.ln() + (to.ln() - from.ln()) * t).exp()
}
```

`cue()` and `unload()` reset `filter_knob = 0.0`, `last_filter_knob = 0.0`, and re-initialize both biquads as a bypassed low-pass at `FILTER_LP_FREQ_BYPASS` (so the initial mode is harmless even if we don't take the bypass branch immediately).

### Component 3 — `Deck::pull` ordering

The signal chain becomes:

```
decoder output
  → eq_low (low-shelf)
  → eq_mid (peaking)
  → eq_high (high-shelf)
  → filter   ← NEW (skipped when |knob| < 0.01)
  → fader
  → mix
```

In `pull`, after the existing EQ recompute block, add a parallel block for the filter:

```rust
self.recompute_filter_if_dirty();
let filter_active = self.state.filter_knob.abs() >= FILTER_BYPASS_EPSILON;
```

Then in the per-sample loop, after applying the EQ chain and before the fader:

```rust
let (l, r) = if filter_active {
    (self.filter_l.process(l), self.filter_r.process(r))
} else {
    (l, r)
};
```

`recompute_filter_if_dirty` compares `state.filter_knob` to `last_filter_knob`. When they differ by more than `f64::EPSILON`:
- If the new knob is inside `(-FILTER_BYPASS_EPSILON, +FILTER_BYPASS_EPSILON)`, reset biquad coefficients to a passthrough LP at bypass freq (doesn't matter — bypass branch will skip).
- Otherwise call `filter_cutoff_hz(knob)` and `Biquad::set_cutoff(kind, cutoff)` on both channels. (`Biquad::set_cutoff` already exists and supports LP/HP.)
- Update `last_filter_knob`.

**Mode crossing (LP↔HP at knob=0)**: when the knob crosses zero, the biquad gets re-coefficient'd from LP to HP (or vice versa). The state samples (`x1/x2/y1/y2`) are NOT cleared, so there's a brief transient as the biquad "settles" into the new mode. In practice this is masked by the bypass window around zero (the no-filter region) so the mode change happens silently while the biquad isn't being applied.

### Component 4 — `FilterFade` recipe

```rust
pub struct FilterFade;

impl Recipe for FilterFade {
    fn name(&self) -> &'static str { "filter_fade" }

    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        // Equal-power fader pair (same as EqualPowerCrossfade)
        // + Tempo Step (no-op confirmation)
        // + incoming Filter Linear from +1.0 to 0.0 over the full crossfade
        // outgoing has no Filter automation (stays at 0)
    }
}
```

Automations:

```
outgoing.Fader  EqualPowerOut  1.0 → 0.0    over [t0, t0+L]
incoming.Fader  EqualPowerIn   0.0 → 1.0    over [t0, t0+L]
incoming.Tempo  Step           ratio → ratio at [t0, t0+1]      (no-op)
incoming.Filter Step           1.0 → 1.0    at [t0, t0+1]       (init: full HP)
incoming.Filter Linear         1.0 → 0.0    over [t0, t0+L]     (sweep open)
```

`total_duration_frames = L`.

### Component 5 — `FilterKill` recipe

```rust
pub struct FilterKill;

impl Recipe for FilterKill {
    fn name(&self) -> &'static str { "filter_kill" }

    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        // Equal-power fader pair on both decks
        // + Tempo Step
        // + outgoing Filter Linear from 0.0 to +1.0 over [t0, t0 + L*0.6]
    }
}
```

Automations:

```
outgoing.Fader  EqualPowerOut  1.0 → 0.0    over [t0, t0+L]
incoming.Fader  EqualPowerIn   0.0 → 1.0    over [t0, t0+L]
incoming.Tempo  Step           ratio → ratio at [t0, t0+1]
outgoing.Filter Linear         0.0 → 1.0    over [t0, t0 + L*0.6]
```

`total_duration_frames = L`.

The outgoing's filter reaches +1.0 (full HP at 8 kHz) by 60 % of the way through the crossfade, then stays there for the last 40 % while the fader takes everything down to silence. The incoming fades in over the entire window with no filtering.

### Component 6 — `RecipeChoice` extension and `--force-recipe` CLI

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

`pick_recipe`'s body is unchanged. The new variants are reached only via `--force-recipe`.

`config.rs` extension:

```rust
pub struct Cli {
    // existing fields ...

    /// Force every transition to use this recipe, overriding pick_recipe.
    /// One of: equal_power, hard_cut, bass_swap, filter_fade, filter_kill.
    /// Mainly for manual testing of new recipes.
    #[arg(long)]
    pub force_recipe: Option<ForceRecipe>,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ForceRecipe {
    EqualPower,
    HardCut,
    BassSwap,
    FilterFade,
    FilterKill,
}

impl ForceRecipe {
    pub fn to_recipe_choice(self) -> RecipeChoice { /* one-to-one */ }
}
```

`main.rs` integration:

```rust
let choice = cli
    .force_recipe
    .map(ForceRecipe::to_recipe_choice)
    .unwrap_or_else(|| dj_engine::recipe::pick_recipe(&plan));
```

Add a log line when override is in effect:

```rust
if cli.force_recipe.is_some() {
    info!(
        forced = ?choice,
        natural = ?dj_engine::recipe::pick_recipe(&plan),
        "force_recipe override is active"
    );
}
```

(Once per arming, so the log makes it obvious you're not seeing natural planner behavior.)

### Component 7 — `transition_type_label`

Extend the labels:

```rust
fn transition_type_label(length_bars: usize, use_eq_swap: bool) -> &'static str {
    // unchanged — labels are derived from plan, not from recipe choice
}
```

Since `transition_type` reflects the planner's *intent* (the bass_swap suffix comes from `use_eq_swap=true`), not the realized recipe, this stays as-is. When `--force-recipe` overrides, the existing arming log already prints both `transition_type` (planner intent) and `recipe` (actually selected) on the same line, so the divergence is visible.

## Testing strategy

### Unit tests

- **`automation.rs`**:
  - `apply_value` for `Filter` updates `state.filter_knob` and clamps to `[-1.0, +1.0]`.
- **`deck.rs`**:
  - `cue()` resets `filter_knob = 0` and `last_filter_knob = 0`.
  - `unload()` same.
  - Schedule `Filter` Step at +1.0; pull; verify `state.filter_knob == 1.0` and the deck's biquad is configured as HP at ~8 kHz. Drive a 100 Hz sine vs a 5 kHz sine through it: 100 Hz attenuated, 5 kHz mostly intact.
  - Schedule `Filter` Step at -1.0; same test mirrored: 5 kHz attenuated, 100 Hz intact (within LP slope tolerance).
  - With `filter_knob = 0.0`, output equals input (bypass branch).
- **`recipe.rs`**:
  - `FilterFade::build` emits exactly: 1 outgoing automation (Fader); 3 incoming (Fader, Tempo Step, Filter Step at +1, Filter Linear +1→0). Actually 4 incoming — recount: Fader, Tempo Step, Filter Step (init), Filter Linear (sweep). So `incoming_automations.len() == 4`.
  - `FilterKill::build` emits: 2 outgoing (Fader, Filter Linear 0→+1 over [t0, t0+L*0.6]); 2 incoming (Fader, Tempo Step).
- **`config.rs`** / CLI parsing:
  - `--force-recipe filter_fade` → `cli.force_recipe == Some(ForceRecipe::FilterFade)`.
  - Missing flag → `None`.
- **`recipe.rs::pick_recipe`** — unchanged routing test still passes; no new `pick_recipe` case for filter recipes (they're operator-only in this sub-project).

### Integration test

- Extend `mixer_integration_test.rs` with a `FilterFade` and `FilterKill` test, similar in shape to the `BassSwap` test. Use the existing 6 s sine fixtures. Don't try to verify the audible filtering with FFT; just verify continuous audio and no underrun.

### Manual verification (the point of `--force-recipe`)

Restart the engine with `--force-recipe filter_fade` and listen to a transition. Should hear the incoming track building from treble down. Then restart with `--force-recipe filter_kill` and hear the outgoing wiping away.

## Risks

- **Mode-switch glitch at `knob = 0`**: when the knob crosses zero, the biquad's filter type flips LP↔HP. State samples carry over and may produce a brief transient. Mitigation: the bypass window around zero (knob ∈ [-0.01, +0.01]) skips the biquad entirely; the mode flip happens while the biquad isn't being evaluated, so the transient never reaches the output.
- **Resonance ringing at very low LP cutoffs**: at knob = -1, cutoff = 200 Hz. Below that, only sub-bass passes. Butterworth Q (no resonance) keeps this clean, but a very heavy bass track might get some low-frequency phase shift that sounds odd. Acceptable for sub-project 3; no mitigation needed.
- **Forced-recipe doesn't undo per-deck stickiness**: if the planner decided this was a hard-cut moment (length_bars=0), forcing `filter_fade` makes the deck run a multi-second crossfade in the wrong context. The flag is a *testing* tool; behavior under it is "use whatever recipe I named, you're responsible for the audible result."

## Open questions

- The 60 % cutoff in `FilterKill` is a stylistic choice. Could expose as a knob (recipe parameter) later if we want operator-tunable transitions. Not in scope.

## Roadmap (remaining sub-projects)

- **Sub-project 4** — IIR echo delay + loop-capture buffer on the deck. `EchoOut` (outgoing echo crescendo) and `LoopOut` (outgoing locks into a 4-bar loop while incoming swaps in) recipes.
- **Sub-project 5** — Selector returns `(next_track, recipe)`. Scoring favors recipes based on the pair's BPM gap, bass content, key relationship, energy delta. Removes the need for `--force-recipe`.

## Project layout changes

```
engine/src/
  config.rs                  ← +ForceRecipe enum, +Cli.force_recipe field
  automation.rs              ← +AutomationTarget::Filter, +clamp in apply_value
  deck.rs                    ← +filter_knob in state, +filter_l/r + cache, +mapping
                               helpers, +recompute_filter_if_dirty, +filter step in pull
  recipe.rs                  ← +FilterFade, +FilterKill, +RecipeChoice::FilterFade,
                               +RecipeChoice::FilterKill
  main.rs                    ← +force_recipe override in choice selection, +arm match
                               adds FilterFade/FilterKill arms, +override log
engine/tests/
  automation_test.rs         ← +Filter apply tests
  deck_test.rs               ← +cue resets knob, +Filter Step +1 attenuates lows
  recipe_test.rs             ← +FilterFade/FilterKill build tests
  mixer_integration_test.rs  ← +FilterFade and FilterKill continuous-audio tests
```
