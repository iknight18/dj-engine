# Mixer Enhancement — "Real DJ Feel" Design

**Date:** 2026-05-27
**Status:** Approved for planning
**Builds on:** `2026-05-26-dj-engine-design.md`

## Summary

Upgrade the engine's mixer from a clean volume-only crossfade into something that sounds like a real DJ set, while staying fully autonomous. Three coupled enhancements, all engine-side (no analyzer changes):

1. **EQ swap during crossfade** — split frequency bands and crossfade them on different curves so basslines stop clashing.
2. **Phrase-aligned transitions** — snap transition start times to 16-bar phrase boundaries so transitions land where the listener's ear expects them.
3. **Adaptive crossfade length** — pick 32-bar / 16-bar / 8-bar / hard-cut transitions based on how similar the two tracks are (BPM / key / energy).

The mixer becomes a thin executor of a `TransitionPlan` produced by a new policy module. New DSP and policy logic live in focused new files; the existing `mixer.rs` shrinks.

## Goals

- Transitions sound like a human DJ mixed them: bass swaps cleanly, transitions land on phrase boundaries, and the engine adapts how it transitions based on the pair of tracks.
- Stays fully autonomous (no human input during a set).
- No analyzer changes — derive phrase boundaries from existing sidecar data (`downbeats_seconds`, `intro_end_seconds`, `outro_start_seconds`).
- Each new feature is implemented in a focused module that can be unit-tested in isolation.

## Non-Goals (v1 of this enhancement)

- 3-band EQ split (start with 2-band: lows/highs). Adding a mid band is a follow-up if needed.
- High-quality time-stretch via rubberband — keep ffmpeg's `atempo`. The current ±6% BPM range doesn't expose `atempo`'s weaknesses.
- Effects beyond EQ during transitions (no echo / reverb / filter risers on outro tails — those are separate v2 ideas).
- Sample-accurate beat alignment beyond what phrase-snapping already buys us. The downbeat math in `align.rs` stays as-is.
- Per-genre or per-mood transition rules. The decision is based on metric similarity only.
- ML/learning for threshold tuning. Thresholds are named constants in `transition.rs`, hand-tuned after listening.

## Architecture

```
                                    ┌───────────────────────┐
                                    │   selector.rs         │
                                    │   (unchanged)         │
                                    └──────────┬────────────┘
                                               │ picks `incoming`
                                               ▼
┌──────────┐   reads sidecars   ┌───────────────────────┐
│ phrase.rs│ ─────────────────▶ │  transition.rs        │
│  pure    │                    │  plan_transition()    │
└──────────┘                    │  → TransitionPlan     │
                                └──────────┬────────────┘
                                           │
                                           ▼
┌──────────┐                    ┌───────────────────────┐
│  eq.rs   │ ◀───── uses ────── │  crossfade.rs         │
│  pure    │                    │  mix_crossfade_eq()   │
│  biquads │                    │  mix_crossfade()      │
└──────────┘                    └──────────┬────────────┘
                                           │
                                           ▼
                                ┌───────────────────────┐
                                │  mixer.rs             │
                                │  (executes the plan)  │
                                └───────────────────────┘
```

### Module responsibilities

| Module | What it does | Pure? |
|---|---|---|
| `phrase.rs` | Derive 16-bar phrase-boundary timestamps from a sidecar | Yes |
| `transition.rs` | Pick crossfade length, variant, snapped start times for a pair of tracks | Yes |
| `eq.rs` | Biquad filter primitives (low-pass, high-pass) and a "swept filter" helper | Yes |
| `crossfade.rs` | Adds `mix_crossfade_eq()` alongside existing `mix_crossfade()` | Yes |
| `mixer.rs` | Consumes a `TransitionPlan`, drives decoders, applies crossfade, pushes to ring buffer | Effectful |

Selector, analyzer, ffmpeg subprocess modules, ring buffer, encoder, supervisor, logging — all unchanged.

## Component 1 — EQ swap during crossfade

### What the listener hears

Real DJs don't fade two whole tracks together. Two basslines playing at the same time sound muddy and clash. The DJ keeps the lows of one track at a time:

