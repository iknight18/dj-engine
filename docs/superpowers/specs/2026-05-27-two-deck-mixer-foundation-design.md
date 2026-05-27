# Two-Deck Mixer — Foundation (Sub-project 1)

**Date:** 2026-05-27
**Status:** Approved for planning
**Branch:** `research/two-deck-mixer`
**Sub-project:** 1 of 5 (see Roadmap)

## Summary

Replace the engine's current "one persistent decoder + a transient in-mixer decode of the incoming track" with **two persistent, controllable deck pipelines** summed by a **tick-driven virtual mixer**. v1 ships only the foundation plus a drop-in transition recipe that reproduces today's default `equal_power_crossfade` audio behavior. Hard-cut and the pitch-journey tempo ramp port to the new model. Everything that today's engine does audibly stays the same; the internals get the real-DJ-style two-deck model.

This is the foundation for sub-projects 2–5 (per-deck EQ + bass-swap, one-knob filter + filter transitions, echo and loop effects, selector-picks-recipe).

## Goals

- Persistent `Deck` objects (one each for Deck A and Deck B), reused across the entire engine lifetime — not destroyed and re-created per transition.
- Each deck owns its own ffmpeg+atempo subprocess and exposes a `pull(n_frames) -> Vec<f32>` interface plus a small state-bag (fader, tempo, position, play state).
- A `Mixer` that ticks at a fixed block size, pulls from each deck in parallel, applies each deck's current fader and any active automations, sums, and pushes into the output ring buffer.
- An `Automation` primitive (timed, curved change to one parameter on one deck) as the only thing the mixer evaluates per tick.
- A `Recipe` layer on top of `Automation` so transitions are expressed as named styles compiling down to automation lists. v1 ships exactly two recipes: `equal_power_crossfade` (matches today's default 8/16/32-bar volume crossfade) and `hard_cut` (matches today's 0-bar variant). The pitch-journey ramp is expressed as a follow-up `Tempo` automation.
- Foundation code (deck, automation, recipe, scheduler) sized so sub-projects 2–5 can extend it without touching the mixer tick loop.

## Non-Goals (sub-project 1)

- No per-deck EQ (sub-project 2). The existing `mix_crossfade_eq` / `use_eq_swap` master-bus EQ behavior is **temporarily not produced by v1**. See "Known temporary regressions" below.
- No per-deck filter (sub-project 3), echo, or loop (sub-project 4).
- Selector unchanged. It still returns just a next track; transition style is determined inside the engine by `plan_transition` exactly as today (sub-project 5 adds selector-driven recipe choice).
- No live MIDI/control input, no API surface change, no Icecast pipeline changes, no new ffmpeg flags, no new heavy deps.
- No change to sidecar schema or analyzer.

## Mental model

```
                          ┌──────────────────────┐
                          │  TransitionScheduler │
                          │  (tokio task)        │
                          └──────────┬───────────┘
                                     │ arms automations
                                     ▼
        ┌─────────────────┐   ┌─────────────────┐
        │     Deck A      │   │     Deck B      │
        │  ffmpeg+atempo  │   │  ffmpeg+atempo  │
        │  fader, tempo,  │   │  fader, tempo,  │
        │  position       │   │  position       │
        │  + AutomationQ  │   │  + AutomationQ  │
        └────────┬────────┘   └────────┬────────┘
                 │ pull(N) → PCM       │ pull(N) → PCM
                 ▼                     ▼
              ┌─────────────────────────────┐
              │   Mixer (tokio tick loop)   │
              │  every tick of N frames:    │
              │   sample = pcmA + pcmB      │
              │   (curves applied inside    │
              │   pull, so this is just     │
              │   a sum)                    │
              └──────────────┬──────────────┘
                             │ push N frames
                             ▼
                  ┌─────────────────────┐
                  │   ring buffer (10s) │
                  └──────────┬──────────┘
                             ▼
                  ┌─────────────────────┐
                  │ encoder ffmpeg pipe │
                  └─────────────────────┘
```

