# Pitch Journey Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After a crossfade, gradually ramp the incoming track's atempo ratio from `outgoing_bpm/incoming_bpm` toward 1.0 over 16 bars so the listener never hears the snap to native BPM.

**Architecture:** New helper `decode_stretched_ramp` in `engine/src/ffmpeg/mixwindow.rs` that splits the ramp into K equal-duration chunks, decoding each with its own atempo ratio (midpoint linear interpolation), advancing source-time by `T*r_i` between chunks. `execute_crossfade` in `engine/src/mixer.rs` calls it after the existing post-roll push, then spawns the new long-lived decoder at `ramp_end` instead of `stretched_end`.

**Tech Stack:** Rust 1.95, tokio, ffmpeg (atempo). No new dependencies.

---

## File Structure

- **Modify** `engine/src/ffmpeg/mixwindow.rs` — add `decode_stretched_ramp` and one private `next_seek_for_chunk` helper used in its loop.
- **Modify** `engine/src/mixer.rs` — extend `execute_crossfade` to call `decode_stretched_ramp` and shift the new-decoder spawn target.
- **Create** `engine/tests/mixwindow_ramp_test.rs` — integration tests against the sine-wave fixtures.

No file split needed — both source files are well-focused already.

---

### Task 1: Add `decode_stretched_ramp` skeleton + identity-case test

**Files:**
- Modify: `engine/src/ffmpeg/mixwindow.rs`
- Create: `engine/tests/mixwindow_ramp_test.rs`

- [ ] **Step 1: Write the failing test (identity case)**

Create `engine/tests/mixwindow_ramp_test.rs`:

```rust
//! Tests for decode_stretched_ramp — chunked atempo with a per-chunk ratio ramp.

use std::path::Path;

use dj_engine::ffmpeg::mixwindow::decode_stretched_ramp;
use dj_engine::ffmpeg::SAMPLE_RATE;

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("audio")
}

#[tokio::test]
async fn identity_ratio_produces_expected_sample_count() {
    let a = fixtures().join("a_sine_440.flac");
    if !a.exists() {
        eprintln!("skipping: fixtures missing — run generate_test_audio.sh");
        return;
    }

    // 1.0 -> 1.0 over 2 seconds, 4 chunks. Output should be ~2s of stereo @ 44.1kHz.
    let pcm = decode_stretched_ramp(&a, 0.0, 1.0, 1.0, 2.0, 4)
        .await
        .expect("decode");

    let expected = (2.0 * SAMPLE_RATE as f64) as usize * 2; // stereo
    // Tolerance: ffmpeg may emit ±1 frame at chunk seams.
    let diff = (pcm.len() as i64 - expected as i64).abs();
    assert!(
        diff < (16 * 2) as i64,
        "got {} samples, expected ~{} (diff {})",
        pcm.len(),
        expected,
        diff
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path engine/Cargo.toml --test mixwindow_ramp_test identity_ratio_produces_expected_sample_count`
Expected: FAIL — `decode_stretched_ramp` does not exist.

- [ ] **Step 3: Add the skeleton in `engine/src/ffmpeg/mixwindow.rs`**

Append to `engine/src/ffmpeg/mixwindow.rs` (after the existing `atempo_chain` function):

```rust
/// Decode a ramp region of `source_path`, starting at `start_seconds` in track-time,
/// producing exactly `ramp_seconds` of stereo PCM output. The atempo ratio is
/// linearly interpolated between `start_ratio` and `end_ratio` across `chunks`
/// equal-output-duration chunks (midpoint interpolation: chunk `i` uses
/// `lerp(start, end, (i + 0.5) / chunks)`).
///
/// Each chunk is a separate `decode_stretched` call with a fixed ratio. The
/// source-time start of chunk `i+1` is the source-time end of chunk `i`, which
/// equals `start_i + T * r_i` (atempo at ratio r consumes T*r seconds of input
/// to produce T seconds of output).
pub async fn decode_stretched_ramp(
    source_path: &std::path::Path,
    start_seconds: f64,
    start_ratio: f64,
    end_ratio: f64,
    ramp_seconds: f64,
    chunks: usize,
) -> Result<Vec<f32>> {
    if chunks == 0 || ramp_seconds <= 0.0 {
        return Ok(Vec::new());
    }
    let chunk_out_seconds = ramp_seconds / chunks as f64;
    let mut out = Vec::new();
    let mut src_t = start_seconds;
    for i in 0..chunks {
        let frac = (i as f64 + 0.5) / chunks as f64;
        let ratio = start_ratio + (end_ratio - start_ratio) * frac;
        let chunk_in_seconds = chunk_out_seconds * ratio;
        let chunk = decode_stretched(source_path, src_t, src_t + chunk_in_seconds, ratio).await?;
        out.extend_from_slice(&chunk);
        src_t += chunk_in_seconds;
    }
    Ok(out)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path engine/Cargo.toml --test mixwindow_ramp_test identity_ratio_produces_expected_sample_count`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add engine/src/ffmpeg/mixwindow.rs engine/tests/mixwindow_ramp_test.rs
