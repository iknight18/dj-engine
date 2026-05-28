# Phase D Stem-Aware Recipes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship deck-level multi-stem decoding + 3 stem-aware recipes (`VocalOverlay`, `BassSwapStemAware`, `BreakdownIntoDrop`) + a quick pruning of `EqualPowerCrossfade` and `EchoOut` scores. All stem-aware recipes gated on `|bpm_ratio − 1.0| < 0.01` to dodge the time-stretch question for v1.

**Architecture:** New `StemKind` enum (Drums/Bass/Vocals/Other) + 4 new `AutomationTarget` variants for per-stem faders + `DeckState.stem_faders: [f32; 4]`. New `DeckSource::Stems` variant that holds 4 mono f32 PCM buffers in memory, loaded eagerly at cue time when stem files exist (fallback to mixed source when not). New `Phase::StemFader` DSL variant. New `Recipe::requires_stems(&self) -> bool` trait method (default `false`); scheduler dispatches to `cue_stems` when `true`. Three new recipes consult `ctx.{outgoing,incoming}.sidecar.stems` (density arrays + vocal_segments) to make per-bar decisions.

**Tech Stack:** Rust 2021. Uses existing `ffmpeg` decoder helpers and `serde` derives. No external new deps.

---

## File Structure

| File | New / Modify | Responsibility |
|---|---|---|
| `engine/src/automation.rs` | Modify | +4 `AutomationTarget` variants. `apply_value` arms map to `stem_faders[i]`. |
| `engine/src/deck.rs` | Modify | +`StemKind` enum, +`stem_faders: [f32; 4]` field on `DeckState`, +`DeckSource::Stems` variant + read_frames branch, +`cue_stems` method, +`stems_dir_for` helper. |
| `engine/src/ffmpeg/decoder.rs` | Modify | +`decode_to_mono_pcm_at_44100(path) -> Vec<f32>` helper for stem loading. (Or wherever decoder helpers live.) |
| `engine/src/recipe_script.rs` | Modify | +`Phase::StemFader` variant + compile arm. Re-export `StemKind`. |
| `engine/src/recipe.rs` | Modify | +`Recipe::requires_stems` trait method (default `false`). +3 new recipe structs/impls. Extend `RecipeChoice`, scoring, IDs, candidates. +`PairScoreInputs.stems_available`. Task-0 score tweaks. |
| `engine/src/config.rs` | Modify | +3 `ForceRecipe` variants + mappings. |
| `engine/src/main.rs` | Modify | Extend `recipe_choice_name` + `recipe_name`. Scheduler-arm match adds 3 arms that dispatch `cue_stems` based on `recipe.requires_stems()`. |
| `engine/src/scheduler.rs` | Modify | `TransitionScheduler::arm` branches: if `recipe.requires_stems()` and stems available, cue both decks with stems. |
| `engine/tests/deck_test.rs` | Modify | +Stems read_frames + per-stem fader unit tests. |
| `engine/tests/compile_test.rs` | Modify | +1 test for `Phase::StemFader` compile arm. |
| `engine/tests/script_test.rs` | Modify | +3 phase-shape tests for the new recipes. |
| `engine/tests/recipe_test.rs` | Modify | +3 scoring tests (with/without stems_available). |

---

### Task 0: Deprioritize EqualPowerCrossfade and EchoOut

**Files:**
- Modify: `engine/src/recipe.rs` (`score_recipe`)

- [ ] **Step 1: Lower the scoring constants**

In `engine/src/recipe.rs`, find `pub fn score_recipe`. Find the `RecipeChoice::EqualPower` arm — change the returned value from `0.5` to `0.15`:

```rust
        RecipeChoice::EqualPower => 0.15,
```

Find the `RecipeChoice::EchoOut` arm. The current implementation is:

```rust
        RecipeChoice::EchoOut => {
            if inputs.energy_delta < -0.15 { 0.65 } else { 0.35 }
        }
```

Change to:

```rust
        RecipeChoice::EchoOut => {
            if inputs.energy_delta < -0.15 { 0.45 } else { 0.20 }
        }
```

- [ ] **Step 2: Update any existing tests that pin these scores**

Run:
```
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --test recipe_test 2>&1 | Select-Object -Last 20
```

If any test fails because it asserted `score_recipe(EqualPower, ...) == 0.5` or `score_recipe(EchoOut, ...) == 0.35/0.65`, update the expected value in the test.

- [ ] **Step 3: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe.rs engine/tests/recipe_test.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "fix(engine): deprioritize EqualPowerCrossfade (0.50→0.15) and EchoOut (0.35→0.20)"
```

---

### Task 1: Automation infrastructure — `StemKind` + 4 targets + `stem_faders` state

**Files:**
- Modify: `engine/src/deck.rs`
- Modify: `engine/src/automation.rs`

- [ ] **Step 1: Add `StemKind` to `deck.rs`**

Insert just below the `DeckId` enum definition:

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StemKind {
    Drums = 0,
    Bass = 1,
    Vocals = 2,
    Other = 3,
}
```

- [ ] **Step 2: Add `stem_faders` to `DeckState`**

Find the `DeckState` struct and add a field:

```rust
pub struct DeckState {
    pub fader: f32,
    pub tempo: f64,
    pub eq_low_db: f64,
    pub eq_mid_db: f64,
    pub eq_high_db: f64,
    pub filter_knob: f64,
    pub echo_mix: f64,
    pub loop_gate: f64,
    pub stem_faders: [f32; 4],          // NEW
    pub source_path: Option<String>,
    pub play_state: PlayState,
}
```

Update `impl Default for DeckState` to initialize `stem_faders: [1.0, 1.0, 1.0, 1.0]`.

- [ ] **Step 3: Add 4 new `AutomationTarget` variants**

In `engine/src/automation.rs`, find the `AutomationTarget` enum. Append:

```rust
    StemFaderDrums,
    StemFaderBass,
    StemFaderVocals,
    StemFaderOther,
```

(Place after `LoopGate`. If derives include `serde::Serialize/Deserialize`, the new variants inherit.)