At any moment one deck is the **master** (fader=1.0, the audience hears it) and the other is either **idle** (no decoder, fader=0.0, silence pulls) or **cueing** (decoder running, fader=0.0, positioned and counting samples toward the transition). After a transition completes, master/idle roles swap: the freshly-mixed-in deck is now the master; the other deck unloads its decoder and goes idle. The mixer doesn't know or care which is which — it just sums.

## Components

### Component 1 — `Deck`

```rust
// engine/src/deck.rs

pub struct Deck {
    id: DeckId,                       // DeckA | DeckB — used only in logs
    state: DeckState,
    decoder: Option<DeckDecoder>,     // None when idle
    automations: AutomationQueue,
    frame_cursor: u64,               // master-clock samples this deck has emitted
}

#[derive(Clone)]
pub struct DeckState {
    pub fader: f32,                   // 0.0..1.0 linear; applied as gain
    pub tempo: f64,                   // atempo ratio (1.0 = native)
    pub track: Option<TrackHandle>,
    pub play_state: PlayState,        // Stopped | Cueing | Playing
}

pub enum PlayState { Stopped, Cueing, Playing }

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum DeckId { A, B }
```

**API:**

```rust
impl Deck {
    pub fn new(id: DeckId) -> Self;

    /// Load a track and prepare to play from `source_start_seconds`. Spawns the
    /// ffmpeg subprocess at the current tempo. After this call the deck is in
    /// Cueing state with fader=0; nothing reaches the mix until fader > 0.
    pub async fn cue(&mut self, track: TrackHandle,
                     source_start_seconds: f64) -> Result<()>;

    /// Begin emitting samples on the next pull. Cueing → Playing.
    pub fn play(&mut self);

    /// Pull `n_frames` of stereo PCM. The deck:
    ///   1. Advances `frame_cursor` by n_frames regardless of state.
    ///   2. Evaluates active automations targeting this deck and updates
    ///      `state.fader` / `state.tempo` per-sample as needed.
    ///   3. If Stopped or Cueing: returns a buffer of zeros of length n_frames*2.
    ///   4. If Playing: reads from the decoder, applies the per-sample fader,
    ///      handles tempo changes (subprocess respawn — see Section "atempo
    ///      lifecycle"), returns the gain-modulated PCM.
    pub async fn pull(&mut self, n_frames: usize) -> Result<Vec<f32>>;

    /// Queue an automation. Multiple automations on the same param can overlap;
    /// the latest-starting one wins for the overlap window (single-writer semantics
    /// — sub-project 1 never relies on overlap).
    pub fn schedule(&mut self, automation: Automation);

    /// Stop the deck, kill the decoder, free resources. Playing/Cueing → Stopped.
    pub async fn unload(&mut self);

    pub fn state(&self) -> &DeckState;
    pub fn frame_cursor(&self) -> u64;
    pub fn id(&self) -> DeckId;
}
```

**Key invariants:**

- Exactly two `Deck` instances live for the lifetime of the engine.
- Decks own their decoder; the decoder's atempo lifecycle is hidden inside the deck.
- Pull on a Stopped or Cueing deck returns `n_frames * 2` zeros, so the mixer can blindly sum both decks every tick.
- `frame_cursor` advances by `n_frames` on every pull regardless of state. This gives every automation an unambiguous absolute time anchor: `t_start` and `t_end` are frame_cursor values.

### Component 2 — `Automation` primitive

