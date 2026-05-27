# Two-Deck Mixer — Per-Deck EQ + BassSwap (Sub-project 2)

**Date:** 2026-05-28
**Status:** Approved for planning
**Branch:** `research/two-deck-mixer`
**Sub-project:** 2 of 5

## Summary

Add a per-deck 3-band EQ (low-shelf, peaking-mid, high-shelf) to each `Deck`, plus a `BassSwap` recipe that uses per-deck `EqLow` automations to kill the outgoing's bass before the incoming's bass enters. Restores the bass-clash mitigation that sub-project 1 dropped (and that the pre-refactor engine produced via `mix_crossfade_eq`'s master-bus highpass sweeps) — now as a deck-local, automation-driven feature that future sub-projects can extend (filter sweeps, echo-tail bass cuts, mid scoops, etc.).

## Goals

- Each `Deck` exposes three band gains in dB: `eq_low_db`, `eq_mid_db`, `eq_high_db`. Default 0.0 (passthrough). Range clamped to `[-40, +6]` (kill to small boost).
- Six biquads per deck (3 bands × stereo) processed inside `Deck::pull` after the decoder, before the fader.
- New `AutomationTarget` variants: `EqLow`, `EqMid`, `EqHigh`. Recipes drive EQ gains via automations exactly the way they already drive the fader.
- New `BassSwap` recipe: same equal-power fader curves as `EqualPowerCrossfade`, plus `EqLow` automations that drop the outgoing's bass to −40 dB before the incoming's bass climbs back from −40 dB to 0 dB. The swap timing adapts to the two tracks' bass content (from sidecar `band_energy_means.bass`).
- `pick_recipe` route: `length_bars == 0` → `HardCut`; `use_eq_swap` → `BassSwap`; else → `EqualPowerCrossfade`. (Sub-project 5 makes the selector pick recipes directly; until then `plan_transition` still decides.)

## Non-Goals (this sub-project)

- No recipe currently drives `EqMid` or `EqHigh` — both targets exist so the foundation is ready, but only `EqLow` is exercised in v1. (Sub-project 4 may add mid-scoop or treble-fade recipes.)
- No real-time external EQ control (no CLI/API surface to nudge the EQ); automations are the only writer.
- No per-band Q tuning. Shelves use Butterworth `Q = 1/√2`. The peaking mid uses `Q = 1.0`. Frequencies hard-coded: 200 Hz low, 1 kHz mid, 5 kHz high.
- No change to the existing legacy `apply_swept_filter` helper in `eq.rs` — that stays `#[allow(dead_code)]` for sub-project 3's filter-knob recipe. We don't need it for `BassSwap`.

## Audible behavior summary

| `plan.length_bars` | `plan.use_eq_swap` | Recipe (v1 sub-proj 2)         | Audible behavior change vs. sub-proj 1 |
|---|---|---|---|
| 0 | any | `HardCut` | unchanged |
| 8 / 16 / 32 | false | `EqualPowerCrossfade` | unchanged |
| 8 / 16 / 32 | true | **`BassSwap`** | **outgoing's low band ducks to −40 dB before the midpoint; incoming's low band starts at −40 dB and climbs back to 0 just past the midpoint** |

The bass-swap restores what the pre-refactor `mix_crossfade_eq` was doing — but now each deck owns its own EQ chain, so sub-projects 4/5 can compose it with other effects without revisiting the mixer.

## Components

### Component 1 — `eq.rs` additions

Three new constructors plus three in-place setters on `Biquad`. RBJ audio-EQ-cookbook formulas; all use `f64` arithmetic internally.

```rust
impl Biquad {
    pub fn low_shelf(sample_rate: f64, freq_hz: f64, gain_db: f64) -> Self;
    pub fn peaking  (sample_rate: f64, freq_hz: f64, q: f64, gain_db: f64) -> Self;
    pub fn high_shelf(sample_rate: f64, freq_hz: f64, gain_db: f64) -> Self;

    pub fn set_low_shelf (&mut self, sample_rate: f64, freq_hz: f64, gain_db: f64);
    pub fn set_peaking   (&mut self, sample_rate: f64, freq_hz: f64, q: f64, gain_db: f64);
    pub fn set_high_shelf(&mut self, sample_rate: f64, freq_hz: f64, gain_db: f64);
}
```

Shelf slope `S = 1.0` is hardcoded inside the constructor (standard cookbook "max slope, no overshoot"). Each setter mutates coefficients in place but leaves the state samples (`x1/x2/y1/y2`) untouched so a sweep doesn't pop.

### Component 2 — `DeckState` and `Deck`

Add to `DeckState`:

```rust
pub eq_low_db:  f64,   // default 0.0; clamped [-40.0, +6.0]
pub eq_mid_db:  f64,
pub eq_high_db: f64,
```

Add to `Deck` (private fields):

```rust
eq_low_l:  Biquad,
eq_low_r:  Biquad,
eq_mid_l:  Biquad,
eq_mid_r:  Biquad,
eq_high_l: Biquad,
eq_high_r: Biquad,

// Tracks the gain that was used the last time biquad coefficients were
// recomputed, so we only recompute when state actually changed.
last_eq_low_db:  f64,
last_eq_mid_db:  f64,
last_eq_high_db: f64,
```

Hardcoded band centers as module-private constants:

```rust
const EQ_LOW_FREQ:  f64 =  200.0;
const EQ_MID_FREQ:  f64 = 1000.0;
const EQ_MID_Q:     f64 =    1.0;
const EQ_HIGH_FREQ: f64 = 5000.0;
```

`Deck::new` and `Deck::unload` initialize / reset all six biquads at gain 0 dB (passthrough) and clear `last_eq_*_db = 0.0`.

`Deck::cue` re-initializes the biquads at 0 dB regardless of incoming track (a fresh chain per track).

### Component 3 — `Deck::pull` ordering

Per tick:

1. Pre-step: `automations.step(start_cursor, &mut state)` so any automations starting *at* `start_cursor` (e.g., `BassSwap`'s `EqLow Step from=-40 to=-40` on the incoming) update `eq_*_db` before the biquads' coefficients are computed.
2. Recompute biquad coefficients if any of `state.eq_low_db / eq_mid_db / eq_high_db` differs from `last_eq_*_db`. Update `last_eq_*_db`. (No allocation, just six `set_*` calls and three comparisons.)
3. Drain the source queue into `out` (existing logic, unchanged).
4. Per-sample loop:
   - `automations.step(cursor, &mut state)` (per-sample, as today, so fader and tempo automations evaluate correctly each frame).
   - Process L and R through `eq_low → eq_mid → eq_high` biquads (six `Biquad::process` calls per frame).
   - Multiply by `state.fader`.
5. Final `automations.step(self.frame_cursor, &mut state)` to install `to` values for automations finishing exactly on the tick boundary.
6. Advance `source_position_seconds += emitted_seconds * tempo` (unchanged).

**Coefficient-recompute granularity.** Per-tick recompute means a `Linear` EQ sweep changes coefficients in steps of ~93 ms (TICK_FRAMES at 44.1 kHz). For a 16-bar bass-swap that lasts several seconds, this is far below audible zipper threshold. For an instantaneous EQ change (a `Step` automation), the change is sample-accurate at the tick boundary on which the step fires, because the pre-step ensures `state.eq_low_db` is updated before coefficients are recomputed.

**Why apply EQ before the fader.** Linearity makes the order mathematically equivalent, but the DJ-traditional signal path is decoder → EQ → fader → sum. Applying biquads before the fader matches the mental model and makes future per-deck effects (filter knob, echo send) compose naturally.

### Component 4 — `AutomationTarget` extension

```rust
pub enum AutomationTarget {
    Fader,
    Tempo,
    EqLow,
    EqMid,
    EqHigh,
}
```

`apply_value` in `automation.rs`:

```rust
match target {
    AutomationTarget::Fader  => { state.fader = v.clamp(0.0, 1.0) as f32; }
    AutomationTarget::Tempo  => { state.tempo = v; }
    AutomationTarget::EqLow  => { state.eq_low_db  = v.clamp(-40.0, 6.0); }
    AutomationTarget::EqMid  => { state.eq_mid_db  = v.clamp(-40.0, 6.0); }
    AutomationTarget::EqHigh => { state.eq_high_db = v.clamp(-40.0, 6.0); }
}
```

### Component 5 — `BassSwap` recipe

```rust
pub struct BassSwap;

impl Recipe for BassSwap {
    fn name(&self) -> &'static str { "bass_swap_crossfade" }

    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput { /* see below */ }
}
```

The recipe emits all the fader/tempo automations that `EqualPowerCrossfade` does, **plus** EqLow automations.

**Bass-content adaptation** (matches the pre-refactor behavior of `mix_crossfade_eq`'s `BassMixHint` logic):

| outgoing bass | incoming bass | swap window (fractions of L) |
|---|---|---|
| both < 0.3 | both < 0.3 | **skip the EQ entirely** — return the same automations as `EqualPowerCrossfade`. Running shelves on near-empty bass is just biquad noise. |
| both > 0.5 | both > 0.5 | outgoing kill `[0.30, 0.48]`, incoming release `[0.52, 0.70]` — wider gap because both basses are big. |
| any other  |  | outgoing kill `[0.35, 0.48]`, incoming release `[0.52, 0.65]` — default, tighter. |

Bass means come from `ctx.outgoing.sidecar.band_energy_means.as_ref().map(|b| b.bass)` (and incoming). If either is `None` (older sidecars), treat as 0.5 (use default windows).

For non-skip cases, the automations are:

```
outgoing.Fader  EqualPowerOut 1.0 → 0.0     over [t0, t0+L]
incoming.Fader  EqualPowerIn  0.0 → 1.0     over [t0, t0+L]
incoming.Tempo  Step          ratio → ratio at [t0, t0+1]              (no-op confirmation)
outgoing.EqLow  Linear        0.0 → -40.0   over [t0 + L*kill_start,  t0 + L*kill_end]
incoming.EqLow  Step          -40.0 → -40.0 at [t0, t0+1]              (init kill at sample 0)
incoming.EqLow  Linear        -40.0 → 0.0   over [t0 + L*rel_start,   t0 + L*rel_end]
```

`total_duration_frames = L`. No pitch-journey ramp (consistent with sub-project 1's fix).

### Component 6 — `pick_recipe` + `RecipeChoice`

```rust
pub enum RecipeChoice {
    EqualPower,
    HardCut,
    BassSwap,
}

pub fn pick_recipe(plan: &TransitionPlan) -> RecipeChoice {
    if plan.length_bars == 0 {
        RecipeChoice::HardCut
    } else if plan.use_eq_swap {
        RecipeChoice::BassSwap
    } else {
        RecipeChoice::EqualPower
    }
}
```

### Component 7 — `main.rs` arm match

Add the `BassSwap` arm. `transition_type_label` already handles the `_with_eq_swap_intended` labels added in sub-project 1; we change the suffix from `_intended` to actual realization:

```rust
fn transition_type_label(length_bars: usize, use_eq_swap: bool) -> &'static str {
    match (length_bars, use_eq_swap) {
        (0, _)         => "hard_cut",
        (8, false)     => "quick_cut",
        (8, true)      => "bass_swap_quick_cut",
        (16, false)    => "standard_blend",
        (16, true)     => "bass_swap_standard_blend",
        (32, false)    => "long_blend",
        (32, true)     => "bass_swap_long_blend",
        _              => "custom_blend",
    }
}
```

The arming log already includes `recipe = recipe_name` which surfaces the actual recipe selected, so debugging is straightforward: when `transition_type` says `bass_swap_…` but `recipe` says `equal_power_crossfade`, that signals the bass-content adaptation skipped the swap.

## Testing strategy

### Unit tests

- **`eq.rs`**:
  - Each new constructor at 0 dB produces a coefficient set whose impulse response sums to ~1 (passthrough).
  - `low_shelf` at +6 dB and `high_shelf` at +6 dB: process a unit DC step, verify gain ≈ 2× for the boosted band's region. (Sanity check, not an exhaustive frequency-response test.)
  - `set_low_shelf` mid-stream does NOT clear the state samples (the biquad continues filtering smoothly).
- **`automation.rs`**:
  - `apply_value` for `EqLow` / `EqMid` / `EqHigh` updates the right field and clamps `[-40, +6]`.
- **`recipe.rs`**:
  - `BassSwap::build` with `band_energy_means = None`:
    - outgoing_automations.len() == 2 (Fader + EqLow Linear)
    - incoming_automations.len() == 4 (Fader + Tempo Step + EqLow Step + EqLow Linear)
    - EqLow timings match default windows.
  - `BassSwap::build` with both bass means < 0.3:
    - outgoing_automations.len() == 1 (just Fader)
    - incoming_automations.len() == 2 (Fader + Tempo Step) — no EqLow.
  - `BassSwap::build` with both bass means > 0.5:
    - EqLow Linear timings match wide windows (`[0.30 L, 0.48 L]`, `[0.52 L, 0.70 L]`).
  - `pick_recipe` returns `BassSwap` for `length_bars > 0 && use_eq_swap`.
- **`deck.rs`**:
  - `cue()` initializes `eq_*_db = 0.0`.
  - `unload()` resets `eq_*_db = 0.0`.
  - Schedule an `EqLow Step from=-40 to=-40 at [0, 1]` automation. After pull(1024), `state.eq_low_db == -40` and the deck's `eq_low_l/r` biquads have been re-coefficient'd. Spot-check by feeding a 100 Hz sine and a 1000 Hz sine: 100 Hz output is much quieter (~ -40 dB) than 1000 Hz.

### Integration tests

- Extend `mixer_integration_test.rs` to drive `BassSwap` (with sidecar `band_energy_means.bass > 0.5` on both fixtures so the swap actually runs). Assert:
  - Output is continuous (no underrun).
  - During the swap window, total power is plausible (not silenced, not clipped).
  - (Optional, can be deferred:) FFT of the swap window shows the outgoing's <200 Hz content attenuated relative to its mid/high content.

### Drop-in verification (manual)

User listens to a transition where the analyzer's sidecar plan returned `use_eq_swap=true`. They should hear:
- The outgoing's bassline ducking just before the midpoint of the crossfade.
- A brief no-bass window.
- The incoming's bassline arriving just after the midpoint.
- Smooth, no clicks or pops.

For `use_eq_swap=false` transitions, audio should be identical to sub-project 1.

## Risks

- **Sidecar `band_energy_means` may be missing on older sidecars.** Mitigation: treat as 0.5 and use default windows. Existing test fixtures already cover the `None` path.
- **Biquad recompute on every tick adds ~6 cheap function calls per deck per tick (~25 μs total).** Negligible compared to ffmpeg subprocess + ring-buffer push overhead. No mitigation needed.
- **Step automations at sub-tick granularity** (an EqLow Step that starts at cursor X within a tick that already started at cursor Y < X) would update `eq_*_db` mid-tick but the biquad coefficients wouldn't be recomputed until the next tick. v1 recipes only emit Step automations at tick-aligned positions (`t_start`), so this isn't exercised. Documented for future recipes.
- **Drop-in equivalence for `use_eq_swap=true` transitions is not byte-accurate** with the pre-refactor `mix_crossfade_eq` path — the legacy path used swept highpass cutoffs; sub-project 2 uses low-shelf attenuation. Audibly similar (bass ducks then returns) but not identical PCM. This is acceptable for sub-project 1's stated "drop-in for use_eq_swap=false" guarantee; the `true` path was already explicitly listed as a temporary regression to be fixed in sub-project 2 (this).

## Open questions

- Hardcoded EQ band frequencies (200 Hz / 1 kHz / 5 kHz) match common DJ-mixer conventions. If the analyzer's `band_energy_curves` boundary frequencies differ, this could be inconsistent — to revisit if the sidecar exposes different defaults.

## Roadmap (remaining sub-projects)

- **Sub-project 3** — one-knob filter + `FilterFade` / `FilterKill` recipes. The `eq.rs::apply_swept_filter` helper (currently `#[allow(dead_code)]`) gets a caller. Adds a `Filter` AutomationTarget to the deck.
- **Sub-project 4** — IIR echo delay + loop-capture buffer on the deck. New `EchoOut` and `LoopOut` recipes.
- **Sub-project 5** — selector returns `(next_track, recipe)`; scoring extends to favor recipes that fit the pair.

## Project layout after sub-project 2

```
engine/src/
  eq.rs                      ← +3 constructors, +3 setters, +tests
  deck.rs                    ← +6 biquads, +3 dB fields on state, EQ chain in pull
  automation.rs              ← +3 EqLow/EqMid/EqHigh targets, +clamp on apply
  recipe.rs                  ← +BassSwap, +RecipeChoice::BassSwap, +pick_recipe
                               BassSwap routing
  main.rs                    ← +BassSwap arm in match, +transition_type_label
                               update
engine/tests/
  eq_test.rs                 ← +shelf/peaking sanity tests
  automation_test.rs         ← +Eq* apply tests
  recipe_test.rs             ← +BassSwap build tests (3 bass-content cases)
  deck_test.rs               ← +cue resets eq, +EqLow Step kills 100 Hz sine
  mixer_integration_test.rs  ← drive BassSwap with bass-heavy fixtures
```