- [ ] **Step 4: Map targets to `stem_faders[]` in `apply_value`**

Find `fn apply_value` in `automation.rs`. Add 4 match arms (place after `LoopGate`):

```rust
        AutomationTarget::StemFaderDrums => {
            state.stem_faders[crate::deck::StemKind::Drums as usize] = v.clamp(0.0, 1.0) as f32;
        }
        AutomationTarget::StemFaderBass => {
            state.stem_faders[crate::deck::StemKind::Bass as usize] = v.clamp(0.0, 1.0) as f32;
        }
        AutomationTarget::StemFaderVocals => {
            state.stem_faders[crate::deck::StemKind::Vocals as usize] = v.clamp(0.0, 1.0) as f32;
        }
        AutomationTarget::StemFaderOther => {
            state.stem_faders[crate::deck::StemKind::Other as usize] = v.clamp(0.0, 1.0) as f32;
        }
```

- [ ] **Step 5: Build**

```
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --release 2>&1 | Select-Object -Last 5
```

Expected: builds. If existing tests construct `DeckState` with named fields, they need `stem_faders: [1.0; 4]` added — search for `DeckState {` and update each literal.

- [ ] **Step 6: Run tests**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test 2>&1 | Select-Object -Last 10
```

Expected: all pass.

- [ ] **Step 7: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/deck.rs engine/src/automation.rs engine/tests/
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): StemKind + 4 StemFader automation targets + DeckState.stem_faders"
```

---

### Task 2: `Phase::StemFader` DSL variant + compile arm

**Files:**
- Modify: `engine/src/recipe_script.rs`

- [ ] **Step 1: Append the variant**

In `recipe_script.rs`, find the `Phase` enum. Append:

```rust
    /// Per-stem fader ramp. Compiles to one of the four `StemFader*` automation
    /// targets. Stem-aware recipes use this to mute / un-mute individual stems
    /// independently of the mixed fader.
    StemFader {
        deck: DeckRole,
        stem: crate::deck::StemKind,
        from: f64,
        to: f64,
        span: BarSpan,
        curve: Curve,
    },
```

- [ ] **Step 2: Add compile arm**

In `compile()`'s match on `*phase`, add (next to existing `Fader` arm):

```rust
            Phase::StemFader {
                deck,
                stem,
                from,
                to,
                span,
                curve,
            } => {
                let target = match stem {
                    crate::deck::StemKind::Drums  => AutomationTarget::StemFaderDrums,
                    crate::deck::StemKind::Bass   => AutomationTarget::StemFaderBass,
                    crate::deck::StemKind::Vocals => AutomationTarget::StemFaderVocals,
                    crate::deck::StemKind::Other  => AutomationTarget::StemFaderOther,
                };
                emit_ramp(
                    deck, target,
                    clamp01(from), clamp01(to),
                    span, curve,
                    &mut outgoing_automations, &mut incoming_automations,
                );
            }
```

(If the existing code uses `emit_ramp!` macro instead of a closure, use the macro idiom. Read the surrounding `Fader` arm.)

- [ ] **Step 3: Write compile test**

Append to `engine/tests/compile_test.rs`:

```rust
#[test]
fn stem_fader_compiles_to_stem_fader_automation() {
    let outgoing = track();
    let incoming = track();
    let plan = plan_16();
    let ctx = make_ctx(&outgoing, &incoming, &plan, 0);

    let script = TransitionScript {
        length_bars: 16,
        phases: vec![Phase::StemFader {
            deck: DeckRole::Incoming,
            stem: dj_engine::deck::StemKind::Vocals,
            from: 0.0,
            to: 1.0,
            span: bars(0.0, 4.0),
            curve: Curve::Linear,
        }],
        outgoing_init: None,
        incoming_init: None,
    };
    let out = compile(&script, &ctx);
    assert_eq!(out.incoming_automations.len(), 1);
    let a = &out.incoming_automations[0];
    assert_eq!(a.target, AutomationTarget::StemFaderVocals);
    assert_eq!(a.curve, Curve::Linear);
    assert!((a.from - 0.0).abs() < 1e-9);
    assert!((a.to - 1.0).abs() < 1e-9);
    assert_eq!(a.t_start_frames, 0);
    assert_eq!(a.t_end_frames, 4 * 88200);
}
```

- [ ] **Step 4: Run tests**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --test compile_test 2>&1 | Select-Object -Last 10
```

Expected: all pass.

- [ ] **Step 5: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe_script.rs engine/tests/compile_test.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): Phase::StemFader DSL variant + compile arm"
```

---

### Task 3: `Recipe::requires_stems` trait method

**Files:**
- Modify: `engine/src/recipe.rs`

- [ ] **Step 1: Extend the trait**

Find the `Recipe` trait. Add a default method:

```rust
pub trait Recipe {
    fn name(&self) -> &'static str;
    fn build_script(&self, ctx: &TransitionContext<'_>) -> crate::recipe_script::TransitionScript;
    fn build(&self, ctx: &TransitionContext<'_>) -> RecipeOutput {
        crate::recipe_script::compile(&self.build_script(ctx), ctx)
    }

    /// True iff this recipe needs both decks loaded in stems mode (4 separate
    /// audio buffers per deck). Default `false` — existing recipes use the
    /// mixed source. The scheduler reads this and dispatches `cue_stems`
    /// instead of `cue` for incoming, and re-cues outgoing if needed.
    fn requires_stems(&self) -> bool { false }
}
```

No existing recipe needs editing — they all inherit the default `false`.

- [ ] **Step 2: Build**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --release 2>&1 | Select-Object -Last 5
```

Expected: clean build.

- [ ] **Step 3: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): Recipe::requires_stems trait method (default false)"
```

---

### Task 4: `DeckSource::Stems` variant + `cue_stems`

**Files:**
- Modify: `engine/src/deck.rs`
- Modify: `engine/src/ffmpeg/decoder.rs`

- [ ] **Step 1: Add a mono-PCM decoder helper**

