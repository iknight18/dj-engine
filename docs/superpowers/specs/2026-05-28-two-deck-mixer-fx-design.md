# Two-Deck Mixer — Echo + Loop FX (Sub-project 4)

**Date:** 2026-05-28
**Status:** Approved for planning
**Branch:** `research/two-deck-mixer`
**Sub-project:** 4 of 5

## Summary

Add two per-deck effects: an **IIR echo delay** (single-tap with feedback) and a **loop-capture buffer** (records the last N bars of source audio and plays them back in a loop). Each gets one new `AutomationTarget` to drive the dynamic parameter (`EchoMix` for the dry/wet send, `LoopGate` to enter/exit loop mode), plus per-deck initial-config plumbing for static parameters (echo delay/feedback, loop bar count) set once at the start of a transition by the recipe. Two new recipes drive these: `EchoOut` (outgoing's echo crescendos while it fades out, so it "trails off into echoey tails") and `LoopOut` (outgoing locks into a 4-bar loop at the start of the crossfade and fades out while looping, so it "freezes and falls away" while the incoming enters).

## Goals

- Each `Deck` carries an `EchoLine` (circular delay buffer + feedback) and a `LoopBuffer` (capture ring + locked snapshot).
- New `AutomationTarget` variants: `EchoMix` (0.0–1.0 dry/wet), `LoopGate` (0 = source / ≥0.5 = locked loop). Per-tick state updates work the same as the existing automation targets.
- New per-deck **init config** mechanism on `RecipeOutput`: optional `outgoing_init: Option<DeckInitConfig>` and `incoming_init: Option<DeckInitConfig>` carry static parameters (echo delay seconds, echo feedback, loop bars) that the scheduler applies once after cue and before play. This avoids creating a whole automation target just to set a parameter that never changes during a transition.
- New recipes `EchoOut` and `LoopOut`, both routed via the existing `--force-recipe` flag (no auto-pick from `pick_recipe` in this sub-project — sub-project 5 makes selection scoring-driven).
- Existing behavior unchanged for `EqualPowerCrossfade`, `HardCut`, `BassSwap`, `FilterFade`, `FilterKill` (they set both `*_init` to `None`).

## Non-Goals (this sub-project)

- No selector changes. Recipes still auto-fired by the plan-driven `pick_recipe`; the new ones only run via `--force-recipe`.
- No multi-tap echo, no ping-pong stereo echo, no echo low-pass on the feedback path. The echo is a straight single-tap IIR with a fixed-coefficient feedback gain.
- No "loop in / loop out" with custom boundaries — `LoopOut` locks at `t0` and replays the K bars *preceding* `t0` until either the loop_gate goes back to 0 or the deck is unloaded. Snap-to-beat alignment is implicit because both decks are pre-tempo-matched and `t0` is master-clock-aligned with a downbeat (existing sub-project 1 invariant).
- No half-beat / triplet / dotted echo divisions; echo delay is a continuous `f64` in seconds.

## Audible behavior

| Recipe | What you hear |
|---|---|
| `EchoOut` | Both decks crossfade with the normal equal-power fader curves. **The outgoing deck's signal also feeds an echo line** (1-beat delay at outgoing's BPM, feedback 0.5). The echo's wet-mix ramps 0 → 1 over the first half of the crossfade, so as the outgoing fades down, its echoey reflections come up to fill the space. After the fader hits 0 the dry path is silent but the echo line's tail keeps decaying at feedback 0.5 per beat (~6 dB per beat) — you get a fading "ghost" of the outgoing's last beat or two riding alongside the incoming. |
| `LoopOut` | At `t0`, the outgoing deck **locks the most-recent 4 bars of its audio into a buffer** and starts replaying them in a loop, ignoring the source decoder. Both faders run the normal equal-power crossfade across `length_bars`. So you hear the outgoing's last 4 bars repeating while it fades out, with the incoming sliding in over the top. After the crossfade, the outgoing is unloaded and the loop ends. |

Both can be combined with the existing recipes' machinery later (e.g., bass-swap + echo-out), but sub-project 4 ships them as standalone recipes.

## Components

### Component 1 — `EchoLine` struct

A tiny new module `engine/src/effects.rs`:

```rust
pub struct EchoLine {
    buffer_l: Vec<f32>,
    buffer_r: Vec<f32>,
    write_pos: usize,
    delay_samples: usize,
    feedback: f64,         // 0.0..0.9, applied to the delayed sample before storing
}

impl EchoLine {
    /// Allocate for the worst-case delay (e.g., 2 seconds at SAMPLE_RATE).
    /// Initial delay/feedback default to 0 (silence / passthrough).
    pub fn new(capacity_samples: usize) -> Self { /* zero-fill both buffers */ }

    /// Reconfigure the delay tap (in samples) and feedback. State (buffer contents)
    /// is preserved.
    pub fn set_params(&mut self, delay_samples: usize, feedback: f64) {
        self.delay_samples = delay_samples.min(self.buffer_l.len().saturating_sub(1));
        self.feedback = feedback.clamp(0.0, 0.95);
    }

    /// Reset to silence (called by Deck::reset_effects from cue/unload).
    pub fn clear(&mut self) {
        self.buffer_l.iter_mut().for_each(|x| *x = 0.0);
        self.buffer_r.iter_mut().for_each(|x| *x = 0.0);
        self.write_pos = 0;
    }

    /// Push a stereo sample through and return the delayed wet sample.
    /// Caller is responsible for the dry/wet mix.
    ///
    /// Signal: write_in = input + delayed * feedback. The delayed sample is
    /// the buffer value `delay_samples` positions ago.
    pub fn process(&mut self, l_in: f32, r_in: f32) -> (f32, f32) {
        if self.delay_samples == 0 {
            // Bypass — no echo configured. Don't accumulate state.
            return (0.0, 0.0);
        }
        let cap = self.buffer_l.len();
        let read_pos = (self.write_pos + cap - self.delay_samples) % cap;
        let delayed_l = self.buffer_l[read_pos];
        let delayed_r = self.buffer_r[read_pos];
        let store_l = l_in + (delayed_l as f64 * self.feedback) as f32;
        let store_r = r_in + (delayed_r as f64 * self.feedback) as f32;
        self.buffer_l[self.write_pos] = store_l;
        self.buffer_r[self.write_pos] = store_r;
        self.write_pos = (self.write_pos + 1) % cap;
        (delayed_l, delayed_r)
    }
}
```

Default echo line capacity = `2 * SAMPLE_RATE` (2 seconds, enough for 1 beat at ≥30 BPM). Allocated once at `Deck::new`.

### Component 2 — `LoopBuffer` struct

In the same `effects.rs`:

```rust
pub struct LoopBuffer {
    capture_l: Vec<f32>,    // ring buffer that always records latest audio
    capture_r: Vec<f32>,
    capture_pos: usize,
    is_locked: bool,
    locked_pos: usize,       // current read offset within the active loop
    active_len: usize,       // how many samples of the capture are "active" — see lock()
}

impl LoopBuffer {
    /// Allocate for the worst-case loop length (e.g., 16 bars at the slowest BPM).
    /// Initial state: not locked, all silence.
    pub fn new(capacity_samples: usize) -> Self { /* … */ }

    pub fn set_loop_samples(&mut self, samples: usize) {
        self.active_len = samples.min(self.capture_l.len());
    }

    /// Continuous write of one stereo sample to the capture ring.
    /// Caller invokes this on every frame while NOT locked.
    pub fn write(&mut self, l: f32, r: f32) {
        let cap = self.capture_l.len();
        self.capture_l[self.capture_pos] = l;
        self.capture_r[self.capture_pos] = r;
        self.capture_pos = (self.capture_pos + 1) % cap;
    }

    /// Freeze the capture into a loop of `active_len` samples ending at the
    /// current `capture_pos`. Read position starts at the OLDEST sample (i.e.,
    /// `capture_pos - active_len`).
    pub fn lock(&mut self) {
        let cap = self.capture_l.len();
        self.locked_pos = (self.capture_pos + cap - self.active_len) % cap;
        self.is_locked = true;
    }

    pub fn unlock(&mut self) { self.is_locked = false; }

    pub fn is_locked(&self) -> bool { self.is_locked }

    /// Read one stereo sample from the locked region. Wraps after `active_len`.
    pub fn read(&mut self) -> (f32, f32) {
        let cap = self.capture_l.len();
        let l = self.capture_l[self.locked_pos];
        let r = self.capture_r[self.locked_pos];
        // Advance and wrap within the active window.
        let next = (self.locked_pos + 1) % cap;
        // Wrap back to the start of the loop region when we hit capture_pos.
        self.locked_pos = if next == self.capture_pos {
            (self.capture_pos + cap - self.active_len) % cap
        } else {
            next
        };
        (l, r)
    }

    pub fn clear(&mut self) {
        self.capture_l.iter_mut().for_each(|x| *x = 0.0);
        self.capture_r.iter_mut().for_each(|x| *x = 0.0);
        self.capture_pos = 0;
        self.is_locked = false;
        self.locked_pos = 0;
    }
}
```