git commit -m "feat(engine): add decode_stretched_ramp helper"
```

---

### Task 2: Test cumulative input-time math

**Files:**
- Modify: `engine/tests/mixwindow_ramp_test.rs`

- [ ] **Step 1: Add ramp-ratio test**

Add to `engine/tests/mixwindow_ramp_test.rs`:

```rust
#[tokio::test]
async fn ratio_ramp_consumes_expected_input_seconds() {
    let a = fixtures().join("a_sine_440.flac");
    if !a.exists() {
        eprintln!("skipping: fixtures missing");
        return;
    }

    // 1.05 -> 1.0 over 2 seconds, 4 chunks.
    // chunk_out = 0.5s. ratios = midpoints of {0..0.25, 0.25..0.5, 0.5..0.75, 0.75..1.0}
    // i.e., {1.04375, 1.03125, 1.01875, 1.00625}. sum = 4.10. Input consumed = 0.5 * 4.10 = 2.05.
    //
    // We can't directly probe internal seek positions, but we can verify the output
    // sample count is correct (atempo preserves output duration).
    let pcm = decode_stretched_ramp(&a, 0.0, 1.05, 1.0, 2.0, 4)
        .await
        .expect("decode");

    let expected_out = (2.0 * SAMPLE_RATE as f64) as usize * 2;
    let diff = (pcm.len() as i64 - expected_out as i64).abs();
    assert!(
        diff < (16 * 2 * 4) as i64, // up to 16 frames per chunk seam
        "got {} samples, expected ~{} (diff {})",
        pcm.len(),
        expected_out,
        diff
    );
}
```

- [ ] **Step 2: Run test**

Run: `cargo test --manifest-path engine/Cargo.toml --test mixwindow_ramp_test ratio_ramp_consumes_expected_input_seconds`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add engine/tests/mixwindow_ramp_test.rs
git commit -m "test(engine): verify ramp output duration with non-identity ratio"
```

---

### Task 3: Test zero-chunks/zero-seconds edge cases

**Files:**
- Modify: `engine/tests/mixwindow_ramp_test.rs`

- [ ] **Step 1: Add edge-case test**

Add to `engine/tests/mixwindow_ramp_test.rs`:

```rust
#[tokio::test]
async fn zero_chunks_returns_empty() {
    let a = fixtures().join("a_sine_440.flac");
    let pcm = decode_stretched_ramp(&a, 0.0, 1.05, 1.0, 2.0, 0)
        .await
        .expect("decode");
    assert_eq!(pcm.len(), 0);
}

#[tokio::test]
async fn zero_ramp_seconds_returns_empty() {
    let a = fixtures().join("a_sine_440.flac");
    let pcm = decode_stretched_ramp(&a, 0.0, 1.05, 1.0, 0.0, 4)
        .await
        .expect("decode");
    assert_eq!(pcm.len(), 0);
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --manifest-path engine/Cargo.toml --test mixwindow_ramp_test`
Expected: PASS (all 4 tests).

- [ ] **Step 3: Commit**

```bash
git add engine/tests/mixwindow_ramp_test.rs
git commit -m "test(engine): cover zero-chunk and zero-duration ramp edge cases"
```

---

### Task 4: Wire ramp into `execute_crossfade`

**Files:**
- Modify: `engine/src/mixer.rs`

- [ ] **Step 1: Add constants and call site**

In `engine/src/mixer.rs`, at the top of the file (after the existing `const READ_CHUNK_FRAMES`):

```rust
/// Bars of post-crossfade pitch-journey ramp: incoming track's atempo ratio
/// linearly returns to 1.0 over this many bars so the swap to the native-tempo
/// decoder is not audible. See docs/superpowers/specs/2026-05-27-pitch-journey-design.md.
const RAMP_BARS: usize = 16;
/// Output duration per ramp chunk in bars. RAMP_BARS / RAMP_CHUNK_BARS = chunk count.
const RAMP_CHUNK_BARS: usize = 4;
/// Below this |ratio - 1.0|, we skip the ramp entirely — same threshold as
/// mixwindow's atempo bypass.
const RAMP_IDENTITY_EPSILON: f64 = 0.005;
```