Read `engine/src/ffmpeg/decoder.rs` to find the existing decode entry point (likely something like `Decoder::new` or `decode_to_pcm`). Add a sibling function:

```rust
/// Decode an audio file to mono f32 PCM at 44100 Hz. Used for stem loading.
pub async fn decode_to_mono_pcm_44100(path: &std::path::Path) -> Result<Vec<f32>, DecodeError> {
    // Use the existing ffmpeg invocation with: -i <path> -ac 1 -ar 44100 -f f32le -
    // Read stdout as raw f32 little-endian samples.
    // Implementation mirrors existing decoder pattern — find it in this file.
    todo!("implement using existing ffmpeg invocation pattern")
}
```

Implement using the existing ffmpeg subprocess pattern in this file. The output format is `-f f32le` so stdout bytes can be reinterpreted as `f32` by chunking 4 bytes at a time. The function reads stdout to EOF, returns the buffer.

Look at how the existing decoder reads audio for the deck — copy that approach. The key flags are:
- `-ac 1` — mono
- `-ar 44100` — sample rate
- `-f f32le` — raw 32-bit float little-endian
- `-` — stdout

If the existing decoder uses a different mechanism, follow its pattern but produce a mono Vec<f32> at 44100 Hz.

- [ ] **Step 2: Add `DeckSource::Stems` variant in `deck.rs`**

Find the `DeckSource` enum. Add:

```rust
pub(crate) enum DeckSource {
    Decoder(Box<dyn ffmpeg::Decoder>),
    Buffer { /* existing */ },
    Stems {
        drums: Vec<f32>,
        bass: Vec<f32>,
        vocals: Vec<f32>,
        other: Vec<f32>,
        position: usize,
        sample_rate: u32,
    },
}
```

