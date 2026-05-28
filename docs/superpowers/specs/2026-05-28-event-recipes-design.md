# Event Recipes — Drop, BreakdownBridge, DoubleDrop

**Date:** 2026-05-28
**Status:** Approved for planning
**Branch:** `research/two-deck-mixer`
**Sub-project:** recipe-layer addition (no stems required)

## Summary

Add three "event" recipes that aren't crossfades — they introduce *gestures* with a hard cut on a phrase boundary, contrasting with the existing 10 recipes which all overlap A and B and gradually swap parameters. `Drop` (incoming plays high-pass-filtered for 4 bars, building, then hard-cuts to full bandwidth on the next downbeat). `BreakdownBridge` (outgoing's mids+bass are killed for ~4 bars, only its highs ring while incoming is silent, then incoming hard-cuts in at full volume). `DoubleDrop` (both decks fade to silence for half a bar, then both hard-cut in at full volume for the next bar before outgoing dies — a brief mash-up moment). Each recipe uses its own internal window length (4 / 4 / 2 bars respectively), independent of the planner's `length_bars`. No stems needed; uses the existing Fader / EqLow / EqMid / Filter / TempoAlign primitives.

## Motivation

User's qualitative listening feedback: the current mix "feels like a radio, songs just passing." Root cause: every existing recipe — including the new `ThreeBandSwap`, `EchoThrow`, `HighsFirst` — is a *variation on volume crossfade with EQ choreography*. They blend smoothly; they don't punctuate. Real DJ sets alternate smooth blends with *events*: a build-up that resolves on a hard cut, a breakdown into a re-entry, a synchronized double-drop. These create the "moments" that distinguish a DJ set from a radio playlist.

The cheapest way to add events is recipe-layer work using the per-deck primitives we already built (per-deck `Filter`, three-band EQ, `Fader`, `TempoAlign`). No new DSL phases needed. No stems needed. No new effects.

## Goals

- Three new recipes in `recipe.rs`: `Drop`, `BreakdownBridge`, `DoubleDrop`. Each implements `Recipe::build_script`.
- Each recipe **internally fixes its own window length** (`TransitionScript.length_bars`). The recipe ignores `ctx.plan.length_bars` for its phase positions. This is a documented departure from the existing recipes; the existing 10 all honor `plan.length_bars`.
- Three new variants in `RecipeChoice`: `Drop`, `BreakdownBridge`, `DoubleDrop`. Three new branches in `score_recipe`. Three new `recipe_choice_id` entries. Extended `recency_hash` iteration. Extended `pick_recipe` candidates list.
- Three new `--force-recipe` values: `drop`, `breakdown_bridge`, `double_drop`.
- Scoring is **conservative**: these recipes should fire occasionally, not dominate. Drop wins on big energy ramp-up with phrase alignment. BreakdownBridge wins on energy wind-down with phrase alignment. DoubleDrop wins ONLY when BPM is identical, keys are tightly compatible, and energy delta is small — a rare jackpot.
- Tests at three layers: phase shape (script_test.rs), scoring (recipe_test.rs), integration (no changes needed — existing mixer tests catch regressions).

## Non-Goals

- **No new DSL phases.** All three recipes compile from the existing `Phase` variants (`Fader`, `EqRamp`, `EqStep`, `Filter`, `TempoAlign`).
- **No stems usage.** Phase D recipes (after stems analytics complete) will use vocal segments / drum density to time these recipes against actual structural features. This spec is the "no-stems version" that ships first.
- **No planner changes.** The planner still picks `length_bars` from {0, 8, 16, 32} for *other* recipes. The new recipes ignore the planner's length and use their own internal lengths.
- **No selector changes.** The new recipes participate in the existing picker via score; they don't get preferential track selection.
- **No outro-deadline arithmetic changes.** The planner's `outgoing_outro_start` is used as before; the new recipes just consume fewer bars after that point.

## Recipe specifications

For all three: `outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0)`, `incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm)`, `ratio = outgoing_bpm / incoming_bpm.max(1.0)`. `Curve` references are from `crate::automation::Curve`.

### `Drop` — incoming filtered high-pass build-up, hard-cut release on bar 4

Internal length: **`L = 4` bars**.

Phases:

```rust
let l = 4.0;
TransitionScript {
    length_bars: 4,
    phases: vec![
        Phase::TempoAlign { deck: DeckRole::Incoming, ratio },

        // Outgoing plays untouched for 4 bars, then hard-cuts.
        Phase::Fader {
            deck: DeckRole::Outgoing,
            from: 1.0, to: 1.0,
            span: bars(0.0, l - 0.001),  // held high
            curve: Curve::Step,
        },
        Phase::Fader {
            deck: DeckRole::Outgoing,
            from: 1.0, to: 0.0,
            span: bars(l, l),  // 1-frame step at bar 4
            curve: Curve::Step,
        },

        // Incoming fades in slightly during the build, hidden by HP filter.
        // Fader at 0.7 during the build so the HP-filtered treble is audible
        // but doesn't overwhelm outgoing. Steps to 1.0 at the drop.
        Phase::Fader {
            deck: DeckRole::Incoming,
            from: 0.7, to: 0.7,
            span: bars(0.0, l - 0.001),
            curve: Curve::Step,
        },
        Phase::Fader {
            deck: DeckRole::Incoming,
            from: 0.7, to: 1.0,
            span: bars(l, l),
            curve: Curve::Step,
        },

        // Initialize incoming filter knob to 0.85 (mostly HP — treble only).
        Phase::Filter {
            deck: DeckRole::Incoming,
            from: 0.85, to: 0.85,
            span: bars(0.0, 0.0),
            curve: Curve::Step,
        },
        // Build-up: filter sweeps from 0.85 down to 0.55 over 4 bars.
        // Listener hears the mid-band gradually entering — classic riser feel.
        Phase::Filter {
            deck: DeckRole::Incoming,
            from: 0.85, to: 0.55,
            span: bars(0.0, l),
            curve: Curve::Linear,
        },
        // Drop: filter snaps to 0.0 (full bandwidth) on bar 4.
        Phase::Filter {
            deck: DeckRole::Incoming,
            from: 0.0, to: 0.0,
            span: bars(l, l),
            curve: Curve::Step,
        },
    ],
    outgoing_init: None,
    incoming_init: None,
}
```

### `BreakdownBridge` — kill outgoing's mids+bass for 4 bars, then incoming hard-cuts in

Internal length: **`L = 4` bars**.

Phases:

```rust
let l = 4.0;
TransitionScript {
    length_bars: 4,
    phases: vec![
        Phase::TempoAlign { deck: DeckRole::Incoming, ratio },

        // Strip outgoing's mids and bass over the first 3 bars (the breakdown).
        // Highs stay — the listener hears the song "thinning out".
        Phase::EqRamp {
            deck: DeckRole::Outgoing,
            band: EqBand::Low,
            from_db: 0.0, to_db: -40.0,
            span: bars(0.0, 3.0),
            curve: Curve::Linear,
        },
        Phase::EqRamp {
            deck: DeckRole::Outgoing,
            band: EqBand::Mid,
            from_db: 0.0, to_db: -40.0,
            span: bars(0.0, 3.0),
            curve: Curve::Linear,
        },

        // Outgoing fader stays at full until bar 4, then hard-cuts.
        Phase::Fader {
            deck: DeckRole::Outgoing,
            from: 1.0, to: 0.0,
            span: bars(l, l),
            curve: Curve::Step,
        },

        // Incoming silent for 4 bars, then hard-cuts in at full volume.
        Phase::Fader {
            deck: DeckRole::Incoming,
            from: 0.0, to: 0.0,
            span: bars(0.0, l - 0.001),
            curve: Curve::Step,
        },
        Phase::Fader {
            deck: DeckRole::Incoming,
            from: 0.0, to: 1.0,
            span: bars(l, l),
            curve: Curve::Step,
        },
    ],
    outgoing_init: None,
    incoming_init: None,
}
```

### `DoubleDrop` — synchronized silence + dual hard-cut on the downbeat

Internal length: **`L = 2` bars**.

Phases:

```rust
let l = 2.0;
TransitionScript {
    length_bars: 2,
    phases: vec![
        Phase::TempoAlign { deck: DeckRole::Incoming, ratio },

        // Outgoing fades to silence over the first half bar.
        Phase::Fader {
            deck: DeckRole::Outgoing,
            from: 1.0, to: 0.0,
            span: bars(0.0, 0.5),
            curve: Curve::Linear,
        },
        // Both silent for bars [0.5, 1.0] — anticipation.

        // Bar 1 downbeat: both decks hard-cut to full volume.
        Phase::Fader {
            deck: DeckRole::Outgoing,
            from: 0.0, to: 1.0,
            span: bars(1.0, 1.0),
            curve: Curve::Step,
        },
        Phase::Fader {
            deck: DeckRole::Incoming,
            from: 0.0, to: 0.0,
            span: bars(0.0, 1.0 - 0.001),
            curve: Curve::Step,
        },
        Phase::Fader {
            deck: DeckRole::Incoming,
            from: 0.0, to: 1.0,
            span: bars(1.0, 1.0),
            curve: Curve::Step,
        },

        // Bars [1, 2]: both decks play at full volume — the "double drop" moment.

        // Bar 2 downbeat: outgoing dies, incoming continues as the new master.
        Phase::Fader {
            deck: DeckRole::Outgoing,
            from: 1.0, to: 0.0,
            span: bars(l, l),
            curve: Curve::Step,
        },
    ],
    outgoing_init: None,
    incoming_init: None,
}
```