Default loop capacity = `8 * SAMPLE_RATE` (8 seconds, covers 4 bars at ≥120 BPM with margin; if a future recipe needs 16 bars at low BPM, bump this constant). Allocated once at `Deck::new`.

### Component 3 — `AutomationTarget` extension

```rust
pub enum AutomationTarget {
    Fader,
    Tempo,
    EqLow,
    EqMid,
    EqHigh,
    Filter,
    EchoMix,
    LoopGate,
}
```

`apply_value`:

```rust
AutomationTarget::EchoMix => {
    state.echo_mix = v.clamp(0.0, 1.0);
}
AutomationTarget::LoopGate => {
    // Treated as a boolean: < 0.5 = unlocked, >= 0.5 = locked.
    state.loop_gate = v.clamp(0.0, 1.0);
}
```

### Component 4 — `DeckState` extension

```rust
pub echo_mix: f64,     // 0..1, default 0
pub loop_gate: f64,    // 0..1, default 0
```

### Component 5 — `Deck` integration

Add to `Deck`:

```rust
echo_line: EchoLine,
loop_buffer: LoopBuffer,
```

Constants (next to the EQ/filter constants):

```rust
const ECHO_CAPACITY_SAMPLES: usize = (SAMPLE_RATE * 2) as usize;   // 2 seconds
const LOOP_CAPACITY_SAMPLES: usize = (SAMPLE_RATE * 8) as usize;   // 8 seconds
```

`Deck::new` constructs both with the capacities above.

A `reset_effects` helper that `cue()` and `unload()` both call (alongside `reset_eq`, `reset_filter`):

```rust
fn reset_effects(&mut self) {
    self.echo_line.clear();
    self.echo_line.set_params(0, 0.0);  // bypass until configured
    self.loop_buffer.clear();
    self.state.echo_mix = 0.0;
    self.state.loop_gate = 0.0;
}
```

Two new public methods used by the scheduler:

```rust
impl Deck {
    pub fn set_echo_params(&mut self, delay_seconds: f64, feedback: f64) {
        let delay_samples = (delay_seconds * SAMPLE_RATE as f64) as usize;
        self.echo_line.set_params(delay_samples, feedback);
    }

    pub fn set_loop_bars(&mut self, bars: usize, bar_seconds: f64) {
        let samples = (bars as f64 * bar_seconds * SAMPLE_RATE as f64) as usize;
        self.loop_buffer.set_loop_samples(samples);
    }
}
```

### Component 6 — `Deck::pull` ordering

The per-tick header (before the per-sample loop) gets a new branch for loop mode:

```rust
// Loop transitions (boundary-trigger semantics).
if self.state.loop_gate >= 0.5 && !self.loop_buffer.is_locked() {
    self.loop_buffer.lock();
} else if self.state.loop_gate < 0.5 && self.loop_buffer.is_locked() {
    self.loop_buffer.unlock();
}
let loop_active = self.loop_buffer.is_locked();
```

The source-queue drain branches:

```rust
if loop_active {
    // Don't drain the source — fill `out` from the locked loop buffer instead.
    for f in 0..n_frames {
        let (l, r) = self.loop_buffer.read();
        out[f * 2] = l;
        out[f * 2 + 1] = r;
    }
    // NB: source_position_seconds does NOT advance while looping. Once the
    // recipe lets the gate fall, the deck resumes from where the source
    // queue left off.
} else {
    // Existing drain into `out`.
    // ...
}
```

The per-sample chain becomes:

```rust
for f in 0..n_frames {
    let cursor = start_cursor + f as u64;
    self.automations.step(cursor, &mut self.state);

    let li = f * 2;
    let ri = li + 1;
    let raw_l = out[li];
    let raw_r = out[ri];

    // Loop capture: only record fresh source-queue audio. While in loop
    // mode, raw came from the buffer; capturing it again would create a
    // feedback path.
    if !loop_active {
        self.loop_buffer.write(raw_l, raw_r);
    }

    // EQ chain (existing)
    let l = self.eq_high_l.process(self.eq_mid_l.process(self.eq_low_l.process(raw_l)));
    let r = self.eq_high_r.process(self.eq_mid_r.process(self.eq_low_r.process(raw_r)));

    // Filter (existing)
    let (l, r) = if filter_active {
        (self.filter_l.process(l), self.filter_r.process(r))
    } else {
        (l, r)
    };

    // Fader (existing)
    let g = self.state.fader;
    let dry_l = l * g;
    let dry_r = r * g;

    // Echo — pre-fader input feeds the line; the line's wet output is
    // added on top of the post-fader dry signal, scaled by echo_mix.
    let (wet_l, wet_r) = self.echo_line.process(l, r);
    let mix = self.state.echo_mix as f32;
    out[li] = dry_l + wet_l * mix;
    out[ri] = dry_r + wet_r * mix;
}
```

**Why echo is post-EQ-pre-fader (input) and post-fader (output sum)**: this is the standard DJ "send" topology. The echo line accumulates the EQ'd signal regardless of fader position, and its tail rings out even when the fader has reached 0 — so `EchoOut` produces audible reflections after the dry signal has gone silent.

**Source-position-doesn't-advance-during-loop**: documented in the spec. Outgoing's source decoder stays at its current pipe-buffer position; ffmpeg blocks once its pipe fills. When the loop ends (gate falls or deck unloads), the source position is exactly where it was at lock time. For `LoopOut` this is fine because the outgoing is unloaded after the transition.

### Component 7 — `RecipeOutput` extension

```rust
pub struct DeckInitConfig {
    pub echo_delay_seconds: Option<f64>,
    pub echo_feedback: Option<f64>,
    pub loop_bars: Option<usize>,
    pub loop_bar_seconds: Option<f64>,
}

pub struct RecipeOutput {
    pub outgoing_automations: Vec<Automation>,
    pub incoming_automations: Vec<Automation>,
    pub outgoing_init: Option<DeckInitConfig>,
    pub incoming_init: Option<DeckInitConfig>,
    pub total_duration_frames: u64,
}
```

All existing recipes set `outgoing_init = None` and `incoming_init = None` — no behavioral change.

### Component 8 — `TransitionScheduler::arm` applies the init configs

In `scheduler.rs`, after `cue(... ).await?` but before `.play()`, apply the init:

```rust
if let Some(cfg) = output.outgoing_init.as_ref() {
    apply_deck_init(mixer.deck(outgoing_deck), cfg);
}
if let Some(cfg) = output.incoming_init.as_ref() {
    apply_deck_init(mixer.deck(incoming_deck), cfg);
}

fn apply_deck_init(deck: &mut Deck, cfg: &DeckInitConfig) {
    if let (Some(delay), Some(feedback)) = (cfg.echo_delay_seconds, cfg.echo_feedback) {
        deck.set_echo_params(delay, feedback);
    }
    if let (Some(bars), Some(bar_seconds)) = (cfg.loop_bars, cfg.loop_bar_seconds) {
        deck.set_loop_bars(bars, bar_seconds);
    }
}
```

(Re-order if needed so that the outgoing-init runs on the outgoing deck and the incoming-init runs on the incoming deck. The outgoing deck is already loaded from the previous transition — no need to cue it; just apply the init.)

### Component 9 — `EchoOut` recipe

```rust
pub struct EchoOut;

impl Recipe for EchoOut {
    fn name(&self) -> &'static str { "echo_out" }

    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let beat_seconds = bar_seconds / 4.0;
        let sr = ctx.sample_rate as f64;
        let length_frames = (ctx.plan.length_bars as f64 * bar_seconds * sr) as u64;
        let t0 = ctx.master_clock_frames;
        let t1 = t0 + length_frames;

        // Equal-power fader pair, same as EqualPowerCrossfade.
        let outgoing_fader = Automation { /* ... */ };
        let incoming_fader = Automation { /* ... */ };
        let incoming_tempo_step = Automation { /* ... */ };

        // Outgoing EchoMix: 0 → 1 over the first half of the crossfade, then stays at 1
        // for the second half so the wet tail accumulates loudly while the fader takes
        // the dry to silence.
        let outgoing_echo_ramp = Automation {
            target: AutomationTarget::EchoMix,
            t_start_frames: t0,
            t_end_frames: t0 + length_frames / 2,
            from: 0.0,
            to: 1.0,
            curve: Curve::Linear,
        };

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader, outgoing_echo_ramp],
            incoming_automations: vec![incoming_fader, incoming_tempo_step],
            outgoing_init: Some(DeckInitConfig {
                echo_delay_seconds: Some(beat_seconds),  // 1 beat
                echo_feedback: Some(0.5),                // 6 dB per beat
                loop_bars: None,
                loop_bar_seconds: None,
            }),
            incoming_init: None,
            total_duration_frames: length_frames,
        }
    }
}
```