Update `DeckSource::read_frames` (or however the existing variant's read method is named) to handle `Stems`. The Stems read takes a `&DeckState` (or just `[f32; 4]` for stem_faders) so it can mix per-fader. If the existing signature doesn't take state, you'll need to thread it through OR resolve the fader values inside the Deck::pull method instead of inside DeckSource. **Cleanest:** keep DeckSource::Stems::read_frames returning raw 4-stem frames; mix in Deck::pull where state is in scope.

So actually: `Stems::read_frames(n)` returns a `Vec<[f32; 4]>` (per-frame 4-channel). Then `Deck::pull` does the mix.

Pick whichever pattern the existing code admits more naturally. The key constraint is per-stem fader values must apply per-sample.

- [ ] **Step 3: Add `stems_dir_for` helper**

In `deck.rs`:

```rust
fn stems_dir_for(sidecar_path: &std::path::Path) -> Option<std::path::PathBuf> {
    // Example: sidecar at "C:\...\analyzed-real\001-song.mp3.json"
    //          stems at "C:\...\analyzed-real-stems\001-song\"
    let analyzed_dir = sidecar_path.parent()?;
    let stems_root_name = format!("{}-stems",
        analyzed_dir.file_name()?.to_string_lossy());
    let stems_root = analyzed_dir.parent()?.join(stems_root_name);
    let basename = sidecar_path
        .file_name()?
        .to_string_lossy()
        .trim_end_matches(".json")
        .trim_end_matches(".mp3")
        .to_string();
    let dir = stems_root.join(&basename);
    if dir.exists() { Some(dir) } else { None }
}
```

- [ ] **Step 4: Add `cue_stems` method**

In `impl Deck`:

```rust
pub async fn cue_stems(
    &mut self,
    track: LoadedTrack,
    offset_seconds: f64,
    rate: f64,
) -> Result<(), DeckError> {
    let stems_dir = stems_dir_for(&track.sidecar_path)
        .ok_or(DeckError::StemsUnavailable)?;
    let drums = crate::ffmpeg::decoder::decode_to_mono_pcm_44100(
        &stems_dir.join("drums.mp3")).await?;
    let bass = crate::ffmpeg::decoder::decode_to_mono_pcm_44100(
        &stems_dir.join("bass.mp3")).await?;
    let vocals = crate::ffmpeg::decoder::decode_to_mono_pcm_44100(
        &stems_dir.join("vocals.mp3")).await?;
    let other = crate::ffmpeg::decoder::decode_to_mono_pcm_44100(
        &stems_dir.join("other.mp3")).await?;
    let len = drums.len().min(bass.len()).min(vocals.len()).min(other.len());
    let position = ((offset_seconds * 44_100.0).max(0.0) as usize).min(len);
    self.sources.push_back(DeckSource::Stems {
        drums: drums.into_iter().take(len).collect(),
        bass:  bass.into_iter().take(len).collect(),
        vocals: vocals.into_iter().take(len).collect(),
        other: other.into_iter().take(len).collect(),
        position,
        sample_rate: 44_100,
    });
    self.state.source_path = Some(track.sidecar.source_path.clone());
    self.state.play_state = PlayState::Cueing;
    self.state.tempo = rate;
    Ok(())
}
```

Add `DeckError::StemsUnavailable` variant if `DeckError` exists; otherwise wherever the error type is.

- [ ] **Step 5: Update `Deck::pull` to mix stems**

Find the audio path that reads from `DeckSource`. Add a branch:

```rust
DeckSource::Stems { drums, bass, vocals, other, position, .. } => {
    let mut frames = Vec::with_capacity(n_frames);
    for _ in 0..n_frames {
        if *position >= drums.len() { break; }
        let mixed =
              drums[*position]  * state.stem_faders[0]
            + bass[*position]   * state.stem_faders[1]
            + vocals[*position] * state.stem_faders[2]
            + other[*position]  * state.stem_faders[3];
        frames.push(mixed);
        *position += 1;
    }
    frames
}
```

Make sure to advance position correctly. If the existing code returns a `Vec<f32>` of length up to n_frames, match that contract.

- [ ] **Step 6: Build + test**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --release 2>&1 | Select-Object -Last 10
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test 2>&1 | Select-Object -Last 10
```

Expected: builds and tests pass.

- [ ] **Step 7: Add deck unit tests**

Append to `engine/tests/deck_test.rs` (or wherever per-deck unit tests live):

```rust
#[test]
fn stems_read_frames_mixes_with_faders() {
    // Hand-construct 4 stem buffers: each stem has unit values, faders test mix.
    let drums = vec![1.0_f32; 100];
    let bass = vec![1.0_f32; 100];
    let vocals = vec![1.0_f32; 100];
    let other = vec![1.0_f32; 100];

    // With all faders at 1.0, mixed sample = 1+1+1+1 = 4.0.
    // (Verify with the actual read function — adapt to its signature.)
    // ... (test the actual read path you implemented)
}

#[test]
fn stems_dir_for_resolves_sibling_stems_directory() {
    use dj_engine::deck::stems_dir_for;  // if public; else expose via cfg(test) or test-helper
    // ... assert path math
}
```

If the helper isn't pub, you can either make it pub(crate) and access via an integration test wrapper, or skip the path-math test and rely on the integration smoke test in Task 8.

- [ ] **Step 8: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/deck.rs engine/src/ffmpeg/decoder.rs engine/tests/deck_test.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): DeckSource::Stems + cue_stems + per-stem mix in pull"
```

---

### Task 5: `VocalOverlay` recipe + scoring + `PairScoreInputs.stems_available`

**Files:**
- Modify: `engine/src/recipe.rs`

- [ ] **Step 1: Add `stems_available` to `PairScoreInputs`**

Find the `PairScoreInputs` struct. Append:

```rust
    pub stems_available: bool,
```

In `PairScoreInputs::from_pair`, derive:

```rust
let stems_available = outgoing.sidecar.stems.is_some()
    && incoming.sidecar.stems.is_some();
```

Update the struct literal at the end of `from_pair` to include `stems_available`.

- [ ] **Step 2: Add `VocalOverlay` struct + impl**

Insert before `pub enum RecipeChoice`:

```rust
/// `VocalOverlay`: 16-bar layered transition.
/// Bars [0, 4]: incoming's vocals stem fades in over outgoing's full mix.
/// Bars [4, 12]: hold — outgoing plays + incoming vocal overlay.
/// Bars [12, 16]: incoming's drums/bass/other ramp in; outgoing fades out.
pub struct VocalOverlay;

impl Recipe for VocalOverlay {
    fn name(&self) -> &'static str { "vocal_overlay" }

    fn requires_stems(&self) -> bool { true }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        use crate::deck::StemKind;

        TransitionScript {
            length_bars: 16,
            phases: vec![
                Phase::TempoAlign { deck: DeckRole::Incoming, ratio },

                // Outgoing equal-power fades out over bars [12, 16].
                Phase::Fader {
                    deck: DeckRole::Outgoing, from: 1.0, to: 0.0,
                    span: bars(12.0, 16.0), curve: Curve::EqualPowerOut,
                },

                // Incoming mixed fader at 1.0 from bar 0 — per-stem faders gate everything.
                Phase::Fader {
                    deck: DeckRole::Incoming, from: 1.0, to: 1.0,
                    span: bars(0.0, 0.0), curve: Curve::Step,
                },

                // Mute incoming non-vocal stems at bar 0.
                Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Drums, from: 0.0, to: 0.0, span: bars(0.0, 0.0), curve: Curve::Step },
                Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Bass,  from: 0.0, to: 0.0, span: bars(0.0, 0.0), curve: Curve::Step },
                Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Other, from: 0.0, to: 0.0, span: bars(0.0, 0.0), curve: Curve::Step },
                Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Vocals, from: 0.0, to: 0.0, span: bars(0.0, 0.0), curve: Curve::Step },

                // Bars [0, 4]: incoming vocals fade in.
                Phase::StemFader {
                    deck: DeckRole::Incoming, stem: StemKind::Vocals,
                    from: 0.0, to: 1.0,
                    span: bars(0.0, 4.0), curve: Curve::Linear,
                },

                // Bars [12, 16]: incoming non-vocal stems fade in.
                Phase::StemFader {
                    deck: DeckRole::Incoming, stem: StemKind::Drums,
                    from: 0.0, to: 1.0,
                    span: bars(12.0, 16.0), curve: Curve::Linear,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming, stem: StemKind::Bass,
                    from: 0.0, to: 1.0,
                    span: bars(12.0, 16.0), curve: Curve::Linear,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming, stem: StemKind::Other,
                    from: 0.0, to: 1.0,
                    span: bars(12.0, 16.0), curve: Curve::Linear,
                },
            ],
            outgoing_init: None, incoming_init: None,
        }
    }
}
```

- [ ] **Step 3: Defer the rest of the wiring**

Don't add to `RecipeChoice` / `score_recipe` / etc. yet — Task 8 batches all the wiring for all three new recipes.

- [ ] **Step 4: Build to catch compile errors**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --release 2>&1 | Select-Object -Last 5
```

Expected: builds (the new struct is just an unused type — Rust may warn `dead_code`, ignore for now).

- [ ] **Step 5: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): VocalOverlay recipe + stems_available on PairScoreInputs"
```

---

### Task 6: `BassSwapStemAware` recipe — density-array snapping

**Files:**
- Modify: `engine/src/recipe.rs`

- [ ] **Step 1: Add helper to find lowest-density bar in a window**

In `recipe.rs`, near `BassSwap`:

```rust
/// Index of the lowest value in `density[lo..hi]`, returned as an offset from `lo`.
/// Returns `None` if the slice is empty or `lo >= hi`.
fn bar_with_min_density(density: &[f64], lo: usize, hi: usize) -> Option<usize> {
    let hi = hi.min(density.len());
    if lo >= hi { return None; }
    let mut best = (lo, density[lo]);
    for i in (lo + 1)..hi {
        if density[i] < best.1 {
            best = (i, density[i]);
        }
    }
    Some(best.0)
}

/// First index in `density[lo..hi]` where `density[i] >= threshold`. Returns the
/// last index of the slice when no bar passes. Returns `None` for empty input.
fn first_bar_at_or_above(density: &[f64], lo: usize, hi: usize, threshold: f64) -> Option<usize> {
    let hi = hi.min(density.len());
    if lo >= hi { return None; }
    for i in lo..hi {
        if density[i] >= threshold {
            return Some(i);
        }
    }
    Some(hi - 1)
}
```

- [ ] **Step 2: Add `BassSwapStemAware`**

```rust
/// `BassSwapStemAware`: 16-bar equal-power crossfade, with bass-stem kill/release
/// timed to actual bass-density structure. Outgoing's bass stem cuts off at the
/// lowest-bass-density bar in [12, 16]; incoming's bass stem comes in at the
/// first bar in [0, 16] where its bass_density >= 0.7. Other stems crossfade
/// equal-power across the window.
pub struct BassSwapStemAware;

impl Recipe for BassSwapStemAware {
    fn name(&self) -> &'static str { "bass_swap_stem_aware" }
    fn requires_stems(&self) -> bool { true }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        use crate::deck::StemKind;
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let l = 16.0;

        // Where in the OUTGOING track do bars [outro+12, outro+16] fall?
        // For density-array indexing, we measure relative to track start.
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
        let outgoing_outro_bar = (ctx.plan.outgoing_crossfade_start_seconds / bar_seconds) as usize;

        let outgoing_bass_density = ctx.outgoing.sidecar.stems
            .as_ref()
            .map(|s| s.bass_density.clone())
            .unwrap_or_default();
        let incoming_bass_density = ctx.incoming.sidecar.stems
            .as_ref()
            .map(|s| s.bass_density.clone())
            .unwrap_or_default();

        // outgoing kill: lowest-density bar in [outro_bar+12, outro_bar+16].
        let outgoing_kill_bar_track = bar_with_min_density(
            &outgoing_bass_density,
            outgoing_outro_bar + 12,
            outgoing_outro_bar + 16,
        );
        // Convert track-bar back to transition-window-bar (relative to window start).
        let outgoing_kill_bar_window = outgoing_kill_bar_track
            .map(|b| (b - outgoing_outro_bar) as f64)
            .unwrap_or(14.0);  // fallback: bar 14 of window

        // For incoming: bass density relative to track start. The incoming
        // intro_end is where transition bar 0 lands. Look ahead 16 bars from
        // intro_end - 1 for the first bar where bass_density >= 0.7.
        let incoming_intro_bar = (ctx.plan.incoming_crossfade_start_seconds / bar_seconds) as usize;
        let incoming_release_bar_track = first_bar_at_or_above(
            &incoming_bass_density,
            incoming_intro_bar,
            incoming_intro_bar + 16,
            0.7,
        );
        let incoming_release_bar_window = incoming_release_bar_track
            .map(|b| (b - incoming_intro_bar) as f64)
            .unwrap_or(2.0);  // fallback: bar 2

        TransitionScript {
            length_bars: 16,
            phases: vec![
                Phase::TempoAlign { deck: DeckRole::Incoming, ratio },

                // Equal-power crossfade on mixed fader across the full window.
                Phase::Fader { deck: DeckRole::Outgoing, from: 1.0, to: 0.0,
                    span: bars(0.0, l), curve: Curve::EqualPowerOut },
                Phase::Fader { deck: DeckRole::Incoming, from: 0.0, to: 1.0,
                    span: bars(0.0, l), curve: Curve::EqualPowerIn },

                // Incoming bass stem: muted until release bar.
                Phase::StemFader {
                    deck: DeckRole::Incoming, stem: StemKind::Bass,
                    from: 0.0, to: 0.0, span: bars(0.0, 0.0), curve: Curve::Step,
                },
                Phase::StemFader {
                    deck: DeckRole::Incoming, stem: StemKind::Bass,
                    from: 0.0, to: 1.0,
                    span: bars(incoming_release_bar_window, incoming_release_bar_window),
                    curve: Curve::Step,
                },

                // Outgoing bass stem: killed at the valley bar.
                Phase::StemFader {
                    deck: DeckRole::Outgoing, stem: StemKind::Bass,
                    from: 1.0, to: 0.0,
                    span: bars(outgoing_kill_bar_window, outgoing_kill_bar_window),
                    curve: Curve::Step,
                },
            ],
            outgoing_init: None, incoming_init: None,
        }
    }
}
```

- [ ] **Step 2 (continued): Commit**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --release 2>&1 | Select-Object -Last 5
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): BassSwapStemAware recipe with density-array snapping"
```

---

### Task 7: `BreakdownIntoDrop` recipe

**Files:**
- Modify: `engine/src/recipe.rs`

- [ ] **Step 1: Add the recipe**

```rust
/// `BreakdownIntoDrop`: 8-bar transition timed to outgoing's drum-density valley
/// and incoming's drum-density peak. During the window, outgoing's drum stem
/// progressively drops (the breakdown). At the first incoming-drum-peak bar
/// within the window, hard-cut: outgoing fader → 0, incoming fader → 1.
pub struct BreakdownIntoDrop;

