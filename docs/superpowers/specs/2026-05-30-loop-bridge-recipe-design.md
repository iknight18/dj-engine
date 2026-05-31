# LoopBridge Recipe — Design

**Date:** 2026-05-30
**Status:** Draft, pending user review
**Spec depends on:** existing analyzer 0.5.0 sidecars (vocal_segments, phrase_boundaries_seconds, downbeats_seconds, band_energy_curves, key); existing engine infrastructure (LoopBuffer, Filter, StemFader*, LoopGate automation).

## Goal

Three-act stems-aware transition where incoming's pre-vocal rhythm is captured into a loop, filter-crossfaded against outgoing's continuing vocal phrase, then released right at outgoing's last phrase end so incoming's vocals enter naturally on the downbeat. The listener experiences:

- Outgoing's vocal phrase continues uninterrupted.
- A new rhythm builds underneath via incoming's loop, revealed through a filter sweep.
- At outgoing's phrase end, incoming's vocals tag in seamlessly — the loop releases and incoming's source plays forward into its own vocal entry.

The gesture is "two vocal phrases handed off across a rhythmic bridge." It only fires when both decks' structural data lines up.

## Motivation

The recipe-catalog consolidation produced 7 recipes. Of those, the filter-based ones (`filter_fade`, `filter_kill`) work musically for this listener; the layered-melody recipes (vocal-overlay, bass-swap variants) do not. `RhythmSwap` is the closest existing equivalent but it doesn't use a loop or anchor on outgoing's phrase end.

A real DJ does a more theatrical version: when two compatible tracks (same key, similar groove) need a "moment" transition, they:
1. Cue incoming on a rhythm-only intro just before vocals enter.
2. Bring incoming in via a loop, often with a high-pass filter making it shimmery at first.
3. Let outgoing's last vocal phrase finish playing on top of the rising loop.
4. At the end of outgoing's phrase, release the loop so incoming's vocals enter — the listener perceives a clean handoff between two singers/themes.

This recipe encodes that gesture. It's a "specialist" — fires rarely (5–10% of transitions) but lands with theatrical impact when conditions match.

## Scope

### Delivers

- New `LoopBridge` recipe struct + Recipe impl in `engine/src/recipe.rs`.
- `RecipeChoice::LoopBridge` variant; registered in picker (candidates, recipe_choice_id, recency_hash, dispatch, clap ForceRecipe enum).
- `score_recipe` arm: 0.90 when all conditions match, 0.05 otherwise.
- 3 new `Sidecar` helpers in `engine/src/sidecar.rs`:
  - `last_phrase_boundary_at_or_before(seconds: f64) -> Option<f64>`
  - `last_vocal_segment_end_at_or_before(seconds: f64) -> Option<f64>`
  - `snap_to_nearest_downbeat_at_or_before(seconds: f64) -> Option<f64>`
- Key-compatibility helper in `engine/src/camelot.rs` (or new module): `keys_compatible(a: &str, b: &str) -> bool` (matches Camelot wheel — same key, ±1 on N, or opposite letter at same N).
- Scheduler enhancement: log new fields at transition arming (`loop_lock_bar`, `loop_lock_source_seconds`, `phrase_end_bar`, `phrase_end_source_seconds`, `computed_length_bars`, `key_compatibility`).
- Tests: unit tests for new helpers; recipe build_script test; scoring tests for high-score and low-score conditions; key compatibility tests.

### Out of scope (deferred)

- **Per-bar LUFS** for fully data-driven loudness compensation. Hand-tuned fader curves used in v1; band-energy-aware compensation is a follow-up if hand-tuning proves insufficient.
- **Override of planner's incoming cue point** to land exactly on a rhythm-only section. v1 uses the planner's default cue; recipe scores 0 if it doesn't give enough pre-vocal headroom.
- **`loop_bars` adaptation** to the actual phrase length. v1 hardcodes 4 bars.

## Architecture

### Three-act timeline

Let `L = ctx.plan.length_bars` (initially what the planner picked) but the recipe **overrides** it via `TransitionScript.length_bars = computed_length_bars`. Let `cue_start = plan.incoming_crossfade_start_seconds`. Let `bar_seconds = (60.0 / outgoing_bpm) * 4.0`.

**Computed anchor bars (recipe-local time):**