### Component 10 — `LoopOut` recipe

```rust
pub struct LoopOut;

impl Recipe for LoopOut {
    fn name(&self) -> &'static str { "loop_out" }

    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let sr = ctx.sample_rate as f64;
        let length_frames = (ctx.plan.length_bars as f64 * bar_seconds * sr) as u64;
        let t0 = ctx.master_clock_frames;
        let t1 = t0 + length_frames;

        let outgoing_fader = /* EqualPowerOut 1 -> 0 */;
        let incoming_fader = /* EqualPowerIn 0 -> 1 */;
        let incoming_tempo_step = /* ratio Step */;

        // Lock outgoing into a loop at t0; release at t1 (just in case the deck
        // isn't unloaded promptly after the crossfade).
        let outgoing_loop_on = Automation {
            target: AutomationTarget::LoopGate,
            t_start_frames: t0,
            t_end_frames: t0 + 1,
            from: 1.0,
            to: 1.0,
            curve: Curve::Step,
        };
        let outgoing_loop_off = Automation {
            target: AutomationTarget::LoopGate,
            t_start_frames: t1,
            t_end_frames: t1 + 1,
            from: 0.0,
            to: 0.0,
            curve: Curve::Step,
        };

        RecipeOutput {
            outgoing_automations: vec![outgoing_fader, outgoing_loop_on, outgoing_loop_off],
            incoming_automations: vec![incoming_fader, incoming_tempo_step],
            outgoing_init: Some(DeckInitConfig {
                echo_delay_seconds: None,
                echo_feedback: None,
                loop_bars: Some(4),
                loop_bar_seconds: Some(bar_seconds),
            }),
            incoming_init: None,
            total_duration_frames: length_frames,
        }
    }
}
```

### Component 11 — `RecipeChoice` + CLI

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

Extend `config.rs::ForceRecipe` and `to_recipe_choice()` with the two new variants. `pick_recipe`'s body is unchanged (still plan-driven; new recipes only fire via `--force-recipe`).

`main.rs`'s arm-match gets two more branches calling `EchoOut` and `LoopOut` respectively.

`transition_type_label` is unchanged (still planner-intent-driven).

## Testing strategy

### Unit tests

- **`effects.rs`**:
  - `EchoLine::process` with `delay_samples=10, feedback=0.5`: feed an impulse, verify subsequent reads emit decaying taps at offsets 10, 20, 30, … with amplitudes 1.0, 0.5, 0.25, …
  - `EchoLine::process` with `delay_samples=0`: returns silence (bypass).
  - `EchoLine::set_params` updates parameters but preserves buffer.
  - `LoopBuffer::write` + `lock` + `read`: write a known ramp, lock, read N samples back, verify they match the input ramp (and wrap after `active_len`).
  - `LoopBuffer::unlock` resumes capture without clobbering the locked snapshot semantics.
- **`automation.rs`**:
  - `EchoMix` and `LoopGate` apply + clamp tests.
- **`deck.rs`**:
  - `cue()` resets `echo_line.delay_samples == 0`, `loop_buffer.is_locked() == false`, `state.echo_mix == 0`, `state.loop_gate == 0`.
  - With `set_echo_params(0.5, 0.5)` and `state.echo_mix = 1.0`, pull from a deck playing a 440 Hz sine — the output should have audible repeating "echoes" (RMS of a tail block after the input fades is non-trivial).
  - With `set_loop_bars(4, bar_seconds)` and a Step `LoopGate=1` automation, the deck's source_position_seconds does NOT advance after the lock (assert before/after).