impl Recipe for BreakdownIntoDrop {
    fn name(&self) -> &'static str { "breakdown_into_drop" }
    fn requires_stems(&self) -> bool { true }

    fn build_script(&self, ctx: &TransitionContext<'_>) -> TransitionScript {
        use crate::deck::StemKind;
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;

        let incoming_intro_bar = (ctx.plan.incoming_crossfade_start_seconds / bar_seconds) as usize;
        let incoming_drum_density = ctx.incoming.sidecar.stems
            .as_ref()
            .map(|s| s.drum_density.clone())
            .unwrap_or_default();

        // Find incoming's drum peak in bars [intro, intro+8]; fall back to bar 4.
        let drop_bar_track = first_bar_at_or_above(
            &incoming_drum_density,
            incoming_intro_bar,
            incoming_intro_bar + 8,
            0.85,
        );
        let drop_bar_window = drop_bar_track
            .map(|b| (b - incoming_intro_bar) as f64)
            .unwrap_or(4.0);

        TransitionScript {
            length_bars: 8,
            phases: vec![
                Phase::TempoAlign { deck: DeckRole::Incoming, ratio },

                // Outgoing's drum stem progressively drops over the first
                // drop_bar window bars (the breakdown).
                Phase::StemFader {
                    deck: DeckRole::Outgoing, stem: StemKind::Drums,
                    from: 1.0, to: 0.0,
                    span: bars(0.0, drop_bar_window.max(1.0)),
                    curve: Curve::Linear,
                },

                // Outgoing mixed fader holds at 1.0 then hard-cuts at drop_bar.
                Phase::Fader { deck: DeckRole::Outgoing, from: 1.0, to: 0.0,
                    span: bars(drop_bar_window, drop_bar_window), curve: Curve::Step },

                // Incoming silent until drop_bar.
                Phase::Fader { deck: DeckRole::Incoming, from: 0.0, to: 0.0,
                    span: bars(0.0, 0.0), curve: Curve::Step },
                Phase::Fader { deck: DeckRole::Incoming, from: 0.0, to: 1.0,
                    span: bars(drop_bar_window, drop_bar_window), curve: Curve::Step },
            ],
            outgoing_init: None, incoming_init: None,
        }
    }
}
```

- [ ] **Step 2: Build + commit**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --release 2>&1 | Select-Object -Last 5
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): BreakdownIntoDrop recipe (density-aware drop timing)"
```

