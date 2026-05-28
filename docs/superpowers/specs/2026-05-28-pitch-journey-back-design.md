# Pitch Journey Back to Natural BPM

**Date:** 2026-05-28
**Status:** Approved for planning
**Branch:** `research/two-deck-mixer`
**Sub-project:** engine recipe-layer enhancement

## Summary

Every recipe currently emits `Phase::TempoAlign { Incoming, ratio }` as a Step at bar 0 of the transition, locking the incoming deck at the outgoing's BPM. The incoming track then plays at that pitched tempo forever — over a chain of transitions, all tracks end up playing at the seed's BPM, with a small but audible time-stretch artifact riding on every track that isn't natively at the seed's tempo. Real DJs handle this by **gradually returning the now-playing deck to its natural BPM** over the first 30-60 seconds of the new track. Add a `Phase::TempoRamp` variant to the DSL and inject it automatically (via `build_and_compile`) after every transition when the original pitch shift was small enough to be tasteful (`|ratio - 1.0| < 0.05`). Skip injection on no-sync transitions (`plan.skip_tempo_align == true`).

## Motivation

The engine's tempo-align is a single Step at bar 0 — set incoming's tempo ratio, then walk away. The deck plays at `incoming.bpm * ratio` until the next transition. In a 10-transition chain starting from a 120-BPM seed, a 105-BPM track gets stretched to 120 BPM and stays there for its entire playback — 4 minutes of audible time-stretch artifact on vocals and transients.

Git history shows this was once implemented (commit `90e7493 fix(engine): correct outro deadline + drop pitch-journey ramp`) but was removed alongside a bug fix for outro-deadline arithmetic. The underlying `Tempo Linear` automation primitive and chunked-atempo decoder support (sub-project 1, task 14) remain in place — only the *emission* of the ramp was removed. This spec reintroduces emission with a narrower scope (only when the pitch shift was modest) and accepts the outro-deadline rounding risk as a known tradeoff.

## Goals

- Add `Phase::TempoRamp { deck, from, to, span, curve }` to the recipe DSL.
- Compile `TempoRamp` into a `Tempo` Linear automation. **Do not clamp the span to `[0, length_bars]`** — the ramp is meant to extend past the transition window into the new master's playback.
- Inject the ramp automatically in `build_and_compile` (the call seam that goes from `Recipe::build_script` → `compile`) when:
  - `!ctx.plan.skip_tempo_align` (we beatmatched in the first place)
  - The script contains a `TempoAlign { deck: Incoming, ratio }` phase
  - `0.005 ≤ |ratio - 1.0| < 0.05` (worth doing, but not so far apart that the journey itself becomes audible drift)
- Inject at span `bars(length_bars, length_bars + 32)`, ramping from `ratio` to `1.0`, `Curve::Linear`.
- Individual recipes get **no changes**. The injection is centralised so all 10 recipes benefit identically.

## Non-Goals

- **No outro-deadline arithmetic fix.** The existing code in `main.rs` computes frames-to-outro using the deck's *current* tempo. While the tempo is ramping, that calculation under- or over-estimates by a few seconds. Accepted risk: 32-bar journey at 120 BPM ≈ 64 seconds, which finishes well before the typical track's outro. If a track has an unusually short body (under ~75 s), the next outro deadline may be a fraction of a bar off — a minor fade-out timing wobble, not a phase-mismatch event.
- **No journey length adaptation.** Fixed at 32 bars. A future enhancement could clamp to `min(32, available_bars_before_outro)` if needed.
- **No new Curves.** `Linear` is sufficient. A future enhancement might use a logarithmic curve so listeners perceive linear pitch-change rather than linear-frequency change, but Linear is fine to start.
- **No retro-fitting individual recipes.** The injection happens at `build_and_compile`, untouched by recipe code.
- **No changes to `Phase::TempoAlign`.** It stays as a Step; that's correct for the initial sync.
- **No removal of `TempoAlign` from recipes.** All recipes still emit it. The journey runs *after* the TempoAlign Step has done its job.

## Core types

Add to `engine/src/recipe_script.rs` (`Phase` enum):