```
                bar 0 ─────── bar 8 (middle) ─────── bar 16
HIGHS  outgoing ──────────────── linear blend ──────────── incoming
LOWS   outgoing  ████████████ HARD swap @ bar 8 ████████████ incoming
```

After EQ swap: the bass of the outgoing track *replaces* the bass of the incoming at the midpoint; the highs and mids overlap throughout the crossfade.

### DSP

Two-band split using biquad filters. Cookbook (Robert Bristow-Johnson) low-pass and high-pass with Q = 0.707 (Butterworth response, no resonance).

For each crossfade of `crossfade_frames` frames:
- **Outgoing low-pass sweep:** cutoff = 22 kHz at `t = 0`, sweeps to 200 Hz over `t ∈ [0.375, 0.5]`, stays at 200 Hz onward. Effect: outgoing's bass is killed at the midpoint and never returns.
- **Incoming high-pass sweep:** cutoff = 200 Hz at `t = 0`, stays at 200 Hz until `t = 0.5`, sweeps to 22 kHz over `t ∈ [0.5, 0.625]`. Effect: incoming has no bass until the midpoint, then bass kicks in.
- **Volume envelope:** standard equal-power curves on both sides, applied AFTER the filtering. (Equal-power × filter = combined effect.)

The "swap window" is a 1-bar band centered at the crossfade midpoint (`t ∈ [0.375, 0.625]`). Outside that window, the filters are at their endpoints (effectively pass-through or hard-stop).

Independent biquad state for L and R channels.

### Module surface

```rust
// eq.rs
pub struct Biquad { /* state */ }

impl Biquad {
    pub fn lowpass(sample_rate: f64, cutoff_hz: f64) -> Self;
    pub fn highpass(sample_rate: f64, cutoff_hz: f64) -> Self;
    /// Mutate the coefficients without resetting state — for cutoff sweeps.
    pub fn set_cutoff(&mut self, sample_rate: f64, cutoff_hz: f64);
    pub fn process(&mut self, sample: f32) -> f32;
}

pub enum FilterKind { LowPass, HighPass }

/// Process a stereo interleaved buffer in place with a time-varying cutoff.
/// `cutoff_curve(t)` returns Hz, where t is normalized to [0, 1] across the buffer.
pub fn apply_swept_filter(
    samples: &mut [f32],
    kind: FilterKind,
    sample_rate: f64,
    cutoff_curve: impl Fn(f64) -> f64,
);
```

### `mix_crossfade_eq` signature

```rust
// crossfade.rs
pub fn mix_crossfade_eq(
    outgoing: &[f32],
    incoming: &[f32],
    crossfade_frames: usize,
    sample_rate: f64,
) -> Vec<f32>;
```

Returns a buffer the same length as `outgoing.min(incoming).len()`. The existing `mix_crossfade()` (volume-only) stays for the hard-cut/short-cut variants and as the no-EQ-swap fallback.

## Component 2 — Phrase-aligned transitions

### Why this matters

Most pop / electronic music is built in 16-bar phrases. A real DJ starts a transition on a phrase boundary so the new track's verse, chorus, or drop lands where the listener's ear expects something new.

Currently we start the crossfade at `outro_start_seconds`. That's accurate to within a few seconds but not aligned to a phrase. Snapping fixes it.

### Algorithm — zero analyzer changes needed

`phrase.rs`:

```rust
pub fn phrase_boundaries(sc: &Sidecar, phrase_bars: usize) -> Vec<f64>;
```

Steps:
1. Anchor on the first downbeat at or after `sc.intro_end_seconds`. That's the start of the body.
2. From that anchor, every `phrase_bars`-th downbeat is a phrase boundary.
3. Return ascending timestamps. Stop before `sc.outro_start_seconds`.

For a 165 s track at 140 BPM with `intro_end_seconds=28`, this returns 3-4 boundaries.

### How the mixer uses them

`transition.rs::plan_transition` computes:
- **Outgoing crossfade start:** the phrase boundary in `phrase_boundaries(outgoing, 16)` nearest to `outgoing.outro_start_seconds`. Can shift the start by up to 8 bars in either direction.
- **Incoming crossfade start:** the FIRST entry of `phrase_boundaries(incoming, 16)`, falling back to `incoming.intro_end_seconds` if the list is empty. For most tracks these are identical.