## Scoring rules

In `score_recipe`, add three branches between the existing `RecipeChoice::HighsFirst => { ... }` and the closing `}`:

```rust
RecipeChoice::Drop => {
    // Drop is a build-into-release move. Wants big energy ramp-up + phrase aligned + tight BPM.
    let big_ramp = inputs.energy_delta > 0.20 && inputs.phrase_aligned;
    let modest_ramp = inputs.energy_delta > 0.10 && inputs.phrase_aligned;
    if big_ramp { 0.75 }
    else if modest_ramp { 0.40 }
    else { 0.10 }
}

RecipeChoice::BreakdownBridge => {
    // Wind-down event. Wants energy_delta < 0 (going softer) + phrase aligned.
    let big_winddown = inputs.energy_delta < -0.20 && inputs.phrase_aligned;
    let modest_winddown = inputs.energy_delta < -0.10 && inputs.phrase_aligned;
    if big_winddown { 0.70 }
    else if modest_winddown { 0.40 }
    else { 0.10 }
}

RecipeChoice::DoubleDrop => {
    // Jackpot recipe — only fires when both tracks really match.
    let identical_bpm_close_energy =
        inputs.length_bars >= 16            // proxy for "planner thinks they're already similar"
            && inputs.energy_delta.abs() < 0.05
            && inputs.phrase_aligned;
    if identical_bpm_close_energy { 0.80 }
    else { 0.05 }
}
```

Notes:

- `Drop` and `BreakdownBridge` compete with `HighsFirst` (also `energy_delta > 0.2`) and `EchoThrow` / `FilterKill` (for wind-down). The recency penalty keeps rotation healthy.
- `DoubleDrop`'s gate keys off `inputs.length_bars >= 16` as a proxy for "this is a pair the planner would have given a long blend to" — i.e., similar enough to mash up. The recipe will still use L=2 internally for its window; we just use the planner's gate as a *selection* signal.
- All three recipes are conservative on score — they sit at 0.10 baseline and only earn higher when their fit conditions are met. Compared to `BassSwap` (peak 0.85) and `ThreeBandSwap` (peak 0.90), they're slightly less common.

## RecipeChoice + dispatch wiring

```rust
pub enum RecipeChoice {
    // ... existing 10 variants ...
    Drop,
    BreakdownBridge,
    DoubleDrop,
}
```

- `recipe_choice_id`: `Drop => 11`, `BreakdownBridge => 12`, `DoubleDrop => 13`.
- `recency_hash`: iterate over all 13 variants.
- `pick_recipe` candidates array: append the three new variants.
- `--force-recipe` enum (`config.rs`): three new variants `Drop`, `BreakdownBridge`, `DoubleDrop` with `to_recipe_choice` mappings.
- `main.rs::recipe_choice_name` (the helper feeding the `scores=` log field): `Drop => "drop"`, `BreakdownBridge => "breakdown_bridge"`, `DoubleDrop => "double_drop"`.
- `main.rs::recipe_name` (used in the `transition arming` log): same three strings.
- `main.rs`'s scheduler-arm match: three new arms calling `dj_engine::recipe::Drop.build(&ctx)` etc.

## Testing

### Phase-level (script_test.rs)

Three new tests, one per recipe:

1. **`drop_emits_expected_phases`**: `Drop.build_script(ctx_at_120bpm)` returns a `TransitionScript` with `length_bars == 4`, contains a `Filter` phase with `from: 0.85, to: 0.55, span: [0, 4]` and another `Filter` phase that snaps to 0 at bar 4.
2. **`breakdown_bridge_kills_outgoing_mids_and_bass`**: contains `EqRamp { deck: Outgoing, band: Low, from_db: 0, to_db: -40 }` and the same for `Mid`.
3. **`double_drop_holds_silence_then_both_play`**: contains a `Fader { Outgoing, 1→0, span: [0, 0.5] }` and a `Fader { Outgoing, 0→1, span: [1, 1] }`; same for incoming with `[1, 1]` snap to 1.0.