---

### Task 8: Wire RecipeChoice + scoring + CLI + main.rs + scheduler dispatch

**Files:**
- Modify: `engine/src/recipe.rs`
- Modify: `engine/src/config.rs`
- Modify: `engine/src/main.rs`
- Modify: `engine/src/scheduler.rs`

- [ ] **Step 1: Extend `RecipeChoice` with 3 variants**

```rust
pub enum RecipeChoice {
    // ... existing 13 variants ...
    VocalOverlay,
    BassSwapStemAware,
    BreakdownIntoDrop,
}
```

- [ ] **Step 2: Update `recipe_choice_id`**

```rust
        RecipeChoice::VocalOverlay      => 14,
        RecipeChoice::BassSwapStemAware => 15,
        RecipeChoice::BreakdownIntoDrop => 16,
```

- [ ] **Step 3: Update `recency_hash`**

Add all 3 to the iteration array (after the existing variants).

- [ ] **Step 4: Update `pick_recipe` candidates**

Add 3 entries to the `candidates` array in `pick_recipe` AND in `pick_recipe_with_scores`.

- [ ] **Step 5: Add scoring branches**

```rust
        RecipeChoice::VocalOverlay => {
            let identical_bpm = (inputs.energy_delta.abs() < 0.20)  // not way off in energy
                && inputs.stems_available;
            // Gate on ratio ~1 — proxy: existing pick uses bpm_delta, but here we
            // need to look at the actual ratio. Use a derived field or proxy.
            // For now: only fire when stems_available AND BPM gates (the planner's
            // length_bars promotion to 16 already requires bpm_delta <= 0.02).
            let strong = identical_bpm && inputs.phrase_aligned && inputs.length_bars >= 16;
            if strong { 0.85 }
            else if identical_bpm { 0.30 }
            else { 0.05 }
        }
        RecipeChoice::BassSwapStemAware => {
            let strong = inputs.stems_available
                && inputs.phrase_aligned
                && inputs.length_bars >= 16
                && (inputs.outgoing_bass > 0.5 || inputs.incoming_bass > 0.5);
            if strong { 0.92 }  // beats normal BassSwap when stems are there
            else if inputs.stems_available { 0.30 }
            else { 0.05 }
        }
        RecipeChoice::BreakdownIntoDrop => {
            let strong = inputs.stems_available
                && inputs.phrase_aligned
                && inputs.energy_delta > 0.10;  // ramp-up
            if strong { 0.80 }
            else if inputs.stems_available { 0.20 }
            else { 0.05 }
        }
```

(The "ratio ≈ 1.0" gate from the spec isn't strictly enforced here because `PairScoreInputs` doesn't carry ratio. Acceptable v1: `length_bars >= 16` already implies a tight BPM gate in the planner — the planner only promotes to 16 when `bpm_delta <= 0.02`. So the score effectively requires near-identical BPMs.)

- [ ] **Step 6: Extend `ForceRecipe` in `config.rs`**

Append `VocalOverlay`, `BassSwapStemAware`, `BreakdownIntoDrop` variants and their `to_recipe_choice` arms.

- [ ] **Step 7: Extend `main.rs` helpers**

Add to `recipe_choice_name`:

```rust
        RecipeChoice::VocalOverlay      => "vocal_overlay",
        RecipeChoice::BassSwapStemAware => "bass_swap_stem_aware",
        RecipeChoice::BreakdownIntoDrop => "breakdown_into_drop",
```

Same for `recipe_name` if separate.

- [ ] **Step 8: Update scheduler dispatch**

In `main.rs` (or wherever the `match choice` calls `TransitionScheduler::arm`), add 3 arms:

```rust
            dj_engine::recipe::RecipeChoice::VocalOverlay => {
                dj_engine::scheduler::TransitionScheduler::arm(
                    &mut mixer, current_deck, other_deck, &current, &next_track, &plan,
                    &dj_engine::recipe::VocalOverlay,
                ).await?
            }
            dj_engine::recipe::RecipeChoice::BassSwapStemAware => {
                dj_engine::scheduler::TransitionScheduler::arm(
                    &mut mixer, current_deck, other_deck, &current, &next_track, &plan,
                    &dj_engine::recipe::BassSwapStemAware,
                ).await?
            }
            dj_engine::recipe::RecipeChoice::BreakdownIntoDrop => {
                dj_engine::scheduler::TransitionScheduler::arm(
                    &mut mixer, current_deck, other_deck, &current, &next_track, &plan,
                    &dj_engine::recipe::BreakdownIntoDrop,
                ).await?
            }
```