```rust
pub enum Phase {
    // ... existing variants ...

    /// Linear tempo ramp. Used by `build_and_compile` to inject the pitch-journey-
    /// back after a successful beatmatched transition. Unlike other phases, the
    /// span MAY extend past `TransitionScript::length_bars` — the journey runs
    /// during the new master's playback, after the transition window has closed.
    TempoRamp {
        deck: DeckRole,
        from: f64,
        to: f64,
        span: BarSpan,
        curve: Curve,
    },
}
```

## Compiler changes

In `compile()` (`engine/src/recipe_script.rs`), add a `Phase::TempoRamp` arm in the match:

```rust
Phase::TempoRamp { deck, from, to, span, curve } => {
    // Important: do NOT clamp span.end to [0, length_bars]. The journey
    // intentionally runs past the transition window.
    if !span.end.bars.is_finite() || !span.start.bars.is_finite() {
        warn!("compile: TempoRamp has non-finite span, dropping phase");
        return;  // or continue, matching the enclosing structure
    }
    if span.end.bars <= span.start.bars {
        warn!("compile: TempoRamp has non-positive span, dropping phase");
        return;
    }
    // Allow negative or oversize bars values for TempoRamp — only the frame
    // index matters and the scheduler's automation queue handles
    // past-the-end positions fine.
    let frames_per_bar_local = bar_seconds * (ctx.sample_rate as f64);
    let t_start = t0 + (span.start.bars * frames_per_bar_local) as u64;
    let t_end_raw = t0 + (span.end.bars * frames_per_bar_local) as u64;
    let t_end = if t_end_raw <= t_start { t_start + 1 } else { t_end_raw };
    push(
        deck,
        Automation {
            target: AutomationTarget::Tempo,
            t_start_frames: t_start,
            t_end_frames: t_end,
            from,
            to,
            curve,
        },
        &mut outgoing_automations,
        &mut incoming_automations,
    );
}
```

(The compiler already has `bar_seconds`, `frames_per_bar`, `t0`, and the `push` macro/closure in scope. Match the surrounding style.)

`TempoRamp` deliberately bypasses the `bar_to_frame` helper because that helper clamps to `[0, length_bars_f]`. We need direct conversion for this phase only.

## Injection point: `build_and_compile`

In `engine/src/recipe.rs`, modify the existing helper:

```rust
pub fn build_and_compile(recipe: &dyn Recipe, ctx: &TransitionContext<'_>) -> RecipeOutput {
    let mut script = recipe.build_script(ctx);
    inject_pitch_journey(&mut script, ctx);
    crate::recipe_script::compile(&script, ctx)
}

fn inject_pitch_journey(
    script: &mut crate::recipe_script::TransitionScript,
    ctx: &TransitionContext<'_>,
) {
    use crate::recipe_script::{bars, DeckRole, Phase};

    if ctx.plan.skip_tempo_align {
        return;
    }

    // Find the TempoAlign on Incoming; its `ratio` is what we journey back from.
    let ratio = script.phases.iter().find_map(|p| match p {
        Phase::TempoAlign { deck: DeckRole::Incoming, ratio } => Some(*ratio),
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
        curve: crate::automation::Curve::Linear,
    });
}
```

Notes:

- Idempotent: `inject_pitch_journey` only runs once per call. If `build_and_compile` is called twice on the same script (it shouldn't be, but defensively), the second run finds the same TempoAlign, computes the same conditions, and pushes a duplicate ramp. Mitigation: also check that no `Phase::TempoRamp` already exists with `deck: Incoming` and skip injection if found.
- The `0.005..0.05` threshold: below 0.005 the journey is < 6 cents — inaudible, no point; above 0.05 the original sync was already audibly pitch-shifted, and journeying back means the listener hears the new track's pitch *change* over 64 seconds — more jarring than the steady (wrong) pitch.

## Testing strategy

### Unit tests

`engine/tests/compile_test.rs` — append:

1. **TempoRamp compiles to a Tempo Linear automation.** Script with one `TempoRamp { Incoming, from: 0.97, to: 1.0, span: bars(16, 48), Linear }` and `length_bars = 16` → emits one `Automation { target: Tempo, curve: Linear, from: 0.97, to: 1.0, t_start = t0 + 16*88200, t_end = t0 + 48*88200 }` on incoming.
2. **TempoRamp's span is NOT clamped to length_bars.** Span `bars(16, 64)` on a `length_bars=16` script produces `t_end = t0 + 64*88200` (not `t0 + 16*88200`).
3. **TempoRamp with `end <= start` is dropped.**
4. **TempoRamp with non-finite bars is dropped.**

`engine/tests/recipe_test.rs` — append:

5. **`build_and_compile` injects TempoRamp when ratio ≈ 0.95.** Synthesize a `TransitionContext` with outgoing 120 BPM, incoming 126 BPM (ratio ≈ 0.952), `plan.skip_tempo_align = false`. Call `build_and_compile(&EqualPowerCrossfade, &ctx)`. Assert `output.incoming_automations` contains a `Tempo` automation with `curve = Linear, from ≈ 0.952, to = 1.0`, `t_start_frames` near `t0 + 16 bars worth of frames`.
6. **`build_and_compile` does NOT inject when ratio = 1.0.** Outgoing and incoming both 120 BPM. No Tempo Linear automation in output.
7. **`build_and_compile` does NOT inject when ratio = 0.85.** Outside the [0.005, 0.05) delta band. No Tempo Linear automation in output.
8. **`build_and_compile` does NOT inject when `plan.skip_tempo_align = true`.** Even with ratio in the journey band.
9. **`inject_pitch_journey` is idempotent.** Call it twice on the same script. Assert only one `TempoRamp { Incoming, .. }` exists. (Implementation: add the no-duplicate check noted above.)

### Integration safety

`engine/tests/mixer_integration_test.rs` already exercises the full pipeline. After this change, the integration tests should still pass: any test that uses a small BPM delta (within the journey band) will get a Tempo Linear automation it didn't have before, but the automation queue handles unknown-to-test automations fine — they just don't affect the listener's audio expectations measurably within the transition window.

If a mixer integration test asserts on the exact count of automations on the incoming deck, that count grows by 1 when the journey injects. Update the assertion to expect the new count.

## Risks

- **Outro-deadline rounding.** During the 32-bar journey, `main.rs`'s `frames_until_outro = ((outro_start - source_pos) * SR / tempo) as u64` uses the deck's *current* tempo — which is mid-ramp. If the journey is still in progress when the planner computes the next outro deadline, the result is slightly off. Magnitude: at most 32 bars × 2.5% (the average ratio offset during a 5%-ratio journey) ≈ 0.8 bars ≈ 1.5 seconds. Listener hears the fade-out start ~1.5 s early or late. Not phase-broken, not jarring.
- **Idempotency.** Mitigated by checking for an existing `TempoRamp { Incoming, .. }` before injecting.
- **Journey overlaps the next transition's arming.** Unlikely (32 bars ≈ 64 s; tracks are typically 3-4 min) but possible on short tracks. If the next TempoAlign Step fires before the ramp finishes, the step wins (it's queued after the ramp by virtue of having later `t_start_frames`, and the automation queue processes them in time order). The behavior is: tempo ramps toward 1.0 → next transition starts → TempoAlign Step jumps to new ratio. Listener hears a kink in the tempo curve at the moment of the new transition's bar 0, which is masked by the transition itself.
- **Recipes that don't emit TempoAlign for Incoming.** `inject_pitch_journey` returns early if no such phase is found. All 10 current recipes emit one. If a future recipe omits TempoAlign deliberately (e.g., a "free-tempo" recipe), the journey simply doesn't fire — correct behavior.

## Components and file layout

```
engine/src/
  recipe_script.rs       ← +Phase::TempoRamp variant
                          ← compile() gains a TempoRamp arm
  recipe.rs              ← build_and_compile gains an inject_pitch_journey call
                          ← new private function inject_pitch_journey

engine/tests/
  compile_test.rs        ← +4 tests for TempoRamp compile behavior
  recipe_test.rs         ← +5 tests for build_and_compile injection logic
```

No CLI changes. No sidecar schema changes. No scheduler/deck/mixer/automation changes. Backward compatible: existing recipes work identically; the new ramp is purely additive.

## Open questions

- After listening, the user may want to tune the journey duration (32 bars too long? too short?) or the ratio thresholds (`0.005..0.05`). All three are constants in one function (`inject_pitch_journey`) — easy follow-up.
- A future enhancement could ramp logarithmically (so listeners perceive linear pitch change) instead of linearly in the tempo ratio. Out of scope here.