The mixer:
- Streams the outgoing track until `samples_pushed >= (outgoing_crossfade_start - decoder.start) * SR * 2` (instead of using `outro_start_seconds` directly).
- Fetches the incoming mix window centered on `incoming_crossfade_start_seconds` (instead of `intro_end_seconds`).
- Spawns the new playing decoder at `incoming_crossfade_start_seconds + crossfade_length_seconds`.

### Edge cases (handled in `plan_transition`)

- **No phrase boundaries available:** `phrase_boundaries()` returns empty. Fall back to existing behavior: use `outro_start_seconds` / `intro_end_seconds` directly.
- **Snap would exceed `cue_out_seconds`:** snap to the previous phrase boundary instead.
- **Outgoing body < 16 bars:** skip phrase alignment for this transition only.

## Component 3 — Adaptive crossfade length

### Variants

| Variant | Length | Triggers | Uses EQ swap |
|---|---|---|---|
| Long blend | 32 bars | Δbpm ≤ 2% AND Δenergy ≤ 0.10 AND key Identical | Yes |
| Standard blend | 16 bars | Default | Yes |
| Quick cut | 8 bars | Δenergy > 0.20 OR key Relative/None | Yes |
| Hard cut | 0 bars | Δenergy > 0.35 OR incoming `is_mixable=false` | No (instant swap) |

### Decision logic — `transition.rs`

```rust
pub struct TransitionPlan {
    pub outgoing_crossfade_start_seconds: f64,
    pub incoming_crossfade_start_seconds: f64,
    pub length_bars: usize,           // 0 = hard cut
    pub use_eq_swap: bool,
}

pub fn plan_transition(outgoing: &Sidecar, incoming: &Sidecar) -> TransitionPlan;
```

Thresholds are named constants at the top of the module so they're easy to tune by ear later:

```rust
const HARD_CUT_ENERGY_DELTA: f64 = 0.35;
const QUICK_CUT_ENERGY_DELTA: f64 = 0.20;
const LONG_BLEND_BPM_DELTA: f64 = 0.02;
const LONG_BLEND_ENERGY_DELTA: f64 = 0.10;
```

### Hard cut implementation

A hard cut is not a crossfade — it's a sample-accurate splice on the next downbeat.

Steps inside `mixer.rs` when `plan.length_bars == 0`:
1. Continue streaming the outgoing decoder until we hit the next downbeat at or after `outgoing_crossfade_start_seconds`. Push samples up to that exact sample index.
2. Discard the rest of the outgoing decoder (kill the subprocess).
3. Push the first frame from the incoming pre-fetched buffer directly. No volume blending, no EQ math.
4. Spawn the new playing decoder at `incoming_crossfade_start_seconds`.

This relies on the phrase-snapping from Component 2 to land on a clean musical boundary — without it, hard cuts would feel arbitrary.

## How `mixer.rs` changes

Existing flow (5 steps):
1. Decode incoming mix window (stretched).
2. Stream outgoing until samples_pushed crosses outro_start.
3. Read crossfade-length frames from outgoing.
4. `mix_crossfade(outgoing_tail, incoming_head, frames)`.
5. Spawn new decoder at `intro_end`.

New flow:
1. `let plan = transition::plan_transition(&outgoing.sidecar, &incoming.sidecar);`
2. Decode incoming mix window stretched, centered on `plan.incoming_crossfade_start_seconds`.
3. Stream outgoing until `samples_pushed >= (plan.outgoing_crossfade_start_seconds - decoder.start_seconds) * SR * 2`.
4. Branch on `plan.length_bars`:
   - `0` → execute hard cut (Component 3): push to next downbeat, discard outgoing, splice incoming first frame.
   - `> 0` → read `length_bars * bar_seconds * SR` frames from outgoing.
5. Pick the mixing function:
   - `plan.use_eq_swap` → `mix_crossfade_eq(outgoing_tail, incoming_head, frames, SR)`
   - else → existing `mix_crossfade(...)`
6. Push mixed buffer to ring producer.
7. Spawn new playing decoder at `plan.incoming_crossfade_start_seconds + plan.length_bars * bar_seconds`.

