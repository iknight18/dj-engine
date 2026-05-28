# Phase D — Stem-Aware Recipes (VocalOverlay + BassSwapStemAware + BreakdownIntoDrop)

**Date:** 2026-05-28
**Status:** Approved for planning
**Branch:** `research/two-deck-mixer`
**Sub-project:** engine deck-level multi-stem decoding + 3 stem-aware recipes (Phase D.1)

## Summary

Teach each deck to load **4 stem audio buffers** (drums, bass, vocals, other) alongside or instead of the existing mixed source, and mix them per-frame using independently automatable per-stem fader values. Add four new `AutomationTarget`s (`StemFaderDrums`, `StemFaderBass`, `StemFaderVocals`, `StemFaderOther`) and a new `Phase::StemFader` variant in the recipe DSL. Build three new recipes on top: `VocalOverlay` (keep outgoing's instrumental, ride incoming's vocals on top), `BassSwapStemAware` (snap the bass kill/release windows to outgoing's bass-density valley and incoming's bass-density peak), `BreakdownIntoDrop` (anchor the transition to outgoing's drum-density valley → incoming's drum-density peak with a hard cut on the latter's first downbeat). Stem-aware recipes only fire when `|bpm_ratio - 1.0| < 0.01` (effectively identical BPM after analyzer quantization) — this dodges the time-stretch question for v1. Also includes a small **Task 0**: deprioritize `EqualPowerCrossfade` (0.50 → 0.15 baseline) and `EchoOut` (0.35 → 0.20) so the picker spends more rotation on character-bearing recipes.

## Motivation

The user's running criticism — "feels like radio, songs just passing" — has two halves:

1. **Transition-level**: gestures all feel like crossfades. Largely addressed by event recipes (`Drop`, `BreakdownBridge`, `DoubleDrop`) shipping in the previous spec.
2. **Stem-level**: real DJs don't only crossfade *whole tracks*; they manipulate *parts* of tracks — kill the bass on one deck while another's bass drops, layer an a-cappella over an instrumental, drop a track precisely on its own kick after the previous track's drum breakdown. None of that is possible with the current mixed-source-only architecture.

The analyzer has been emitting stems data for every track (4 per-bar density arrays + vocal segments + 4 stem audio files at `analyzed-real-stems/<track>/`). The engine reads it into the sidecar but doesn't *use* it. This sub-project closes the loop: deck-level multi-stem decoding makes the analytics actionable.

## Goals

### Infrastructure (D.1.A)

- New `StemKind` enum: `Drums`, `Bass`, `Vocals`, `Other`.
- `DeckState` gains a `stem_faders: [f32; 4]` field (default all 1.0 — backward-compatible). Indexed by `StemKind`'s discriminant.
- 4 new `AutomationTarget` variants: `StemFaderDrums`, `StemFaderBass`, `StemFaderVocals`, `StemFaderOther`. `apply_value` maps each to the right slot in `stem_faders`.
- `DeckSource` gains a `Stems` variant: `{ drums: Vec<f32>, bass: Vec<f32>, vocals: Vec<f32>, other: Vec<f32>, position: usize, sample_rate: u32 }`. All four buffers are mono f32 PCM at 44.1 kHz (the rate Demucs writes).
- `Deck` gains a `cue_stems(track, offset_seconds)` method: locates `analyzed-real-stems/<basename>/{drums,bass,vocals,other}.mp3` next to the track's sidecar, decodes each via the existing ffmpeg path into a `Vec<f32>` (mono), and queues a `DeckSource::Stems` instead of the usual `DeckSource::Decoder`.
- `DeckSource::Stems::read_frames(n)` reads `n` frames from each of the 4 buffers at the current position, multiplies each by `state.stem_faders[stem]`, sums to a single mono channel, advances position. If position exceeds the shortest buffer, returns end-of-stream.
- Compiler arm in `recipe_script::compile` for `Phase::StemFader { deck, stem, from, to, span, curve }` → emits one `Automation` targeting the right `StemFader*` variant.

### Recipes (D.1.B)

For each, **gate firing on `|bpm_ratio - 1.0| < 0.01`** in `score_recipe`. This sidesteps the time-stretch problem for v1: stem playback runs at natural rate; if BPMs aren't already identical (which is most of the library after the analyzer's octave-collapse), the recipe scores 0.05 and loses.