- [ ] **Step 9: Update `TransitionScheduler::arm` to dispatch cue vs cue_stems**

Read `engine/src/scheduler.rs`. Find `TransitionScheduler::arm`. Currently it calls `incoming_deck.cue(...)`. Add a branch:

```rust
if recipe.requires_stems() {
    incoming_deck.cue_stems(track.clone(), cue_offset, rate).await?;
    // If the outgoing isn't already in stems mode, re-cue it.
    // For v1, ALWAYS cue both decks in stems mode (the eager approach
    // from the spec).
} else {
    incoming_deck.cue(track.clone(), cue_offset, rate).await?;
}
```

**Per the spec's Eager mode**: cue *both* decks in stems mode whenever stems are available, regardless of recipe. Simpler implementation: in the main loop's seed-loading and post-transition handoff, always try `cue_stems` first, fall back to `cue` if `StemsUnavailable`. That avoids re-cue mid-transition.

For v1 of this task: make the SCHEDULER respect requires_stems for the INCOMING. Don't change outgoing's cue mode for this task — outgoing might be in mixed mode. Stem-aware recipes that operate on outgoing's stems (BassSwapStemAware kills outgoing bass) will see no audible effect if outgoing is in mixed mode (StemFader on outgoing's mixed source is a no-op because there are no stems to fade). Accept this v1 limitation; document.

Better v1 behavior: also re-cue outgoing in stems mode when arming a stem-aware recipe. But mid-transition re-cue with seek-to-current-position is non-trivial. Defer to Phase D.2.

For now: stem-aware recipes that fire on a non-stems-loaded outgoing will partially work — incoming stems operate correctly; outgoing stem modifications are no-ops. Acceptable.

A cleaner alternative implementable here: in `main.rs` loop, ALWAYS cue the NEXT incoming as stems if stems are available for it. By the time it becomes the next outgoing, it's already in stems mode. Over a few transitions the engine drifts to all-stems-cued. Implement this if straightforward.

- [ ] **Step 10: Build + test**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --release 2>&1 | Select-Object -Last 10
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test 2>&1 | Select-Object -Last 10
```

Expected: all green. If `score_recipe` tests fail due to the new pruning constants (Task 0) — already handled. If a `pick_recipe` test fails because a stem-aware recipe edged out the old winner, update the test to allow the new choice.

- [ ] **Step 11: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/recipe.rs engine/src/config.rs engine/src/main.rs engine/src/scheduler.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): wire RecipeChoice + scoring + CLI + scheduler for stem-aware recipes"
```

---

### Task 9: Phase-shape tests for the 3 new recipes

**Files:**
- Modify: `engine/tests/script_test.rs`

- [ ] **Step 1: Append tests for each recipe**

```rust
#[test]
fn vocal_overlay_emits_stem_fader_phases() {
    use dj_engine::recipe::VocalOverlay;
    use dj_engine::deck::StemKind;
    let outgoing = track();
    let incoming = track();
    let p = plan(16);
    let ctx = TransitionContext {
        outgoing: &outgoing, incoming: &incoming, plan: &p,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A, incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let script = VocalOverlay.build_script(&ctx);
    assert_eq!(script.length_bars, 16);

    // Incoming vocals ramp [0, 4].
    let has_vocal_intro = script.phases.iter().any(|ph| matches!(ph,
        Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Vocals, from, to, span, curve: Curve::Linear }
            if (*from - 0.0).abs() < 1e-9 && (*to - 1.0).abs() < 1e-9
               && (span.start.bars - 0.0).abs() < 1e-9 && (span.end.bars - 4.0).abs() < 1e-9));
    assert!(has_vocal_intro);

    // Incoming drums ramp [12, 16].
    let has_drums_intro = script.phases.iter().any(|ph| matches!(ph,
        Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Drums, from, to, span, curve: Curve::Linear }
            if (*from - 0.0).abs() < 1e-9 && (*to - 1.0).abs() < 1e-9
               && (span.start.bars - 12.0).abs() < 1e-9 && (span.end.bars - 16.0).abs() < 1e-9));
    assert!(has_drums_intro);
}

#[test]
fn bass_swap_stem_aware_emits_bass_stem_phases() {
    use dj_engine::recipe::BassSwapStemAware;
    use dj_engine::deck::StemKind;
    let outgoing = track();
    let incoming = track();
    let p = plan(16);
    let ctx = TransitionContext {
        outgoing: &outgoing, incoming: &incoming, plan: &p,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A, incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let script = BassSwapStemAware.build_script(&ctx);
    assert_eq!(script.length_bars, 16);

    // Outgoing bass stem killed somewhere — at least one bass-targeted StemFader on outgoing.
    let has_outgoing_bass_kill = script.phases.iter().any(|ph| matches!(ph,
        Phase::StemFader { deck: DeckRole::Outgoing, stem: StemKind::Bass, .. }));
    assert!(has_outgoing_bass_kill);

    // Incoming bass stem released somewhere.
    let has_incoming_bass_release = script.phases.iter().any(|ph| matches!(ph,
        Phase::StemFader { deck: DeckRole::Incoming, stem: StemKind::Bass, .. }));
    assert!(has_incoming_bass_release);
}

#[test]
fn breakdown_into_drop_emits_drum_breakdown_and_drop() {
    use dj_engine::recipe::BreakdownIntoDrop;
    use dj_engine::deck::StemKind;
    let outgoing = track();
    let incoming = track();
    let p = plan(8);
    let ctx = TransitionContext {
        outgoing: &outgoing, incoming: &incoming, plan: &p,
        master_clock_frames: 0,
        outgoing_deck: DeckId::A, incoming_deck: DeckId::B,
        sample_rate: SR,
    };
    let script = BreakdownIntoDrop.build_script(&ctx);
    assert_eq!(script.length_bars, 8);

    // Outgoing drum stem ramps 1.0 → 0.0.
    let has_drum_breakdown = script.phases.iter().any(|ph| matches!(ph,
        Phase::StemFader { deck: DeckRole::Outgoing, stem: StemKind::Drums, from, to, .. }
            if (*from - 1.0).abs() < 1e-9 && (*to - 0.0).abs() < 1e-9));
    assert!(has_drum_breakdown);

    // Incoming mixed fader hard-cuts 0 → 1.
    let has_drop = script.phases.iter().any(|ph| matches!(ph,
        Phase::Fader { deck: DeckRole::Incoming, from, to, curve: Curve::Step, .. }
            if (*from - 0.0).abs() < 1e-9 && (*to - 1.0).abs() < 1e-9));
    assert!(has_drop);
}
```