The function signature stays:
```rust
pub async fn play_until_transition_and_crossfade(
    outgoing: &LoadedTrack,
    incoming: &LoadedTrack,
    decoder: &mut PlayingDecoder,
    producer: &mut SampleProducer,
) -> Result<PlayingDecoder>;
```

## Testing strategy

### `eq.rs`
- Lowpass kills high frequencies: 10 kHz sine through `Biquad::lowpass(44100, 200)` → output RMS < 1% of input.
- Lowpass passes low frequencies: 100 Hz sine through same filter → output RMS within ±0.5 dB of input.
- Highpass mirrors: 10 kHz passes, 100 Hz killed for `Biquad::highpass(44100, 1000)`.
- Stereo independence: distinct L/R signals → no bleed.
- No NaN, no clip during a cutoff sweep on unit-amplitude noise.

### `phrase.rs`
- Anchors on `intro_end_seconds`: first boundary is the downbeat at/after intro_end.
- Spacing: returned boundaries are exactly `phrase_bars` indices apart in the downbeats list.
- Stops at `outro_start_seconds`.
- Short body returns empty.
- Custom `phrase_bars` argument respected.

### `transition.rs`
- Identical tracks → `length_bars = 32`, `use_eq_swap = true`.
- Big energy jump (Δ=0.5) → `length_bars = 0`, `use_eq_swap = false`.
- `is_mixable=false` incoming → hard cut regardless of other metrics.
- Default similar-but-not-identical → `length_bars = 16`.
- Outgoing snapping: `outro_start=40.5`, phrase boundaries `[8, 24, 40, 56]` → plan picks 40.0.
- Snap past `cue_out` → snap to previous boundary.

### `crossfade.rs::mix_crossfade_eq`
- Integration: 440 Hz outgoing + 523 Hz incoming, EQ-swap enabled → both frequencies in mid-region; bass band (50-200 Hz) shows the crossover.
- Existing `mix_crossfade()` tests still pass (regression).

### `mixer.rs`
- Hard-cut variant: craft `is_mixable=false` incoming → output buffer has the sharp cut at the expected sample, no overlap region.
- Phrase-snap: craft sidecars with known downbeats, intercept the outgoing decoder's read calls → confirm streaming stops at the snapped time, not the raw `outro_start_seconds`.

### End-to-end smoke (manual)
Run engine against `data/` for ~10 minutes via VLC. Listen for:
- Mostly 16-bar blends.
- At least one long blend (similar tracks).
- At least one hard cut (energy/key jump).
- No two basslines fighting (EQ swap working).
- Transitions land on phrase boundaries (subjective).

## Project layout — additions

```
DJ-Engine/
  engine/
    src/
      eq.rs                    # NEW
      phrase.rs                # NEW
      transition.rs            # NEW
      crossfade.rs             # MODIFIED — adds mix_crossfade_eq
      mixer.rs                 # MODIFIED — uses TransitionPlan
      lib.rs                   # MODIFIED — declare new modules
    tests/
      eq_test.rs               # NEW
      phrase_test.rs           # NEW
      transition_test.rs       # NEW
      crossfade_eq_test.rs     # NEW
      mixer_integration_test.rs # MODIFIED — add hard-cut + phrase-snap cases
  docs/superpowers/specs/
    2026-05-27-mixer-dj-feel-design.md   # THIS DOC
  docs/superpowers/plans/
    2026-05-27-mixer-dj-feel.md          # FUTURE — implementation plan
```

## Open questions / future work

- **Mid-band EQ handling.** Starting with 2-band (lows + highs). If transitions still sound congested in the 200 Hz–2 kHz region, add a band-pass with its own crossover curve.
- **Threshold tuning.** The thresholds in `transition.rs` are starting values from standard DJ practice. After listening to a few hours of output we'll adjust by ear.
- **Filter resonance.** Q = 0.707 (Butterworth, flat). If we ever want a more "DJ-style" filter sweep (with audible resonance peaks), bump Q and expose it.
- **Genre-aware transitions.** Not in v1. If we want it later, sidecars would need a genre tag (analyzer change).
- **Beat-repeat / loop-roll on hard cuts.** A real DJ might loop the last bar of outgoing once before the hard cut for tension. Easy follow-up.