- `phrase_end_seconds` (in outgoing's source time):
  ```
  let p_boundary = outgoing.last_phrase_boundary_at_or_before(outro_start + bar_seconds);
  let v_end      = outgoing.last_vocal_segment_end_at_or_before(outro_start + 4 * bar_seconds + bar_seconds);
  let phrase_end = max(p_boundary, v_end).unwrap_or(0.0);
  ```
  The vocal-end check is a safety net so a phrase boundary that falls mid-vocal doesn't chop a vocal note.

- `phrase_end_bar = (phrase_end_seconds - plan.outgoing_crossfade_start_seconds) / bar_seconds`

- `vocal_entry_seconds` (in incoming's source time) = `incoming.vocal_segments[0].start`
- `vocal_entry_snapped = incoming.snap_to_nearest_downbeat_at_or_before(vocal_entry_seconds)`
- `loop_lock_bar = (vocal_entry_snapped - cue_start) / bar_seconds` (in transition-local time)

- `computed_length_bars = ceil(phrase_end_bar)` — sets the transition's length.

**Recipe phases:**

```rust
phases: vec![
    Phase::TempoAlign { deck: Incoming, ratio },

    // Outgoing's mixed fader stays at 1.0 throughout Act 1 + Act 2; reduces
    // for loudness compensation during Act 2; hard-cut at Act 3 (phrase_end).
    Phase::Fader {
        deck: Outgoing, from: 1.0, to: 0.55,
        span: bars(loop_lock_bar, phrase_end_bar - 1.0),
        curve: Curve::Linear,
    },
    Phase::Fader {
        deck: Outgoing, from: 0.0, to: 0.0,
        span: bars(phrase_end_bar, phrase_end_bar),
        curve: Curve::Step,
    },

    // Incoming's mixed fader: 0 during Act 1 (silent capture); 0→0.85 over the
    // first 4 bars of Act 2 (loudness-compensated rise — never reaches 1.0
    // during Act 2 so the total doesn't bump); Step→1.0 at Act 3.
    Phase::Fader {
        deck: Incoming, from: 0.0, to: 0.0,
        span: bars(0.0, 0.0),
        curve: Curve::Step,
    },
    Phase::Fader {
        deck: Incoming, from: 0.0, to: 0.85,
        span: bars(loop_lock_bar, loop_lock_bar + 4.0),
        curve: Curve::EqualPowerIn,
    },
    Phase::Fader {
        deck: Incoming, from: 1.0, to: 1.0,
        span: bars(phrase_end_bar, phrase_end_bar),
        curve: Curve::Step,
    },

    // ====== Act 1 — Loop preparation, bars [0, loop_lock_bar] ======
    // Incoming's stems isolated to rhythm-only so the loop buffer captures
    // rhythm-only content even if the underlying audio has pre-vocal melodic
    // pads or synths.
    Phase::StemFader { deck: Incoming, stem: Drums,  from: 1.0, to: 1.0, span: bars(0.0, 0.0), curve: Curve::Step },
    Phase::StemFader { deck: Incoming, stem: Bass,   from: 1.0, to: 1.0, span: bars(0.0, 0.0), curve: Curve::Step },
    Phase::StemFader { deck: Incoming, stem: Vocals, from: 0.0, to: 0.0, span: bars(0.0, 0.0), curve: Curve::Step },
    Phase::StemFader { deck: Incoming, stem: Other,  from: 0.0, to: 0.0, span: bars(0.0, 0.0), curve: Curve::Step },

    // LoopGate ON at loop_lock_bar — freezes incoming's source position,
    // loop buffer holds the last 4 bars of pre-vocal rhythm.
    Phase::LoopGate {
        deck: Incoming,
        on_at: bar(loop_lock_bar),
        off_at: bar(phrase_end_bar),
    },

    // ====== Act 2 — Filter crossfade, bars [loop_lock_bar, phrase_end_bar - 1] ======
    // Incoming filter sweep — the "rising rhythm" reveal (filter-fade gesture
    // applied to the looped rhythm).
    Phase::Filter {
        deck: Incoming, from: 0.85, to: 0.0,
        span: bars(loop_lock_bar, phrase_end_bar - 1.0),
        curve: Curve::Linear,
    },

    // Outgoing filter sweep — gentle thinning. Vocals (mid + high) ring through;
    // kick and bass step out of the way.
    Phase::Filter {
        deck: Outgoing, from: 0.0, to: 0.6,
        span: bars(loop_lock_bar, phrase_end_bar - 1.0),
        curve: Curve::Linear,
    },

    // ====== Act 3 — Vocal handoff at phrase_end_bar ======
    // LoopGate OFF — already encoded above via on_at/off_at.
    // Stem fader restoration (vocals + other back on).
    Phase::StemFader { deck: Incoming, stem: Vocals, from: 1.0, to: 1.0, span: bars(phrase_end_bar, phrase_end_bar), curve: Curve::Step },
    Phase::StemFader { deck: Incoming, stem: Other,  from: 1.0, to: 1.0, span: bars(phrase_end_bar, phrase_end_bar), curve: Curve::Step },

    // Filter reset on both decks (cleanup; outgoing is silent anyway).
    Phase::Filter { deck: Outgoing, from: 0.0, to: 0.0, span: bars(phrase_end_bar, phrase_end_bar), curve: Curve::Step },
    Phase::Filter { deck: Incoming, from: 0.0, to: 0.0, span: bars(phrase_end_bar, phrase_end_bar), curve: Curve::Step },
],
outgoing_init: None,
incoming_init: Some(DeckInitConfig {
    echo_delay_seconds: None,
    echo_feedback: None,
    loop_bars: Some(4),
    loop_bar_seconds: Some(bar_seconds),
}),
```

### Stem-fader and fader state by phase (illustrative L=20, loop_lock_bar=6, phrase_end_bar=18)

| Time | OUT.fader | OUT.filter | IN.fader | IN.filter | IN.drums | IN.bass | IN.vocals | IN.other | LoopGate(IN) |
|---|---|---|---|---|---|---|---|---|---|
| pre-cue | 1.0 | 0.0 | 0.0 | 0.0 | 1.0 | 1.0 | 1.0 | 1.0 | OFF |
| bar 0 | 1.0 | 0.0 | 0.0 | 0.0 | 1.0 | 1.0 | 0.0 | 0.0 | OFF |
| bars 0–6 (Act 1) | 1.0 | 0.0 | 0.0 (silent) | 0.0 | 1.0 | 1.0 | 0.0 | 0.0 | OFF |
| bar 6 (LoopGate ON, Act 2 begins) | 1.0 | 0.0 | 0.0 (ramping) | 0.85 | (irrelevant, loop active) | | | | ON |
| bar 10 (mid-Act 2) | 0.78 | 0.30 | 0.85 | 0.45 | (loop) | | | | ON |
| bar 17 (end of Act 2 ramps) | 0.55 | 0.6 | 0.85 | 0.0 | (loop) | | | | ON |
| bar 18 (Act 3 fires) | 0.0 | 0.0 | 1.0 | 0.0 | 1.0 | 1.0 | 1.0 | 1.0 | OFF |
| bar 18+ | 0.0 | — | 1.0 | 0.0 | 1.0 | 1.0 | 1.0 | 1.0 | OFF (incoming source resumes from vocal_entry_snapped) |

What the listener hears:
- **Bars 0–6:** outgoing's full mix only. Incoming is silent (capturing). Listener doesn't know a transition is starting — total subtlety.
- **Bar 6:** loop locks. Incoming starts to enter at filter 0.85 (treble-only sparkle), fader rising. The first hint of new groove.
- **Bars 6–10:** incoming's loop rising through the spectrum (filter opening). Outgoing slightly thinning (filter 0.3). Both contributing.
- **Bars 10–17:** incoming at full filter, outgoing thinned to 55% fader at 0.6 filter. Both at ~equal perceived loudness; outgoing's vocals still on top.
- **Bar 18 (the moment):** loop releases, outgoing cuts out, incoming jumps to full and resumes source playback → vocals enter on the downbeat. Listener experiences a perfect handoff between two vocal moments.

## Loudness Compensation (v1 — hand-tuned)

The numbers in the spec (outgoing fader 1.0→0.55; incoming fader 0.0→0.85; incoming filter 0.85→0.0; outgoing filter 0.0→0.6) are first-pass estimates. After implementation:

1. Listen to a few LoopBridge transitions.
2. Note any perceptual loudness bumps or dips.
3. Adjust the fader-curve endpoints accordingly.
4. Document the chosen values in the recipe's doc-comment.

If hand-tuning proves insufficient (e.g., compensation differs significantly per track due to band-energy variation), a follow-up project adds band-energy-aware compensation using the existing `band_energy_curves` field — no analyzer changes needed.

A full per-bar LUFS project (Phase E) is the most-correct path but not required for v1.

## Picker Scoring

```rust
RecipeChoice::LoopBridge => {
    let Some(out_sc) = inputs.outgoing_sidecar else { return 0.05; };
    let Some(in_sc) = inputs.incoming_sidecar else { return 0.05; };

    if !inputs.stems_available || !inputs.phrase_aligned {
        return 0.05;
    }

    let bar_seconds = (60.0 / out_sc.bpm.unwrap_or(120.0)) * 4.0;
    let outro_start = out_sc.outro_start_seconds.unwrap_or(0.0);
    let cue_start = inputs.incoming_crossfade_start_seconds;

    // Phrase boundary anchor
    let Some(phrase_end_seconds) = out_sc.last_phrase_boundary_at_or_before(outro_start + bar_seconds) else { return 0.05; };

    // Incoming vocal entry available
    let Some(vocal_entry) = in_sc.stems.as_ref()
        .and_then(|s| s.vocal_segments.first().map(|seg| seg[0])) else { return 0.05; };

    // Snap to downbeat
    let Some(vocal_entry_snapped) = in_sc.snap_to_nearest_downbeat_at_or_before(vocal_entry) else { return 0.05; };

    let loop_lock_bar = (vocal_entry_snapped - cue_start) / bar_seconds;
    let phrase_end_bar = (phrase_end_seconds - inputs.outgoing_crossfade_start_seconds) / bar_seconds;
    let computed_length = phrase_end_bar.ceil();

    // Conditions for high score
    let loop_lock_ok = loop_lock_bar >= 4.0;
    let act2_long_enough = (phrase_end_bar - loop_lock_bar) >= 8.0;
    let length_in_range = computed_length >= 16.0 && computed_length <= 32.0;
    let keys_compat = crate::camelot::keys_compatible(
        out_sc.key.as_deref().unwrap_or(""),
        in_sc.key.as_deref().unwrap_or(""),
    );

    if loop_lock_ok && act2_long_enough && length_in_range && keys_compat {
        0.90
    } else {
        0.05
    }
}
```

Note: `PairScoreInputs` will need to expose `outgoing_sidecar`, `incoming_sidecar`, and `incoming_crossfade_start_seconds` for this to work. Currently it only carries derived primitives (energy_delta, outgoing_bass, etc.). The scoring function needs broader access for the LoopBridge — extending `PairScoreInputs` to hold optional references is the cleanest path.

**Alternative scoring signature:** instead of extending `PairScoreInputs`, accept a context object that includes the sidecars + plan. The recipe-scoring infrastructure may need a small refactor for this.

## Camelot Key Compatibility

```rust
// camelot.rs
pub fn keys_compatible(a: &str, b: &str) -> bool {
    let Some((a_n, a_letter)) = parse_camelot(a) else { return false; };
    let Some((b_n, b_letter)) = parse_camelot(b) else { return false; };

    // Same key
    if a_n == b_n && a_letter == b_letter { return true; }
    // ±1 on the wheel, same letter
    let diff = ((a_n - b_n + 12) % 12).min((b_n - a_n + 12) % 12);
    if diff == 1 && a_letter == b_letter { return true; }
    // Same N, opposite letter (relative major/minor)
    if a_n == b_n && a_letter != b_letter { return true; }
    false
}

fn parse_camelot(key: &str) -> Option<(i32, char)> {
    // e.g., "8A" -> (8, 'A')
    let n_str: String = key.chars().take_while(|c| c.is_ascii_digit()).collect();
    let letter = key.chars().last()?;
    if letter != 'A' && letter != 'B' { return None; }
    let n: i32 = n_str.parse().ok()?;
    if !(1..=12).contains(&n) { return None; }
    Some((n, letter))
}
```

## New Sidecar Helpers

```rust
impl Sidecar {
    /// Find the last phrase_boundary at or before `seconds`. Returns None when
    /// phrase_boundaries_seconds is empty or all entries exceed `seconds`.
    pub fn last_phrase_boundary_at_or_before(&self, seconds: f64) -> Option<f64> {
        self.phrase_boundaries_seconds.as_ref()?
            .iter().rev().find(|&&t| t <= seconds).copied()
    }

    /// Find the last vocal_segment's end at or before `seconds`. Returns None
    /// when stems or vocal_segments is empty.
    pub fn last_vocal_segment_end_at_or_before(&self, seconds: f64) -> Option<f64> {
        self.stems.as_ref()?.vocal_segments.iter()
            .map(|seg| seg[1])  // [start, end]
            .filter(|&end| end <= seconds)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Snap `seconds` to the nearest downbeat at or before it.
    pub fn snap_to_nearest_downbeat_at_or_before(&self, seconds: f64) -> Option<f64> {
        self.downbeats_seconds.iter().rev()
            .find(|&&t| t <= seconds).copied()
    }
}
```

## Observability

The `transition arming` log gains these fields when the recipe is `loop_bridge`:
- `loop_lock_bar` — the bar (transition-local) where the loop locks
- `loop_lock_source_seconds` — incoming source-time at lock
- `phrase_end_bar` — the bar where Act 3 fires
- `phrase_end_source_seconds` — outgoing source-time at the cut
- `computed_length_bars` — the recipe's chosen length
- `key_compatibility` — bool indicating outgoing and incoming key compat

These are recipe-specific; the existing log already emits common fields.

## Components Touched

### Engine source

- `engine/src/sidecar.rs` — 3 new helpers (above).
- `engine/src/camelot.rs` (or new module) — `keys_compatible` + `parse_camelot`.
- `engine/src/recipe.rs`:
  - `LoopBridge` struct + Recipe impl with three-act build_script.
  - `RecipeChoice::LoopBridge` variant.
  - `recipe_choice_id`: add arm with ID 8 (after the consolidated 7).
  - Candidates arrays in `pick_recipe` and `pick_recipe_with_scores`.
  - `recency_hash` loop list.
  - `score_recipe`: new arm per spec above.
  - `PairScoreInputs` extension: add `outgoing_sidecar: &'a Sidecar`, `incoming_sidecar: &'a Sidecar`, `incoming_crossfade_start_seconds: f64` (so the scoring function has the data it needs).
- `engine/src/main.rs`:
  - Dispatch match arm for `RecipeChoice::LoopBridge`.
  - `recipe_name` / `recipe_choice_name` helpers.
  - Optional: extend `transition arming` log with the recipe-specific fields when recipe is `loop_bridge`.
- `engine/src/config.rs`:
  - `ForceRecipe::LoopBridge` variant.
  - `to_recipe_choice` mapping.

### Tests

- `engine/tests/sidecar_test.rs` — 3 new tests, one per helper (basic case + None case).
- `engine/tests/camelot_test.rs` — `keys_compatible` covers: same key, ±1, opposite letter, incompatible.
- `engine/tests/script_test.rs` — `loop_bridge_emits_three_act_phases` verifies all 3 acts present and on right deck.
- `engine/tests/recipe_test.rs` — scoring tests: full conditions met → 0.90; missing phrase boundary → 0.05; key incompatible → 0.05; loop_lock_bar < 4 → 0.05; computed_length < 16 → 0.05; instrumental incoming → 0.05.

## Error Handling

- **Missing sidecar fields (phrase_boundaries, vocal_segments, key)**: scoring returns 0.05. Recipe never fires.
- **Computed length out of range (< 16 or > 32)**: scoring returns 0.05.
- **Incoming.key or outgoing.key absent**: scoring returns 0.05.

The recipe is "all-or-nothing" — it requires the entire chain of sidecar data to align. Defensive scoring prevents the recipe from firing on bad data.

## Testing

### Unit tests

Per components-touched list above.

### Live verification

After implementation:
1. Rebuild engine, restart with sidecars.
2. Force-test: `--force-recipe loop-bridge` on a pair known to satisfy all conditions (compatible key, both having phrase boundaries near outro_start, incoming with vocal_segments well past cue point).
3. Listen for: subtle Act 1 (no audible change), filter sweep Act 2, clean handoff at Act 3.
4. Natural picker run: confirm LoopBridge fires occasionally (when conditions match), doesn't dominate.

### Tuning iteration

The loudness-compensation fader values (0.55, 0.85, etc.) are likely to need adjustment after listening. Document the chosen values in the recipe's doc-comment.

## Migration

- No sidecar re-analysis required. Uses existing analyzer 0.5.0 fields.
- Engine rebuild + restart.

## Rollback

- Revert the implementation commits. Catalog falls back to 7 recipes.
- No data migration; pure engine-side change.

## Open Items For Implementation

- Whether `PairScoreInputs` should hold sidecar references directly, or a new context object should be introduced. v1 picks the simpler refactor (extending the struct).
- Whether `keys_compatible` should be more permissive (e.g., ±2 on the wheel = "energy boost" but still musically tolerable for this gesture). v1 sticks to ±1.
- Whether `loop_bars = 4` is the right default. Some tracks have 2-bar grooves where 4 captures a too-long phrase. v1 hardcodes 4; v2 could adapt based on phrase length.