```rust
// engine/src/automation.rs

#[derive(Clone, Copy, Debug)]
pub struct Automation {
    pub target: AutomationTarget,
    pub t_start_frames: u64,        // absolute master-clock sample (per-deck)
    pub t_end_frames: u64,          // exclusive
    pub from: f64,
    pub to: f64,
    pub curve: Curve,
}

#[derive(Clone, Copy, Debug)]
pub enum AutomationTarget {
    Fader,
    Tempo,
    // sub-project 2: EqLow, EqMid, EqHigh
    // sub-project 3: Filter
    // sub-project 4: EchoMix, LoopGate
}

#[derive(Clone, Copy, Debug)]
pub enum Curve {
    Linear,
    EqualPowerIn,                    // sin(t * π/2): 0 → 1
    EqualPowerOut,                   // cos(t * π/2): 1 → 0
    Step,                            // instant jump at t_start
}

impl Curve {
    /// Evaluate for `t` in [0, 1]. Outside [0,1] callers must clamp.
    pub fn eval(self, t: f64) -> f64;
}

impl Automation {
    /// Returns Some(value) if `cursor` is inside [t_start, t_end), else None.
    /// `cursor < t_start` and `cursor >= t_end` both return None — the caller
    /// is responsible for applying `from` before the start and `to` after the end
    /// (or letting the deck's current parameter remain as-is).
    pub fn sample_value(&self, cursor: u64) -> Option<f64>;
}

pub struct AutomationQueue {
    pending: Vec<Automation>,
}

impl AutomationQueue {
    /// Returns automations active at `cursor`, partitioned by target.
    /// Also drops any automation whose `t_end_frames <= cursor` after first
    /// applying its `to` value (so terminated automations leave their final
    /// value installed in the deck state).
    pub fn step(&mut self, cursor: u64,
                state: &mut DeckState);
}
```

The mixer never sees `Automation` or `Curve` directly. They live inside the deck and are consumed by `Deck::pull` which calls `AutomationQueue::step` once per tick.

### Component 3 — `Recipe` layer

```rust
// engine/src/recipe.rs

pub trait Recipe {
    fn name(&self) -> &'static str;
    fn build(&self, ctx: &TransitionContext) -> RecipeOutput;
}

pub struct TransitionContext<'a> {
    pub outgoing: &'a LoadedTrack,
    pub incoming: &'a LoadedTrack,
    pub plan: &'a TransitionPlan,            // from existing transition.rs
    pub master_clock_frames: u64,           // when the transition begins
    pub outgoing_deck: DeckId,               // which deck holds outgoing
    pub incoming_deck: DeckId,               // which deck holds incoming
}

pub struct RecipeOutput {
    pub outgoing_automations: Vec<Automation>,
    pub incoming_automations: Vec<Automation>,
    pub total_duration_samples: u64,         // includes any pitch-journey tail
}
```

**v1 recipes:**

1. **`EqualPowerCrossfade`** — the only volume-curve recipe in v1. Maps to today's `mix_crossfade` behavior. For a `length_bars` crossfade at outgoing's BPM:
   - `outgoing.Fader`: `[t0 .. t0+L]` `from=1.0 to=0.0` curve=`EqualPowerOut`
   - `incoming.Fader`: `[t0 .. t0+L]` `from=0.0 to=1.0` curve=`EqualPowerIn`
   - `incoming.Tempo`: `[t0 .. t0+L]` `from=ratio to=ratio` curve=`Step`
   - **Pitch-journey tail** (only if `(ratio - 1.0).abs() >= 0.005` and source headroom allows): `incoming.Tempo` `[t0+L .. t0+L+ramp]` `from=ratio to=1.0` curve=`Linear`. The deck's tempo-automation handling (Section "atempo lifecycle") respawns ffmpeg in chunks during this ramp, exactly as today's `decode_stretched_ramp` does.

2. **`HardCut`** — the `length_bars == 0` case. Produces:
   - `outgoing.Fader`: `[t0 .. t0+1]` `from=1.0 to=0.0` curve=`Step`
   - `incoming.Fader`: `[t0 .. t0+1]` `from=0.0 to=1.0` curve=`Step`
   - `incoming.Tempo`: `[t0 .. t0+1]` `from=ratio to=ratio` curve=`Step`
   - Plus the same pitch-journey tail as `EqualPowerCrossfade`.

The choice between recipes is made by `plan_transition` exactly as today: `length_bars == 0` → `HardCut`, otherwise `EqualPowerCrossfade`. (Sub-project 5 moves this to selector-driven.)

### Component 4 — `Mixer` tick loop

