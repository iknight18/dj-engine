# RhythmHandoff Recipe — Design

**Date:** 2026-05-29
**Status:** Draft, pending user review

## Goal

A new stems-aware transition recipe that hands off the **rhythm section first** (incoming's drums + bass replace outgoing's drums + bass during the layering window) while keeping outgoing's **melodic content** (vocals + other) audible on top. The melody itself hands off at the next phrase boundary. The listener hears one melody at a time, never two competing — so vocal clashes and key clashes during the layering window are eliminated by construction.

## Motivation

The existing `BassSwap` recipe layers incoming's vocals/highs/mids OVER outgoing during the bass-killed window. On non-instrumental tracks this produces a perceptible vocal clash and key clash — the listener hears two melodies fighting until the swap moment. Real DJs avoid this by layering the rhythm section UNDER the existing melody:

1. Outgoing keeps singing/playing melody.
2. The drums and bass have already become the new track's.
3. At a phrase boundary, the melody hands off cleanly — and by then the new groove is already established, so the melody handoff feels like an arrival rather than a forced cut.

This recipe encodes that gesture in the engine. It complements `BassSwap` rather than replacing it — `BassSwap` is still appropriate for instrumental tracks where the "vocals on top" layering doesn't create clashes.

## Scope

### Delivers

- One new recipe `RhythmHandoff` registered in the `RecipeChoice` enum and selectable by the picker.
- Score function that prefers `RhythmHandoff` when: stems available, both tracks phrase-aligned, transition length ≥ 8 bars, both tracks have drum-heavy outro/intro respectively (drum_density mean ≥ 0.5), and energy delta is moderate (|delta| < 0.20).
- Recipe phases that:
  1. Cross-fade outgoing's drums + bass DOWN over the first bar (1-bar Linear ramp `1→0`) while incoming's drums + bass STEP UP to 1 at bar 0.
  2. Hold incoming's vocals + other at 0 for the entire layering window.
  3. At bar `phrase_handoff_bar` (default bar 8 — first 8-bar phrase boundary), Step outgoing's vocals + other to 0 and incoming's vocals + other to 1.
- New script/recipe tests for the build_script + score.

### Out of scope

- **Stem-aware loop buffer.** This recipe plays incoming's drums+bass naturally from the cued source position — no loop. If incoming has a sparse drum intro, the gesture may underperform; a future `RhythmHandoffLooped` could capture and loop a high-density bar of incoming's drums+bass.
- **Variable layering window length.** Fixed at 8 bars for v1. The picker only enables it when `plan.length_bars >= 8`. Longer windows could be a later variant.
- **Outgoing fader changes.** Outgoing's mixed fader stays at 1 throughout. The handoff is entirely via stem faders. The post-handoff state has outgoing's four stems all at 0, so outgoing is effectively silent regardless of the mixed fader.

## Architecture

This recipe uses the existing `Phase::StemFader` infrastructure (added in Phase D). No new automation primitive is required.

### Phase composition

Where `L = plan.length_bars` and `handoff = 8.0`. (For v1, `L >= 8` and the handoff is at bar 8; recipe is not selected when `L < 8`.)

```
Phase::TempoAlign { deck: Incoming, ratio }

# Outgoing rhythm section dies over the first bar.
Phase::StemFader { deck: Outgoing, stem: Drums, from: 1.0, to: 0.0, span: bars(0.0, 1.0), curve: Linear }
Phase::StemFader { deck: Outgoing, stem: Bass,  from: 1.0, to: 0.0, span: bars(0.0, 1.0), curve: Linear }

# Incoming rhythm section steps in at bar 0.
Phase::Fader     { deck: Incoming, from: 1.0, to: 1.0, span: bars(0.0, 0.0), curve: Step }   # mixed fader at 1
Phase::StemFader { deck: Incoming, stem: Drums, from: 1.0, to: 1.0, span: bars(0.0, 0.0), curve: Step }
Phase::StemFader { deck: Incoming, stem: Bass,  from: 1.0, to: 1.0, span: bars(0.0, 0.0), curve: Step }

# Incoming melody held off until phrase handoff.
Phase::StemFader { deck: Incoming, stem: Vocals, from: 0.0, to: 0.0, span: bars(0.0, 0.0), curve: Step }
Phase::StemFader { deck: Incoming, stem: Other,  from: 0.0, to: 0.0, span: bars(0.0, 0.0), curve: Step }

# Phrase handoff at bar 8: outgoing melody dies, incoming melody enters.
Phase::StemFader { deck: Outgoing, stem: Vocals, from: 0.0, to: 0.0, span: bars(handoff, handoff), curve: Step }
Phase::StemFader { deck: Outgoing, stem: Other,  from: 0.0, to: 0.0, span: bars(handoff, handoff), curve: Step }
Phase::StemFader { deck: Incoming, stem: Vocals, from: 1.0, to: 1.0, span: bars(handoff, handoff), curve: Step }
Phase::StemFader { deck: Incoming, stem: Other,  from: 1.0, to: 1.0, span: bars(handoff, handoff), curve: Step }
```

Two notes on this composition:

- **The outgoing Vocals/Other steps go from 0→0 at the handoff bar.** That's a no-op semantically — outgoing's vocals/other were never automated, so they're at their initial value of 1.0 the whole time. To actually mute them, we need to Step them to 0. The correct phases:
  ```
  Phase::StemFader { deck: Outgoing, stem: Vocals, from: 0.0, to: 0.0, span: bars(handoff, handoff), curve: Step }
  ```
  In our DSL, a Step with `from=to=0` compiles to an Automation with `from=0, to=0` — which sets the value to 0 at the step instant. The "from" value is ignored for a Step at its endpoint. So Step with `value=0` mutes. Confirmed by `Phase::EqStep` semantics elsewhere in the recipe catalog.

- **The mixed fader for incoming is explicitly Step to 1 at bar 0.** This is critical because `cue_stems` (the stems-mode entry point) leaves `state.fader = 0.0` after cueing — the scheduler relies on the recipe to bring the fader up. Without this Step the entire incoming deck stays silent.

### Stem-fader state by phase

| Time | OUT.drums | OUT.bass | OUT.vocals | OUT.other | IN.drums | IN.bass | IN.vocals | IN.other |
|---|---|---|---|---|---|---|---|---|
| pre-cue | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| post-cue (incoming just cued) | 1 | 1 | 1 | 1 | 1 | 1 | 1 | 1 |
| bar 0 (Steps apply) | 1→0 (ramp) | 1→0 (ramp) | 1 | 1 | 1 | 1 | 0 | 0 |
| bar 1 (ramp done) | 0 | 0 | 1 | 1 | 1 | 1 | 0 | 0 |
| bars 1..8 (layering) | 0 | 0 | 1 | 1 | 1 | 1 | 0 | 0 |
| bar 8 (handoff) | 0 | 0 | 0 | 0 | 1 | 1 | 1 | 1 |
| post-transition | 0 | 0 | 0 | 0 | 1 | 1 | 1 | 1 |

What the listener hears:
- bar 0: outgoing full mix with rhythm fading + incoming drums+bass entering
- bars 1–7: outgoing vocals/melody + incoming drums+bass (rhythmic foundation has switched)
- bar 8+: incoming full mix

## Components Touched

### Engine

- `engine/src/recipe.rs`:
  - Add `pub struct RhythmHandoff;` with `Recipe` impl: `name() = "rhythm_handoff"`, `requires_stems() = true`, `build_script` per the phase composition above.
  - Add `RecipeChoice::RhythmHandoff` variant. Update `recipe_choice_id()` to assign a stable id (17).
  - Update the `candidates` list inside `pick_recipe` and `pick_recipe_with_scores` to include `RhythmHandoff`.
  - Add the score branch in `score_recipe`. Suggested scoring:
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
  - Extend `PairScoreInputs` with `outgoing_drum_density_mean: f64` and `incoming_drum_density_mean: f64` (derived from `sidecar.stems.drum_density.iter().sum::<f64>() / N`). The current struct already has `outgoing_bass` and `incoming_bass` so the pattern is established.

### Tests

- `engine/tests/script_test.rs`: add `rhythm_handoff_emits_expected_stem_phases()` verifying the phase list shape (drums/bass ramps + steps, vocals/other holds, handoff steps).
- `engine/tests/recipe_test.rs`: add scoring tests covering: drums-heavy + phrase-aligned + L>=8 → 0.88; drums-light → 0.05; stems-unavailable → 0.05.
- `engine/tests/compile_test.rs`: smoke test that compiling a `RhythmHandoff` script produces the expected automation count and target distribution.

### No analyzer changes

This recipe consumes only existing sidecar fields (`stems.drum_density`, BPM, downbeats, energy_mean). No new analyzer outputs required.

## Error Handling

- **Stems not available on either deck** → score is 0.05; recipe will not be picked over any meaningfully-scoring alternative. Defense in depth: `requires_stems() = true` makes the scheduler refuse to run the recipe without stems.
- **Length < 8 bars** → score drops to 0.05 (no fallback within the recipe — the picker chooses a different recipe).
- **drum_density data absent on a sidecar** → treated as 0.0, score collapses to 0.05.

## Testing

### Unit tests (engine)

- `rhythm_handoff_score_drums_heavy_and_aligned_returns_high`: confirms 0.88 in the strong case.
- `rhythm_handoff_score_drums_light_returns_floor`: confirms 0.05 when drum densities are low.
- `rhythm_handoff_score_short_length_returns_floor`: confirms 0.05 when length_bars < 8.
- `rhythm_handoff_score_no_stems_returns_floor`: confirms 0.05 when stems_available = false.
- `rhythm_handoff_script_phases_present`: walks the compiled output, verifies expected automation count per deck and per StemKind, and that the handoff Steps land at bar 8.
- `rhythm_handoff_requires_stems_is_true`: ensures the scheduler refuses without stems.

### Live verification (post-implementation)

1. With the engine running on the new sidecars, force the recipe on a known-good pair: `--force-recipe rhythm-handoff` (note: clap kebab-cases enum variants). Confirm the engine log emits `recipe="rhythm_handoff"` and the transition completes.
2. Listen for the layering: should hear outgoing vocals/melody clearly with incoming drums+bass underneath, never two melodies competing.
3. Listen for the handoff at bar 8: the melody should hand over cleanly on the downbeat.

## Open Items For Implementation

- Verify the exact field name on `Sidecar::stems::drum_density` (existing).
- Confirm the engine's `--force-recipe` flag accepts the kebab-case `rhythm-handoff` (or adjust the spec).
- Decide whether to also emit `outgoing.fader -> 0` at the handoff bar (defensive — if all stems are 0 the source already produces zeros, but explicitly muting the fader makes downstream logging/visualisation clearer). Leaning yes.