- [ ] **Step 2: Import the helper**

Change the `use crate::ffmpeg::mixwindow::decode_stretched;` line in `engine/src/mixer.rs` to:

```rust
use crate::ffmpeg::mixwindow::{decode_stretched, decode_stretched_ramp};
```

- [ ] **Step 3: Replace the post-roll-then-spawn block in `execute_crossfade`**

In `engine/src/mixer.rs`, find this existing block (lines ~143-166 in the current file):

```rust
    // 4) Push the post-crossfade tail from the stretched buffer (1 bar, the headroom we
    // pre-decoded). This bridges the seam between the crossfade audio and the new playing
    // decoder's first samples without silence — and at ratio=1.0 (same BPM, atempo
    // bypassed) the splice is sample-continuous because both halves used accurate seek.
    let post_crossfade_start = (offset_samples + crossfade_samples).min(stretched.len());
    if post_crossfade_start < stretched.len() {
        push_all(producer, &stretched[post_crossfade_start..]).await;
    }

    info!(
        outgoing = %outgoing.sidecar.source_path,
        incoming = %incoming.sidecar.source_path,
        length_bars = plan.length_bars,
        eq = plan.use_eq_swap,
        "crossfade complete"
    );

    // 5) Collect the pre-spawned new decoder — by now it's been starting up in parallel
    // for the duration of the stretched decode + streaming + crossfade, so its first
    // samples are buffered in the OS pipe and ready to read.
    let new_decoder = new_decoder_task
        .await
        .map_err(|e| crate::error::EngineError::Ffmpeg(format!("decoder task join: {e}")))??;
    Ok(new_decoder)
}
```

The pre-spawn block above it (lines ~72-78) also needs to move — see Step 4. First, replace the block above with this version (note: the new-decoder spawn is removed from here; it now happens after the ramp):

```rust
    // 4) Push the post-crossfade tail from the stretched buffer (1 bar, the headroom we
    // pre-decoded). This bridges the seam between the crossfade audio and the ramp.
    let post_crossfade_start = (offset_samples + crossfade_samples).min(stretched.len());
    if post_crossfade_start < stretched.len() {
        push_all(producer, &stretched[post_crossfade_start..]).await;
    }

    // 5) Pitch journey: ramp the incoming track's atempo from `ratio` back to 1.0 over
    // RAMP_BARS so the handoff to the native-tempo decoder is not audible. Skip when
    // the ratio is already ~1.0 — there's nothing to relax.
    let ramp_skipped = (ratio - 1.0).abs() < RAMP_IDENTITY_EPSILON;
    let ramp_seconds = if ramp_skipped {
        0.0
    } else {
        RAMP_BARS as f64 * bar_seconds
    };
    let ramp_chunks = if ramp_skipped {
        0
    } else {
        (RAMP_BARS / RAMP_CHUNK_BARS).max(1)
    };
    let ramp_end = stretched_end + ramp_seconds;

    if !ramp_skipped {
        let cue_out_inc = incoming.sidecar.cue_out_seconds.unwrap_or(f64::INFINITY);
        // Clamp ramp_seconds if cue_out falls inside the ramp region — fall back to
        // no-ramp (length=0) rather than emitting a partial ramp that misaligns with
        // the new decoder's spawn point.
        let max_ramp = (cue_out_inc - stretched_end).max(0.0);
        let effective_seconds = ramp_seconds.min(max_ramp);
        if effective_seconds >= bar_seconds {
            let ramp_pcm = decode_stretched_ramp(
                &incoming_path,
                stretched_end,
                ratio,
                1.0,
                effective_seconds,
                ramp_chunks,
            )
            .await?;
            push_all(producer, &ramp_pcm).await;
        }
    }

    info!(
        outgoing = %outgoing.sidecar.source_path,
        incoming = %incoming.sidecar.source_path,
        length_bars = plan.length_bars,
        eq = plan.use_eq_swap,
        ramp_bars = if ramp_skipped { 0 } else { RAMP_BARS },
        "crossfade complete"
    );

    // 6) Collect the pre-spawned new decoder — it was spawned to start at ramp_end so
    // its first sample lines up with the end of the ramp (or with stretched_end if the
    // ramp was skipped).
    let new_decoder = new_decoder_task
        .await
        .map_err(|e| crate::error::EngineError::Ffmpeg(format!("decoder task join: {e}")))??;
    Ok(new_decoder)
}
```

- [ ] **Step 4: Shift the pre-spawn target to `ramp_end`**