```rust
// engine/src/mixer.rs (rewritten)

pub struct Mixer {
    deck_a: Deck,
    deck_b: Deck,
    producer: SampleProducer,
    master_clock: u64,               // shared sample counter (each pull advances it)
}

impl Mixer {
    pub async fn tick(&mut self) -> Result<()> {
        // Pull both decks in parallel. Each pull internally evaluates its
        // own automations and advances its frame_cursor by TICK_FRAMES.
        let (a_pcm, b_pcm) = tokio::join!(
            self.deck_a.pull(TICK_FRAMES),
            self.deck_b.pull(TICK_FRAMES),
        );
        let a = a_pcm?;
        let b = b_pcm?;

        // Sum sample-by-sample. Both buffers are TICK_FRAMES*2 samples
        // (zeros if the deck was Stopped/Cueing).
        let mut mixed = Vec::with_capacity(a.len());
        for i in 0..a.len() {
            mixed.push(a[i] + b[i]);
        }

        push_all(&mut self.producer, &mixed).await;
        self.master_clock += TICK_FRAMES as u64;
        Ok(())
    }

    pub fn master_clock(&self) -> u64 { self.master_clock }
    pub fn deck(&mut self, id: DeckId) -> &mut Deck;
}

pub const TICK_FRAMES: usize = 4096;
```

That's it. The mixer doesn't see recipes, plans, or transitions — only deck pulls + sums. Producer-bounded pacing: when the ring buffer is full, `push_all` backpressures and the next tick doesn't happen until there's room. Matches today's pattern.

**Tick block size:** `TICK_FRAMES = 4096` (~93 ms at 44.1 kHz). Smaller block = finer-grained automation timing but more overhead. Automation timing granularity worst-case is `TICK_FRAMES/2` frames. For 8-bar+ crossfades (~16 s at 120 BPM), an automation value being off by ~46 ms in a 16 s curve is inaudible.

### Component 5 — `TransitionScheduler`

```rust
// engine/src/scheduler.rs

pub struct TransitionScheduler;

impl TransitionScheduler {
    /// Arm a transition. Cues the incoming deck, builds automations from the
    /// recipe, schedules them on both decks, returns when the transition has
    /// been fully scheduled (NOT when it completes — the mixer plays it out).
    /// After completion, the caller is responsible for unloading the outgoing
    /// deck and swapping master/idle roles.
    pub async fn arm<R: Recipe>(
        mixer: &mut Mixer,
        outgoing_deck: DeckId,
        incoming_deck: DeckId,
        outgoing: &LoadedTrack,
        incoming: &LoadedTrack,
        plan: &TransitionPlan,
        recipe: &R,
    ) -> Result<TransitionHandle>;
}

pub struct TransitionHandle {
    pub end_master_clock_frames: u64,  // when total_duration completes
}
```

**Flow:**

1. Compute `t_start_frames` = `master_clock` + `(plan.outgoing_crossfade_start_seconds - outgoing_deck's current source position) * SAMPLE_RATE`.
2. **Pre-cue the incoming deck**: `mixer.deck(incoming_deck).cue(incoming, plan.incoming_crossfade_start_seconds - 1 bar of headroom)`. The 1-bar head room (~2 s at 120 BPM) absorbs ffmpeg spawn latency comfortably.
3. **Build automations**: `let output = recipe.build(&ctx);`
4. Schedule all `outgoing_automations` on the outgoing deck and `incoming_automations` on the incoming deck.
5. **Play the incoming deck**: `mixer.deck(incoming_deck).play();` — silence keeps pulling until the fader-in automation fires at `t_start_frames`.
6. Return `TransitionHandle { end_master_clock_frames: t_start_frames + output.total_duration_samples }`.

### atempo lifecycle inside a Deck

The hardest part of the refactor; details matter.

**Three regimes** the deck switches between, based on `state.tempo`:

1. **Native (`|tempo - 1.0| < 0.005`)** — long-running ffmpeg subprocess with no `-af atempo`. Existing `PlayingDecoder::spawn` exactly.