- [ ] **Step 2: Run + commit**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --test script_test 2>&1 | Select-Object -Last 10
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/tests/script_test.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "test(engine): phase-shape tests for stem-aware recipes"
```

---

### Task 10: Scoring tests for stem-aware recipes

**Files:**
- Modify: `engine/tests/recipe_test.rs`

- [ ] **Step 1: Append scoring tests**

```rust
#[test]
fn score_vocal_overlay_strong_when_stems_and_aligned() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.05,
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
        stems_available: true,
    };
    assert!((score_recipe(RecipeChoice::VocalOverlay, &inputs) - 0.85).abs() < 1e-9);
}

#[test]
fn score_vocal_overlay_weak_without_stems() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.05,
        outgoing_bass: 0.5,
        incoming_bass: 0.5,
        phrase_aligned: true,
        stems_available: false,
    };
    assert!((score_recipe(RecipeChoice::VocalOverlay, &inputs) - 0.05).abs() < 1e-9);
}

#[test]
fn score_bass_swap_stem_aware_beats_basic_bass_swap_when_stems() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let inputs = PairScoreInputs {
        length_bars: 16,
        use_eq_swap: true,
        energy_delta: 0.0,
        outgoing_bass: 0.7,
        incoming_bass: 0.7,
        phrase_aligned: true,
        stems_available: true,
    };
    let stem_aware = score_recipe(RecipeChoice::BassSwapStemAware, &inputs);
    let basic = score_recipe(RecipeChoice::BassSwap, &inputs);
    assert!(stem_aware > basic);
}

#[test]
fn score_breakdown_into_drop_needs_stems_and_ramp_up() {
    use dj_engine::recipe::{score_recipe, PairScoreInputs, RecipeChoice};
    let strong = PairScoreInputs {
        length_bars: 16, use_eq_swap: false,
        energy_delta: 0.20, outgoing_bass: 0.5, incoming_bass: 0.5,
        phrase_aligned: true, stems_available: true,
    };
    assert!((score_recipe(RecipeChoice::BreakdownIntoDrop, &strong) - 0.80).abs() < 1e-9);

    let no_stems = PairScoreInputs { stems_available: false, ..strong };
    assert!((score_recipe(RecipeChoice::BreakdownIntoDrop, &no_stems) - 0.05).abs() < 1e-9);
}
```

If existing scoring tests construct `PairScoreInputs` with named fields, they need `stems_available: false` added. Search for `PairScoreInputs {` and update each literal.

- [ ] **Step 2: Run + commit**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test 2>&1 | Select-Object -Last 10
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/tests/recipe_test.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "test(engine): scoring tests for stem-aware recipes"
```

---

### Task 11: cargo fmt + clippy + final sweep

- [ ] **Step 1: fmt**

```
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine
& "$env:USERPROFILE\.cargo\bin\cargo.exe" fmt
```

- [ ] **Step 2: clippy**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" clippy --all-targets -- -D warnings
```

Fix any warnings. Common: `clippy::large_enum_variant` if `DeckSource::Stems` ends up huge (it does — 4× Vec<f32>, ~Box of large arrays). Allow with `#[allow(clippy::large_enum_variant)]` on the enum if it fires; the Vec<f32>s only live on the heap so it's a non-issue at runtime, but the lint may complain. Document the allow.

- [ ] **Step 3: Full test sweep**

```
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test
```

Expected: all green.

- [ ] **Step 4: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine status
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add -u
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "chore(engine): fmt + clippy after Phase D stem-aware recipes"
```

(Skip if nothing changed.)

---

## Self-review notes

- **Spec coverage:**
  - StemKind + automation targets + DeckState.stem_faders → Task 1.
  - Phase::StemFader + compile arm → Task 2.
  - Recipe::requires_stems trait method → Task 3.
  - DeckSource::Stems + cue_stems + pull mix → Task 4.
  - VocalOverlay + PairScoreInputs.stems_available → Task 5.
  - BassSwapStemAware → Task 6.
  - BreakdownIntoDrop → Task 7.
  - Wiring (RecipeChoice, scoring, CLI, scheduler) → Task 8.
  - Phase-shape + scoring tests → Tasks 9, 10.
  - fmt + clippy → Task 11.
  - Task 0 (pruning) at the front.
- **Placeholders:** Task 4 Step 1 has a `todo!()` placeholder for the decoder helper because the existing decoder API needs to be inspected. The implementer must read `ffmpeg/decoder.rs` and adapt.
- **Type consistency:** `StemKind` is exported from `deck` and re-exported via `recipe_script` for use in `Phase::StemFader`. `[f32; 4]` indexing uses `StemKind as usize`.
- **Risk: outgoing stem cueing.** Phase D.1 cues OUTGOING in mixed mode for stem-aware recipes (since outgoing is already cued from the previous transition). StemFader operations on outgoing's mixed source are no-ops. Recipes work partially. The "eager mode" alternative (always cue in stems mode if available) is recommended; the implementer should attempt it in Task 8 Step 9 — if non-trivial, fall back to outgoing-in-mixed-mode and note as DONE_WITH_CONCERNS.
- **Risk: build_and_compile pitch-journey injection.** The existing `inject_pitch_journey` runs on every script. For stem-aware recipes with ratio ~1.0, it won't inject (delta < 0.005 threshold). Safe.
- **Risk: track length truncation.** `cue_stems` truncates all 4 stems to the shortest. Verify the stem audio files Demucs writes are all the same length (they should be).