In `engine/src/mixer.rs`, find the pre-spawn block (currently around lines 67-78). It currently spawns the new decoder at `stretched_end`:

```rust
    let new_start = stretched_end;
    let cue_out = incoming.sidecar.cue_out_seconds.unwrap_or(f64::INFINITY);
    let new_decoder_task = {
        let path = incoming_path.clone();
        tokio::spawn(async move { PlayingDecoder::spawn(&path, new_start, cue_out).await })
    };
```

Move this block to AFTER the `stretched_end` computation but compute `new_start` using the same ramp constants. Replace with:

```rust
    // Pre-spawn the new long-lived decoder to start where the ramp ends (or at
    // stretched_end if the ramp will be skipped). Spawning it now means it's ready
    // by the time we finish pushing the crossfade + post-roll + ramp.
    let new_start = {
        let skipped = (ratio - 1.0).abs() < RAMP_IDENTITY_EPSILON;
        if skipped {
            stretched_end
        } else {
            let cue_out_inc = incoming.sidecar.cue_out_seconds.unwrap_or(f64::INFINITY);
            let max_ramp = (cue_out_inc - stretched_end).max(0.0);
            let effective_seconds = (RAMP_BARS as f64 * bar_seconds).min(max_ramp);
            if effective_seconds >= bar_seconds {
                stretched_end + effective_seconds
            } else {
                stretched_end
            }
        }
    };
    let cue_out = incoming.sidecar.cue_out_seconds.unwrap_or(f64::INFINITY);
    let new_decoder_task = {
        let path = incoming_path.clone();
        tokio::spawn(async move { PlayingDecoder::spawn(&path, new_start, cue_out).await })
    };
```

This preserves the existing pre-spawn parallelism while making the spawn target match what the ramp section actually consumes.

- [ ] **Step 5: Build to verify compile**

Run: `cargo build --manifest-path engine/Cargo.toml`
Expected: clean build, no warnings about unused `ramp_end`.

If `ramp_end` is unused, delete that binding from Step 3's code.

- [ ] **Step 6: Commit**

```bash
git add engine/src/mixer.rs
git commit -m "feat(engine): pitch journey — ramp incoming back to native BPM over 16 bars post-crossfade"
```

---

### Task 5: Run full engine test suite

**Files:** none

- [ ] **Step 1: Run tests**

Run: `cargo test --manifest-path engine/Cargo.toml`
Expected: all existing tests pass (transitions, crossfade, EQ, phrase, selector, etc.) plus the 4 new `mixwindow_ramp_test` tests.

- [ ] **Step 2: If anything fails, fix and re-run**

Most likely failure modes:
- `mixer_integration_test.rs` — should be unaffected (uses pure-mix functions, not `execute_crossfade`).
- Unused `import` warnings — clean up if Step 4 left any.

- [ ] **Step 3: Commit any cleanup**

```bash
git add -A
git commit -m "chore(engine): clean up after pitch-journey integration"
```

(Skip if nothing changed.)

---

### Task 6: Listen test

**Files:** none

- [ ] **Step 1: Stop the running engine**

```bash
taskkill /F /PID 47032
```

(PID may have changed — check `tasklist | findstr dj_engine` and use the actual PID.)

- [ ] **Step 2: Restart engine**

Start the engine the same way it was running before. Watch the logs for `crossfade complete` lines — they should now include `ramp_bars=16` on BPM-mismatched transitions and `ramp_bars=0` on already-matched ones.

- [ ] **Step 3: Listen on a transition with a known BPM mismatch (≥5%)**

Confirm by ear:
- No snap at the post-roll → ramp boundary.
- No clicks at chunk boundaries (4 boundaries inside the 16-bar ramp).
- The incoming track audibly relaxes toward its native tempo over the ramp — by the end of the ramp it should sound like its native groove.

- [ ] **Step 4: Report to the user**

Hand off for listening feedback. If they hear clicks at chunk boundaries → reduce chunks to 2 (8-bar chunks). If they hear the ramp drifting too obviously → shorten to 8 bars. If it feels too sudden → lengthen to 32. All changes are constant-only edits in `mixer.rs`.

---

## Self-review notes

- Spec coverage: every requirement in the design doc maps to a task — `decode_stretched_ramp` (Tasks 1-3), mixer integration (Task 4), edge cases (Step 3 of Task 4, plus Task 3 for zero-input).
- No placeholders.
- Type consistency: `decode_stretched_ramp` signature matches across the spec, the tests, and the call site in mixer.rs.
- Ratio interpretation matches `decode_stretched`'s existing convention (`ratio = current_bpm / candidate_bpm`, >1 speeds up the source).