2. **Steady stretched (`|tempo - 1.0| >= 0.005`, no active Tempo automation)** — long-running ffmpeg subprocess with `-af atempo=R`. Same `PlayingDecoder::spawn` pattern, plus the atempo flag.

3. **Ramping (active Tempo automation with `from != to`)** — chunked, exactly as today's `decode_stretched_ramp`:
   - Break the ramp into N chunks of equal output duration (default chunk = 4 bars at outgoing's BPM, matching today's `RAMP_CHUNK_BARS = 4`).
   - For each chunk: spawn a short-lived ffmpeg subprocess seeked to the deck's current source position, with atempo at the chunk's midpoint-interpolated ratio. Read its full output. Kill it. Advance source position by `output_seconds * ratio + ATEMPO_PER_CALL_OVERHEAD_S`.
   - Pre-spawn chunk N+1 in parallel with chunk N's playout (the deck holds a `next_chunk: Option<JoinHandle<DeckDecoder>>` slot so spawn latency doesn't cause underruns).

**Regime transitions:**

- **Native → Steady stretched** (when `Tempo from=1.0 to=R` `curve=Step` fires): kill the current decoder, respawn with `-af atempo=R`, seek to current source position. A SPLICE_FADE_MS (300 ms) short equal-power fade hides any seam.
- **Steady stretched → Native** (when `Tempo to=1.0 curve=Step` fires, or after a ramp completes at 1.0): same — kill, respawn without atempo, splice-fade.
- **Steady stretched → Ramping** (when a `Tempo curve=Linear` fires): switch to chunked mode. The chunked decoder takes over from the steady decoder; splice-fade between them.
- **Ramping → Native** (ramp finishes, end_ratio=1.0): the last chunk's subprocess is short-lived; spawn the native long-running decoder seeked to the source position the ramp ended at; splice-fade.
- **Ramping → Steady stretched** (ramp finishes at non-1.0): same with atempo flag.

The SPLICE_FADE_MS hides ATEMPO_PER_CALL_OVERHEAD_S misalignment (~50–200 ms variance). Same trick as today.

**Source position tracking:** the deck knows its current source position in seconds at all times. After a pull, `source_pos += emitted_output_seconds / current_tempo + (overhead if chunked)`. This lets us seek correctly on any subprocess respawn.

### Migration plan

**New files:**

- `engine/src/deck.rs` — `Deck`, `DeckState`, `DeckId`, `PlayState`, `DeckDecoder` (internal — wraps `PlayingDecoder` + ramp chunks)
- `engine/src/automation.rs` — `Automation`, `AutomationTarget`, `Curve`, `AutomationQueue`
- `engine/src/recipe.rs` — `Recipe` trait, `TransitionContext`, `RecipeOutput`, `EqualPowerCrossfade`, `HardCut`
- `engine/src/scheduler.rs` — `TransitionScheduler`, `TransitionHandle`
- `engine/tests/deck_test.rs`, `automation_test.rs`, `recipe_test.rs`, `scheduler_test.rs`, `two_deck_drop_in_test.rs`

**Modified files:**