- **`recipe.rs`**:
  - `EchoOut::build` emits 2 outgoing automations (Fader + EchoMix), 2 incoming (Fader + Tempo Step), `outgoing_init.echo_delay_seconds ≈ beat_seconds`, `echo_feedback == 0.5`.
  - `LoopOut::build` emits 3 outgoing (Fader + LoopGate-on + LoopGate-off), 2 incoming, `outgoing_init.loop_bars == 4`.
- **`config.rs`** CLI parsing: `--force-recipe echo-out` and `--force-recipe loop-out` resolve correctly.

### Integration tests

In `mixer_integration_test.rs`, add a continuous-audio test for each (modeled on the existing FilterFade/FilterKill tests). Use the same 6-second fixtures. Verify no silence gap > 4096 samples.

## Risks

- **Echo capacity vs. low BPMs**: at 30 BPM, 1 beat = 2 s; at 20 BPM, 1 beat = 3 s — exceeds the 2 s buffer. Mitigation: `set_params` clamps to buffer length; if a real-world track lands below ~30 BPM we'd hear a shorter echo than the spec says. Acceptable for the music library (152 BPM).
- **Loop buffer capacity vs. very slow tracks**: at 80 BPM, 4 bars = 12 s — exceeds the 8 s buffer. Mitigation: `set_loop_samples` clamps; at extreme BPMs `LoopOut` plays a shorter loop than requested. Document and revisit if needed. (At your library's 120–152 BPM, 4 bars is 6.3–8 s, right at the limit.)
- **No feedback runaway**: `set_params` clamps feedback to `0.95`; with feedback at 0.95 the echo decays ~0.5 dB per tap (long tails) but stays stable. At 0.5 (the default for `EchoOut`) decay is 6 dB per beat — audible but not droning.
- **Loop click on lock**: the lock snapshot starts reading at the OLDEST sample in the capture ring, then wraps. If the capture ring is full when locked (i.e., the deck has been playing for ≥ `LOOP_CAPACITY_SAMPLES / SAMPLE_RATE` seconds before t0), the join between "end of loop" and "start of loop" is sample-aligned with the original audio — no click. If the deck just started (capture not yet full), the unfilled portion contains zeros, so the loop will have a brief silence span. Mitigation: `LoopOut` is only useful after the outgoing has been playing for a while (always the case in practice — the outgoing has been the master for a full track). For robustness, the `LoopBuffer` could skip the silence region; not implemented in v1.
- **Buffer allocation**: 2 s (echo) + 8 s (loop) per deck × stereo = ~4 MB per deck. Allocated once at startup. Fine for the engine's memory budget.

## Open questions

- Should `EchoOut`'s echo continue ringing after `tick_until(handle.end_master_clock_frames)` returns and the outgoing deck is unloaded? Current answer: no — `unload()` clears the echo line. A future "decay tail" mechanism (defer unload until echo settles) could be added if the abrupt cut sounds bad.

## Roadmap (remaining sub-projects)

- **Sub-project 5** — Selector returns `(next_track, recipe)`. The selector scores all candidate (track, recipe) pairs and picks the best; `pick_recipe` becomes a hint rather than a decision. EchoOut/LoopOut/FilterFade/FilterKill become naturally auto-selected.

## Project layout changes

```
engine/src/
  effects.rs                 ← NEW: EchoLine + LoopBuffer
  lib.rs                     ← +pub mod effects;
  automation.rs              ← +EchoMix, +LoopGate (apply + clamp)
  deck.rs                    ← +echo_line, +loop_buffer fields; +echo_mix, +loop_gate
                               state; +set_echo_params, +set_loop_bars methods;
                               +reset_effects; pull integration (loop branch,
                               capture, echo mix)
  recipe.rs                  ← +DeckInitConfig, +RecipeOutput.outgoing_init/incoming_init;
                               +EchoOut, +LoopOut; +RecipeChoice::{EchoOut, LoopOut}
  scheduler.rs               ← apply outgoing_init/incoming_init after cue, before play
  config.rs                  ← +ForceRecipe::{EchoOut, LoopOut} variants
  main.rs                    ← +EchoOut, +LoopOut arms in match
engine/tests/
  effects_test.rs            ← NEW: EchoLine + LoopBuffer unit tests
  automation_test.rs         ← +EchoMix, +LoopGate tests
  deck_test.rs               ← +cue resets effects, +echo audible, +loop freezes source
  recipe_test.rs             ← +EchoOut::build, +LoopOut::build tests
  mixer_integration_test.rs  ← +EchoOut, +LoopOut continuous-audio tests
```