### Scoring (recipe_test.rs)

Three new tests:

1. **`score_drop_big_ramp_up`**: `PairScoreInputs { energy_delta: 0.30, phrase_aligned: true, ... } → 0.75`.
2. **`score_breakdown_bridge_winddown`**: `energy_delta: -0.25, phrase_aligned: true → 0.70`.
3. **`score_double_drop_jackpot`**: `length_bars: 16, energy_delta: 0.02, phrase_aligned: true → 0.80`.

### Existing tests

The new variants in `RecipeChoice` will force the `pick_recipe`-driven tests to recompute scores. If any existing test asserts an exact picked recipe and the new variants edge it out, update the test to permit the new winner. Most existing tests assert score *ranges* or specific recipes' scores, which are unaffected.

## Risks

- **Recipe-internal length vs. planner-suggested length disagreement.** The new recipes return `length_bars: 4` or `2` in their `TransitionScript` regardless of what `ctx.plan.length_bars` was. This means: the picker's `score_recipe` sees `inputs.length_bars` from the planner (e.g., 8 or 16), but the actual transition runs for 4 or 2 bars. Acceptable — scoring uses planner length as a proxy for "are these tracks compatible," and recipe-internal length is the artistic gesture.
- **`scores=` log line gets longer.** Three new key=value pairs in the score breakdown. Log line will be ~150 chars instead of 100. Negligible.
- **HardCut overlap.** `BreakdownBridge` is essentially a "hard cut with a 4-bar EQ-killed lead-in on outgoing." When the planner forces `HardCut` (via `skip_tempo_align`), the picker short-circuits to HardCut. BreakdownBridge cannot fire on a skip-tempo pair — correct behavior, since `BreakdownBridge` assumes synced BPMs.
- **`DoubleDrop` may sound chaotic.** Two tracks at full volume for 1 bar is a lot of energy. If the tracks have clashing harmonic content, the result is dissonant. Mitigation: scoring gate requires `energy_delta.abs() < 0.05` and `phrase_aligned`, which heuristically pairs tracks that *should* mash up cleanly. After listening, tighten further if needed (add key-match requirement, lower the score, etc.).
- **`Drop` filter ramp may not be audible on bass-light tracks.** The HP filter at 0.85 strips bass; if the incoming has no significant bass, the build-up sounds the same as the drop. Mitigation: scoring already gates on `energy_delta > 0.10` which usually implies bass-rich incoming. If the listener reports this, demote Drop's score for low-bass pairs.

## Components and file layout

```
engine/src/
  recipe.rs              ← +Drop, +BreakdownBridge, +DoubleDrop structs and impls
                          ← RecipeChoice gains 3 variants
                          ← recipe_choice_id gains 3 ids
                          ← recency_hash iterates 13 variants
                          ← pick_recipe candidates gains 3 entries
                          ← score_recipe gains 3 branches
  config.rs              ← ForceRecipe gains 3 variants + to_recipe_choice arms
  main.rs                ← recipe_name + recipe_choice_name gain 3 entries
                          ← scheduler-arm match gains 3 arms

engine/tests/
  script_test.rs         ← +3 phase-shape tests
  recipe_test.rs         ← +3 scoring tests
```

No DSL changes, no sidecar changes, no analyzer changes, no compiler changes. All additive.

## Open questions

- After listening, tune the constants. `Drop`'s filter knob range (0.85 → 0.55) might be too narrow (not enough build) or too wide (incoming too audible during build). `BreakdownBridge`'s 4-bar window might want to be 8 bars for a longer breakdown feel. `DoubleDrop`'s 0.5-bar fade-out might need to be quicker (0.25 bars) for a tighter "anticipation gap." All single-line edits.
- `Drop`'s build-up could go further — e.g., 8 bars instead of 4 — to give more anticipation. Trade-off is the recipe gets less responsive on the picker's natural rotation.

## Roadmap (Phase D follow-up)

When stems analytics are integrated into the planner/picker (post-Phase D), these three recipes get stem-aware variants:

- `DropOnDrumDensityPeak`: time the drop to incoming's first drum-density peak instead of the next downbeat.
- `BreakdownBridgeStemAware`: align the breakdown's EQ-kill timing to outgoing's actual drum-density valley.
- `DoubleDropOnSyncedDrops`: only fire when outgoing's drum-density valley aligns harmonically with incoming's first drum-density peak.

Out of scope for this spec.