- `engine/src/mixer.rs` — rewritten. The top-level `play_until_transition_and_crossfade` is replaced by a `Mixer` struct with a `tick()` method. A new `run_transition` orchestrator wraps `TransitionScheduler::arm` + ticking until completion + swapping master/idle. `mix_crossfade` / `mix_crossfade_eq` / the post-crossfade tail logic / the splice-fade logic / the ramp logic — all gone from mixer.rs (moved into deck.rs and recipe.rs respectively).
- `engine/src/main.rs` — instantiates two Decks; main loop becomes: `select → recipe::pick(plan) → scheduler.arm → tick until end → unload outgoing → swap`.
- `engine/src/lib.rs` — declare new modules.
- `engine/src/crossfade.rs` — slimmed down. Keep `equal_power_curve` (used by `Curve::EqualPowerIn/Out`). Delete `mix_crossfade`, `mix_crossfade_eq`, `BassMixHint`. These do not come back as-is — sub-project 2 introduces a different implementation (per-deck EQ automations) that doesn't need them.
- `engine/src/transition.rs` — unchanged. `TransitionPlan` still describes the boundary; `use_eq_swap` is computed but **not consumed** in v1 (logged for diagnostics, will be consumed by sub-project 2's recipe selection).
- `engine/src/ffmpeg/mixwindow.rs` — `decode_atempo_at_source`, `decode_stretched_ramp`, `ATEMPO_PER_CALL_OVERHEAD_S`, `ramp_source_consumed_seconds`, `atempo_chain` all move (or are re-exported) into `deck.rs`'s internal helpers. Public surface deleted from `ffmpeg::mod`.
- `engine/src/ffmpeg/mod.rs` — drop the `mixwindow` module declaration.

**Deletions of dead code paths after the refactor:**

- `mix_crossfade`, `mix_crossfade_eq`, `BassMixHint` — deleted. Sub-project 2's per-deck EQ implementation doesn't reuse these.
- `eq.rs::apply_swept_filter` — kept; sub-project 3 reuses it for the one-knob filter. In v1 it has no callers but is `pub` for module re-export and gets a `#[allow(dead_code)]` annotation pending sub-project 3. (User preference is to delete dead code; this is the one exception, justified because sub-project 3 will land within the same research branch.)

### Testing strategy

**Unit tests (each new module gets its own):**

- `automation.rs`
  - `Curve::eval` at t=0, 0.25, 0.5, 0.75, 1.0 for Linear, EqualPowerIn, EqualPowerOut, Step
  - `Automation::sample_value` at boundaries (cursor == t_start, cursor == t_end - 1, cursor < t_start, cursor >= t_end)
  - `AutomationQueue::step` correctly applies and drops finished automations, leaves final `to` value installed

- `recipe.rs`
  - `EqualPowerCrossfade::build` with a synthetic ctx produces:
    - Exactly 1 outgoing automation: Fader 1.0→0.0 EqualPowerOut over `length_bars * bar_seconds * SAMPLE_RATE` frames starting at `master_clock_frames`
    - Exactly 2 incoming automations when `ratio ≈ 1.0`: Fader 0.0→1.0 EqualPowerIn over the same window, and Tempo Step `from=ratio to=ratio` (no-op but documents the deck's tempo)
    - Exactly 3 incoming automations when `ratio != 1.0` and headroom allows: the two above plus a pitch-journey tail Tempo Linear from `ratio` to `1.0` over `RAMP_BARS * bar_seconds * SAMPLE_RATE` frames immediately after the crossfade
    - When headroom does not allow a full ramp: ramp duration clamped to available, or skipped entirely if below 1 bar (matches today's `effective_ramp_seconds` logic)
  - `HardCut::build` produces Step automations of 1-sample duration
  - Determinism: same ctx → same output

- `deck.rs`
  - `pull(n)` on Stopped deck → `n*2` zeros
  - `pull(n)` on Cueing deck → `n*2` zeros (decoder spawned but no samples emitted)
  - `pull(n)` advances `frame_cursor` by `n` in all states
  - `schedule` + `pull` honors Fader automation (Linear 0→1 over 100 samples, pull 50, check fader values ramp 0 → 0.5)
  - `pull` honors Tempo automation Step (decoder respawn happens, splice-fade applied, no audible discontinuity in a synthetic 440 Hz sine)

- `scheduler.rs`
  - `arm` cues incoming deck at correct source position
  - `arm` schedules the right counts of automations on each deck
  - `arm` returns `TransitionHandle` with the correct `end_master_clock_frames`

**Integration tests:**

- `tests/two_deck_mixer_test.rs` — Two synthetic stereo sine-wave tracks (440 Hz and 660 Hz). Run the full mixer (without Icecast — just consume from the ring buffer into a Vec). Execute one transition. Assert:
  - Output PCM has no NaN, no clipping (abs <= 1.5 sample-wise, since fader ramp can be slightly above 1.0 mid-crossfade is impossible — they're equal-power so it sums to constant power not constant gain)
  - During the crossfade window the FFT shows both 440 Hz and 660 Hz components
  - Before the crossfade, only 440 Hz; after, only 660 Hz
  - Output is continuous (no gaps of zeros > 1 ms)

- `tests/two_deck_drop_in_test.rs` — **Drop-in verification.** Two synthetic tracks with known sidecars chosen so `plan_transition` returns `length_bars=16` and `use_eq_swap=false` (i.e., today's vanilla crossfade case). Render the same transition via:
  1. The old engine (`play_until_transition_and_crossfade`, via a git-stash or pinned snapshot)
  2. The new engine (`Mixer::tick`-driven)
  Compute per-sample absolute difference; assert mean absolute difference < 1e-3 (audibly identical). The chunked atempo timing may produce ±1–2 samples of phase shift at chunk boundaries, so the test correlates the two outputs and aligns before diffing.

**Drop-in verification methodology (manual):**
- Set up a 5-track test library where most pairs hit `use_eq_swap=false`.
- Run today's engine → record 10 minutes of MP3 stream from Icecast.
- Run new engine → record 10 minutes from the same seed track.
- A/B listen with headphones — they should be indistinguishable for `use_eq_swap=false` transitions.
- For `use_eq_swap=true` transitions, expect a documented temporary regression (see below).

### Known temporary regressions (sub-project 1 only)

- **Bass-swap EQ disappears.** Today, `plan_transition` returns `use_eq_swap=true` for phrase-aligned crossfades of length ≥ 8 bars; those transitions today use `mix_crossfade_eq` which applies a master-bus high-pass sweep on each side to swap bass at the midpoint. In v1, all crossfades are plain equal-power (the EQ swap is not produced). Sub-project 2 restores it as per-deck EQ automations, which is a cleaner implementation. Net listening impact: a small loss of bass-clash mitigation on a subset of transitions for the duration of one sub-project. Acceptable risk; flagged in commit message.

- **Selector-driven recipe choice (today: none).** No regression — sub-project 1 keeps today's `plan_transition`-driven recipe choice. Sub-project 5 changes this.

### Risks

- **Deck pre-spawn timing.** If the incoming deck is cued less than ~500 ms before its first sample is needed, ffmpeg spawn latency causes underruns. Mitigation: scheduler.arm cues with 1 bar (~2 s) of headroom, plenty of margin.
- **Drop-in A/B tolerance calibration.** Setting the diff tolerance too tight will make the test flaky (chunked atempo + splice-fade introduces sample-level variance); too loose and real regressions slip through. Initial proposal: mean abs diff < 1e-3 over the full transition, peak abs diff < 1e-2. Tune in the test if needed.
- **Master clock vs. per-deck cursor drift.** The mixer's master clock and each deck's `frame_cursor` should advance in lockstep (every tick advances both by `TICK_FRAMES`). A bug that lets them desync would mis-time all automations. Defensive: an assertion in `Mixer::tick` that both deck cursors equal master_clock + TICK_FRAMES after the pull.
- **atempo regime-switch splice-fades during a transition.** If a Tempo Step happens at t_start and a fader ramp also happens at t_start, the splice-fade on the deck might briefly fight the fader-in curve. Mitigation: the splice-fade is applied inside `Deck::pull` to the raw PCM **before** the fader multiplier — they compose multiplicatively, no fight.

### Open questions

- **Should `TICK_FRAMES` be a runtime CLI flag for testing?** Probably yes; default 4096 covers all real-world cases.
- **What about the icecast metadata midpoint update?** Currently fires at the midpoint of the crossfade in `mixer.rs`. In v1, the scheduler fires it at `master_clock + total_duration/2`. Same audible behavior.

## Sub-project 1 deliverables

- ✅ New code: `deck.rs`, `automation.rs`, `recipe.rs`, `scheduler.rs`
- ✅ Rewritten `mixer.rs`, `main.rs`
- ✅ Slimmed `crossfade.rs`
- ✅ Internalized `mixwindow.rs`
- ✅ Tests: unit per module + 2 integration + drop-in A/B
- ✅ `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` all green
- ✅ Manual A/B listening test passes for `use_eq_swap=false` transitions

## Roadmap (sub-projects 2–5)

Each sub-project gets its own design doc + plan and is committed as stacked commits on `research/two-deck-mixer`. They are independently shippable (each leaves the engine releasable).

### Sub-project 2 — Per-deck EQ + bass-swap migration

Add 3-band EQ (low / mid / high, gain in dB, kill-capable) to each `Deck`. New `AutomationTarget` variants: `EqLow`, `EqMid`, `EqHigh`. Implement as a chain of 3 biquads per deck (low-shelf, peaking-mid, high-shelf) applied inside `Deck::pull` after the decoder and before the fader. Port today's bass-swap behavior to a new `BassSwap` recipe that emits per-deck EQ automations replicating the master-bus sweep — but cleaner because each deck owns its own EQ. The `plan_transition` selector still drives recipe choice (`use_eq_swap=true` → `BassSwap`, else `EqualPowerCrossfade`).

### Sub-project 3 — One-knob filter + filter transitions

Add a one-knob filter to each `Deck` (-1.0 = full low-pass sweep, 0 = bypass, +1.0 = full high-pass sweep). One biquad per deck, swept frequency curve. New `AutomationTarget`: `Filter`. New recipes: `FilterFade` (incoming HP-sweeps down while fading in), `FilterKill` (outgoing HP-sweeps up to silence before incoming enters).

### Sub-project 4 — Effects (echo + loop)

Echo: per-deck IIR delay line (single tap with feedback, time = N beats). New target `EchoMix`. Loop: extend `DeckDecoder` to support capturing the last K bars of decoded PCM into an in-memory buffer and replaying it in a loop until a Loop-end automation fires. New target `LoopGate`. New recipes: `EchoOut` (outgoing echo crescendos while incoming enters), `LoopOut` (outgoing locks into a 4-bar loop while incoming swaps in).

### Sub-project 5 — Selector picks transition style

Extend selector to return `(next_track, recipe)` rather than just `(next_track)`. Scoring extends to favor recipes that fit the pair's BPM gap, bass content, key relationship, and energy delta. The engine main loop becomes `(track, recipe) = selector.pick()` → `scheduler.arm(track, recipe)`. `plan_transition` becomes a helper consumed by the selector rather than the authority on recipe choice.

## Project Layout (post sub-project 1)

```
engine/
  Cargo.toml
  src/
    main.rs                   ← rewritten
    lib.rs                    ← +deck, +automation, +recipe, +scheduler
    deck.rs                   ← NEW
    automation.rs             ← NEW
    recipe.rs                 ← NEW
    scheduler.rs              ← NEW
    mixer.rs                  ← rewritten (Mixer struct + tick loop)
    crossfade.rs              ← slimmed
    transition.rs             ← unchanged
    eq.rs                     ← unchanged (callers removed in v1; reused in sub-project 2)
    align.rs, camelot.rs,
    energy_avg.rs, library.rs,
    phrase.rs, recency.rs,
    selector.rs, sidecar.rs,
    icecast_meta.rs,
    logging_setup.rs,
    supervisor.rs,
    buffer.rs, error.rs,
    config.rs
    ffmpeg/
      mod.rs                  ← drop mixwindow declaration
      decoder.rs              ← unchanged
      encoder.rs              ← unchanged
      (mixwindow.rs deleted; logic moved into deck.rs)
  tests/
    deck_test.rs              ← NEW
    automation_test.rs        ← NEW
    recipe_test.rs            ← NEW
    scheduler_test.rs         ← NEW
    two_deck_mixer_test.rs    ← NEW
    two_deck_drop_in_test.rs  ← NEW
    (existing tests stay; mixer_integration_test.rs and crossfade tests rewritten to drive new API)
```