- **`VocalOverlay`**: 16-bar transition. Outgoing plays mixed-source (all 4 stems mixed at fader 1.0). Incoming cues in stems mode with all stems at 0; vocals fader ramps 0→1 over [0, 4] bars; vocals hold at 1 for bars [4, 12]; at bar 12, incoming's drums/bass/other all ramp 0→1 over [12, 16] (the full track enters); outgoing fader equal-power-out over [12, 16]. Listener experience: outgoing keeps playing → incoming's vocal floats in on top for 8 bars (a-cappella overlay) → incoming's full instrumental joins in the last 4 bars → outgoing dies.

- **`BassSwapStemAware`**: 16-bar transition. Both decks cue in stems mode. Inspects sidecar's `bass_density[]` arrays to find: (a) outgoing's lowest-density bar in the **last 4 bars of the transition window** (bars [12, 16] of the transition's frame of reference, which maps to bars [outro_start_bar + 12, outro_start_bar + 16] of the outgoing track) → outgoing's bass stem fader hard-cuts to 0 at that bar; (b) incoming's first bar where `bass_density >= 0.7` after the transition start → incoming's bass stem hard-cuts to 1 at that bar. Other stems crossfade equal-power across the window like a normal bass swap. Difference from existing `BassSwap`: the kill and release are timed to actual *audio* structure, not arbitrary 0.35 / 0.65 fractions.

- **`BreakdownIntoDrop`**: 8-bar transition. Picker requires outgoing has a drum_density valley near its outro AND incoming has a drum_density peak near its intro. The transition window is timed so bar 0 of the window aligns with outgoing's valley start, and the hard cut (outgoing fader 1→0 step, incoming fader 0→1 step) lands on incoming's first peak bar. Both decks cue in stems mode (so the picker can verify density arrays at transition planning time). During the 8 bars: outgoing fader stays high, outgoing's drum stem fader gradually drops to 0 (deepens the breakdown). At the hard-cut bar, outgoing fader cuts to 0 and incoming fader cuts to 1 simultaneously — listener hears outgoing's breakdown drift away, then a hard drop into incoming's peak.

### Picker / CLI / Tests

- `RecipeChoice` gains 3 variants: `VocalOverlay`, `BassSwapStemAware`, `BreakdownIntoDrop`.
- `score_recipe` gains 3 branches. All three include the `|bpm_ratio - 1.0| < 0.01` gate.
- `PairScoreInputs` gains a `stems_available: bool` field (true when both tracks have non-empty stems blocks). Stem-aware recipes return 0 score when `stems_available == false`.
- `recipe_choice_id` gains 3 entries (14, 15, 16).
- `recency_hash` iterates all 16 variants.
- `pick_recipe` candidates array grows by 3.
- `--force-recipe` gains 3 values.
- `main.rs::recipe_choice_name` / `recipe_name` extended.
- `main.rs` scheduler-arm match extended with 3 new arms.

### Task 0 — recipe pruning

- `score_recipe::EqualPowerCrossfade`: change baseline from `0.5` to `0.15`.
- `score_recipe::EchoOut`: change `0.35` baseline to `0.20`, change the energy-delta-driven branch from `0.65` to `0.45`.

## Non-Goals

- **No tempo-aware stem playback.** Stems play at their native sample rate. If `bpm_ratio != 1.0`, stem-aware recipes don't fire. Time-stretching stems on the fly is Phase D.2.
- **No per-stem EQ/filter/echo/loop.** The existing post-mix chain (EQ, filter, echo, loop, mixed fader) still runs on the SUM of the stems. Per-stem fader is the only stem-targeted parameter.
- **No stems-mode scratch/seek.** `cue_stems(offset_seconds)` works; mid-playback seeks aren't supported (no recipe needs it).
- **No streaming stem decode.** All 4 stems load into memory at cue time. For a 4-min track at 44.1 kHz mono f32, that's 4 × ~40 MB = 160 MB per deck. Two decks → max ~320 MB. Acceptable for a desktop dev box; would need redesign for embedded.
- **No fallback rendering when stem files are missing.** If `cue_stems` can't find the 4 files, it returns an error and the recipe falls back through the picker (next-best recipe wins). The picker's `stems_available` gate prevents most of this.
- **No analyzer changes.** Phases D.2 / D.3 are pure engine work.

## Architecture details

### Stem loading on cue

```rust
// engine/src/deck.rs additions

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StemKind {
    Drums = 0,
    Bass = 1,
    Vocals = 2,
    Other = 3,
}

impl Deck {
    pub async fn cue_stems(
        &mut self,
        track: LoadedTrack,
        offset_seconds: f64,
        rate: f64,
    ) -> Result<(), DeckError> {
        let stems_dir = stems_dir_for(&track.sidecar_path)?;
        let drums = decode_stem_to_pcm(&stems_dir.join("drums.mp3")).await?;
        let bass = decode_stem_to_pcm(&stems_dir.join("bass.mp3")).await?;
        let vocals = decode_stem_to_pcm(&stems_dir.join("vocals.mp3")).await?;
        let other = decode_stem_to_pcm(&stems_dir.join("other.mp3")).await?;
        // Trim or pad to equal length (defensively).
        let len = drums.len().min(bass.len()).min(vocals.len()).min(other.len());
        let position = ((offset_seconds * 44_100.0).max(0.0) as usize).min(len);

        self.sources.push_back(DeckSource::Stems {
            drums: drums.into_iter().take(len).collect(),
            bass: bass.into_iter().take(len).collect(),
            vocals: vocals.into_iter().take(len).collect(),
            other: other.into_iter().take(len).collect(),
            position,
            sample_rate: 44_100,
        });
        self.state.source_path = Some(track.sidecar.source_path.clone());
        self.state.play_state = PlayState::Cueing;
        self.state.tempo = rate;  // honored but no atempo applied to stems
        Ok(())
    }
}

fn stems_dir_for(sidecar_path: &Path) -> Result<PathBuf, DeckError> {
    // sidecar at "analyzed-real/001-song.mp3.json" → stems at "analyzed-real-stems/001-song/"
    let analyzed_dir = sidecar_path.parent().ok_or(DeckError::NoStemsDir)?;
    let stems_root = analyzed_dir
        .parent()
        .ok_or(DeckError::NoStemsDir)?
        .join(format!("{}-stems", analyzed_dir.file_name()
            .ok_or(DeckError::NoStemsDir)?
            .to_string_lossy()));
    let basename = sidecar_path.file_stem()
        .ok_or(DeckError::NoStemsDir)?
        .to_string_lossy()
        .trim_end_matches(".mp3")
        .to_string();
    let dir = stems_root.join(&basename);
    if !dir.exists() {
        return Err(DeckError::NoStemsDir);
    }
    Ok(dir)
}
```

`decode_stem_to_pcm` is a small wrapper around the existing ffmpeg decoder that produces mono f32 PCM at 44100 Hz (the rate Demucs writes). Should be 5-10 lines if the existing decoder API allows specifying output format.

### `DeckSource::Stems::read_frames`

```rust
fn read_frames_stems(
    drums: &[f32], bass: &[f32], vocals: &[f32], other: &[f32],
    position: &mut usize,
    faders: [f32; 4],
    n_frames: usize,
) -> Vec<f32> {
    let mut out = Vec::with_capacity(n_frames);
    for _ in 0..n_frames {
        if *position >= drums.len() { break; }
        let mixed =
            drums[*position]  * faders[0]
          + bass[*position]   * faders[1]
          + vocals[*position] * faders[2]
          + other[*position]  * faders[3];
        out.push(mixed);
        *position += 1;
    }
    out
}
```

Called from the existing per-frame audio path; the rest of the chain (EQ, filter, echo, loop, mixed fader) operates on `mixed` as before.

### Recipe phases — example for `VocalOverlay`

```rust
TransitionScript {
    length_bars: 16,
    phases: vec![
        Phase::TempoAlign { deck: DeckRole::Incoming, ratio },

        // Outgoing fades out only in the final 4 bars.
        Phase::Fader {
            deck: DeckRole::Outgoing, from: 1.0, to: 0.0,
            span: bars(12.0, 16.0), curve: Curve::EqualPowerOut,
        },

        // Incoming fader at 1.0 throughout — per-stem faders do the work.
        Phase::Fader {
            deck: DeckRole::Incoming, from: 1.0, to: 1.0,
            span: bars(0.0, 0.0), curve: Curve::Step,
        },

        // Kill incoming's drums, bass, other at bar 0.
        Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Drums, from: 0.0, to: 0.0, span: bars(0.0, 0.0), curve: Curve::Step },
        Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Bass,  from: 0.0, to: 0.0, span: bars(0.0, 0.0), curve: Curve::Step },
        Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Other, from: 0.0, to: 0.0, span: bars(0.0, 0.0), curve: Curve::Step },
        Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Vocals, from: 0.0, to: 0.0, span: bars(0.0, 0.0), curve: Curve::Step },

        // Bars [0, 4]: incoming vocals fade in.
        Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Vocals, from: 0.0, to: 1.0, span: bars(0.0, 4.0), curve: Curve::Linear },

        // Bars [12, 16]: incoming's other 3 stems fade in (the full track joins).
        Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Drums, from: 0.0, to: 1.0, span: bars(12.0, 16.0), curve: Curve::Linear },
        Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Bass,  from: 0.0, to: 1.0, span: bars(12.0, 16.0), curve: Curve::Linear },
        Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Other, from: 0.0, to: 1.0, span: bars(12.0, 16.0), curve: Curve::Linear },
    ],
    outgoing_init: None, incoming_init: None,
}
```

`BassSwapStemAware` and `BreakdownIntoDrop` follow the same pattern but consult `ctx.outgoing.sidecar.stems` / `ctx.incoming.sidecar.stems` to find specific bars where density peaks/valleys live.

### Routing — cue_stems vs cue

The scheduler arms a transition by calling `Deck::cue` on the incoming deck. For stem-aware recipes, the scheduler must call `cue_stems` instead. Two options:

- **A:** Recipe declares via a method `requires_stems(&self) -> bool` (default false). Scheduler reads it and chooses the right cue method.
- **B:** A new `cue_kind` field on the recipe's output (e.g., `DeckInitConfig::cue_mode: CueMode { Mixed | Stems }`) tells the scheduler which loader to use.

Choose **A** (single new method on `Recipe` trait, default returns `false`). Cleaner than threading a field through `DeckInitConfig`.

`main.rs`'s arm-match dispatch reads `recipe.requires_stems()` and decides. For the 13 existing recipes the default `false` is correct; for the 3 new ones, override to `true`.

Both decks need stems when stem-aware recipes fire. The OUTGOING is already cued (it's the currently-playing master). If the outgoing was cued in mixed mode and we need stems mid-set, we'd have to re-cue — which means seeking to the current playback position with stems loaded, swapping `DeckSource`s seamlessly. That's tricky. Two pragmatic options:

- **Eager**: every deck always cues in stems mode when stems are available. Even non-stem recipes benefit by setting per-stem faders to 1.0 (the mixed sum equals the original mix; small numerical drift from re-encoding but not perceptible). This eliminates the re-cue problem entirely.
- **Lazy**: outgoing cues in mixed mode; if the picker chooses a stem-aware recipe, re-cue outgoing in stems mode at the current position before arming.

Choose **Eager** — simpler, no mid-set re-cue. Cost: every track loads 160 MB of stems instead of 5 MB of mixed mp3. Two decks → ~320 MB. Memory budget is fine on a dev box. Could fall back to mixed mode if stem files are missing (deck cues normally in that case).

## Testing strategy

### Unit tests

1. `StemKind` discriminants are 0/1/2/3 (used to index `stem_faders`).
2. `apply_value` for `StemFaderDrums/Bass/Vocals/Other` writes the right index.
3. `compile()` arm for `Phase::StemFader` emits an `Automation` with the right target.
4. `DeckSource::Stems::read_frames`: with all faders at 1.0, output equals sum of 4 buffers. With one fader at 0, that stem is muted.
5. `stems_dir_for` resolves the right path given a known sidecar path.
6. `decode_stem_to_pcm` decodes a known test stem file to expected sample count (smoke test, may need a small fixture).

### Recipe tests

7. `VocalOverlay.build_script` emits the expected phase shape (Fader on outgoing, 4 StemFader Step at bar 0 to kill incoming's non-vocal stems, StemFader ramp on vocals, StemFader ramps on others at bars [12, 16]).
8. `BassSwapStemAware.build_script` reads outgoing's bass_density and emits StemFader Step at the correct bar.
9. `BreakdownIntoDrop.build_script` finds outgoing's valley + incoming's peak and emits a step at the right bar.

### Scoring tests

10. `score_recipe(VocalOverlay, inputs_with_stems_and_ratio_1)` ≥ 0.80.
11. `score_recipe(VocalOverlay, inputs_with_stems_and_ratio_1.1)` ≤ 0.05.
12. `score_recipe(VocalOverlay, inputs_without_stems)` ≤ 0.05.

### Integration (mixer_integration_test)

13. Smoke: arm `VocalOverlay` on a synthetic pair with stems loaded → produce some output samples, assert no panic / NaN.

## Risks

- **Eager stems loading slows cue.** Each cue decodes 4 mp3s instead of 1 → roughly 4× the cue time. With ffmpeg this is still ~200-500 ms per track. Acceptable for transition arming which has minutes of lead time.
- **Memory pressure.** Two decks each holding ~160 MB of decoded PCM = ~320 MB. On the user's dev box this is fine. Production deployment (e.g., the `deploy/` Rancher manifests) would need to check pod memory limits.
- **Stem files missing or corrupt.** Falls back to mixed-source cue (deck cues normally). Stem-aware recipes can't fire on those tracks (score = 0). User can see in logs: `library loaded count=18` will be unchanged but per-track logging could surface "stems unavailable for <track>".
- **`requires_stems` adds a method to `Recipe` trait.** Every existing `impl Recipe for X` needs a `fn requires_stems(&self) -> bool { false }` line — but with a default trait method, no edits needed to existing impls. The 3 new recipes override to `true`.
- **DeckSource::Stems doesn't honor tempo.** If a stem-aware recipe accidentally fires with non-unity ratio (despite the score gate), stems play at native rate while the rest of the engine thinks the deck is at the cued tempo. The picker's `|ratio - 1.0| < 0.01` gate is the only safety. Document loudly.
- **Per-stem fader values affect SUM, not stem-specific EQ.** If a recipe wants "kill bass band but keep bass STEM," it should use `EqLow` (existing), not `StemFaderBass`. Vice versa: "remove the bass stem entirely" uses `StemFaderBass=0`, not `EqLow=-40dB`. Document both knobs.

## Components and file layout

```
engine/src/
  deck.rs                ← +StemKind, +DeckSource::Stems, +cue_stems, +stem_faders state
                          ← read path branches on DeckSource variant
  automation.rs          ← +4 StemFader* targets, +apply_value arms
  recipe_script.rs       ← +Phase::StemFader variant, +compile arm
                          ← +pub use StemKind
  recipe.rs              ← Recipe trait gains requires_stems (default false)
                          ← +3 new recipe structs/impls (override to true)
                          ← RecipeChoice gains 3 variants
                          ← score_recipe gains 3 branches + gate logic
                          ← Task 0: EqualPowerCrossfade 0.5→0.15, EchoOut 0.35→0.20
                          ← PairScoreInputs gains stems_available bool
  config.rs              ← ForceRecipe gains 3 variants
  main.rs                ← recipe_choice_name + recipe_name + scheduler-arm extended
                          ← scheduler arm reads requires_stems → calls cue_stems
  scheduler.rs           ← TransitionScheduler::arm gains a path for stems cueing
  ffmpeg/decoder.rs      ← +decode_to_mono_pcm helper (or similar) for stem loading

engine/tests/
  deck_test.rs           ← +stems decode + per-stem fader unit tests
  script_test.rs         ← +3 phase-shape tests for new recipes
  recipe_test.rs         ← +3 scoring tests with stems_available toggle
  compile_test.rs        ← +1 StemFader compile arm test
```

No DSL changes beyond `Phase::StemFader`. No analyzer changes. No sidecar schema changes (stems block already in place from prior sub-project).

## Roadmap (Phase D.2+, future)

- **D.2 — tempo-aware stem playback**: resample stems at cue time per the BPM ratio, or apply atempo to each stem during decode. Unlocks stem-aware recipes on non-identical-BPM pairs.
- **D.3 — selector uses vocal_segments**: pick `outro_start` to land in a vocal-silent bar of the outgoing.
- **D.4 — per-stem EQ**: kill the bass *band* on the vocals stem (to remove low rumble in a-cappella overlays). Lower priority.
- **D.5 — adaptive recipe variants**: more recipes that read bar-by-bar density (e.g., `DoubleDropOnSyncedDrums` — fires only when both tracks have a drum peak on the same bar).
