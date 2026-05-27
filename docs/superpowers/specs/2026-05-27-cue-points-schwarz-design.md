# Cue Points — Schwarz-Derived Improvements

**Date:** 2026-05-27
**Status:** Approved for planning
**Builds on:** `2026-05-26-dj-engine-design.md`, `2026-05-27-analyzer-selector-enhancement-design.md`
**Reference:** Schwarz, Schindler, Spadavecchia. *A Heuristic Algorithm for DJ Cue Point Estimation.* SMC 2018.

## Summary

The analyzer's `detect_cues.py` today picks `cue_in` / `cue_out` from a coarse 1-second energy curve at 10% of the track's mean energy, and sets `intro_end` / `outro_start` as blind `+/- 16 bars` from those points. The engine relies on these fields to anchor every transition — incoming `intro_end` is aligned to outgoing `outro_start` at the start of a crossfade. When the four numbers don't reflect real structural events, the alignment is musically off even when everything else (BPM, key, phrase boundaries) is correct.

This spec replaces those heuristics with three Schwarz-derived rules:

1. **Loudness anchor for `cue_in` / `cue_out`** — first/last 2-second window whose peak RMS is within 9 dB of the track's maximum. Mastering-invariant; replaces the mean-based threshold.
2. **Structural `intro_end` / `outro_start`** — first/last Foote-novelty peak on a fused chroma + onset SSM, snapped to the nearest downbeat. Replaces the `+/- 16 bars` blind offset; falls back to it when no peak is confident enough.
3. **Long-track shortening** — for tracks longer than 6 minutes, force `cue_out` into the `[5:30, 7:00]` window at the strongest novelty peak (or the floor of 5:30 if none). Soft trigger; harmless for short tracks.

No schema additions, no engine changes. Bump `analyzer_version` to invalidate sidecars and trigger one full re-analysis.

## Goals

- Cue points reflect real structural events in the audio, not regular-grid offsets.
- `cue_in` / `cue_out` placement is invariant to track mastering loudness (loudness-of-max, not loudness-of-mean).
- `intro_end` / `outro_start` lands on the drop / breakdown of the music, not 16 bars after silence ends.
- No change to engine code, no change to the sidecar schema.
- The new computation is tunable through a small handful of module-top constants that listening tests can refine.

## Non-Goals

- Full structural segmentation (intro / verse / chorus / drop labels). Deferred — would be a follow-up spec if the per-section labels prove valuable.
- Replacing librosa with a deep-learning model for boundary detection. We stay within the existing dependency set.
- Engine-side changes. The engine reads the same four fields with the same semantics; only the values change.
- Listening-history persistence, vocal detection, region cues (multiple intros). Out of scope.
- Real-time / streaming novelty. This is offline analysis only.

## Architecture

```
analyzer/src/dj_analyze/
  detect_novelty.py     [NEW]  Foote novelty on fused chroma + onset SSM
  detect_cues.py        [MOD]  Loudness anchor + novelty-driven intro/outro + long-track shortening
  detect_downbeats.py   [MOD]  Exposes compute_chroma_ssm; detect_phrases reuses it
  detect_energy.py      [MOD]  Adds per-window RMS-in-dB curve alongside the existing normalized curve
  analyze.py            [MOD]  Builds SSM once, threads it + novelty result through to cues
```

### Boundary discipline

- `detect_novelty.py` knows about audio, beats, and SSMs. It returns peak timestamps and strengths. It knows nothing about cues.
- `detect_cues.py` knows about beats, downbeats, the energy curve, the RMS-dB curve, and the novelty result. It returns four floats. It knows nothing about audio data or SSMs.
- `analyze.py` is the only orchestrator; it owns the call graph and the data flow.

### Why no schema change

The four output fields (`cue_in_seconds`, `cue_out_seconds`, `intro_end_seconds`, `outro_start_seconds`) already exist in the v2 schema. We are improving how they are computed, not what they describe. The engine reads them unchanged.

## Component 1 — Loudness anchor for cue_in / cue_out

Schwarz's rule (SMC 2018 §3.6): shift the cue region until its minimum loudness exceeds the track's max loudness minus 9 dB, computed as the peak RMS in 2-second windows. We adapt to point cues:

**`cue_in_seconds`:** the first 2-second window whose peak RMS is within 9 dB of the track's loudest window. Snapped **up** to the nearest downbeat (first downbeat at or after that window's start time), so the cue never lands inside the silent intro.

**`cue_out_seconds`:** the last such window. Snapped **down** to the nearest downbeat (last downbeat at or before that window's end time), so the cue never lands inside the silent outro.

**Why this replaces the current 10%-of-mean rule, not composes with it:**

- Mean-based thresholds drift with how a track is mastered. A quiet ballad has a low mean, so the threshold is low, so the cue lands on barely-audible noise. A loud EDM track has the opposite problem.
- Schwarz's max-based threshold is mastering-invariant: −9 dB of the track's own max is a stable perceptual floor across the library.
- Composing the two rules can produce surprising values when they disagree. One source of truth is simpler to reason about.

**Edge cases:**

- All-silent track (no window above the threshold): `cue_in = 0`, `cue_out = duration_seconds`. Same as today.
- Track with steady loudness throughout: first/last qualifying window is very early / very late. Cues approximate `first_downbeat` / `last_downbeat`. Same as today for those tracks.
- Decode failure: the sidecar still gets its `error` field; cue computation does not run.

**Threshold knob:** `LOUDNESS_THRESHOLD_DB = -9.0`. Schwarz's value for professionally produced music. The user's library is heterogeneous mp3s; we ship at −9 and let listening tell us whether to adjust.

## Component 2 — Structural intro_end / outro_start via Foote novelty

New module `detect_novelty.py`:

```python
@dataclass(frozen=True, slots=True)
class NoveltyResult:
    peaks_seconds: list[float]     # sorted; one entry per peak
    peak_strengths: list[float]    # parallel to peaks_seconds
    confidence: float              # 0..1; how clearly the strongest peak rises above noise

def detect_novelty(
    audio: np.ndarray,
    sr: int,
    beats: list[float],
    chroma_ssm: np.ndarray | None = None,
) -> NoveltyResult: ...
```

### Algorithm

1. **Beat-sync chroma SSM** — reuse `chroma_ssm` if passed (built once in `analyze.py` from `compute_chroma_ssm`); otherwise compute fresh.
2. **Beat-sync onset-strength SSM** — `librosa.onset.onset_strength(y, sr)` → `librosa.util.sync` to beats → cosine-similarity affinity matrix.
3. **Fuse** — element-wise product of the two affinity matrices. Both are in `[0, 1]`; product emphasises events that change *both* harmonically and rhythmically (drops, breakdowns, key shifts that bring new percussion).
4. **Foote checkerboard kernel** — convolve the fused matrix's main diagonal with a Gaussian-tapered checkerboard kernel of size `NOVELTY_KERNEL_BEATS` (default 16 beats = 4 bars).
5. **Peak picking** — `scipy.signal.find_peaks` with `prominence = median(novelty) + NOVELTY_MIN_PROMINENCE_MAD * MAD(novelty)`, `distance = 8` beats (no two peaks within 2 bars).
6. **Confidence** — `(strongest_peak - median(novelty)) / MAD(novelty)` if at least one peak found, clipped to `[0, 1]` against `NOVELTY_CONFIDENCE_MAD`. Below threshold means "no clear structure."

### Consumed in `detect_cues.py`

```python
MIN_INTRO_SECONDS = 8.0     # earliest intro_end after cue_in
MAX_INTRO_SECONDS = 60.0    # latest intro_end after cue_in
MIN_OUTRO_SECONDS = 8.0     # earliest distance from cue_out backwards
MAX_OUTRO_SECONDS = 60.0    # latest distance from cue_out backwards
```

**`intro_end_seconds`:**

- Window of interest: `[cue_in + MIN_INTRO_SECONDS, cue_in + MAX_INTRO_SECONDS]`.
- Candidate: the first novelty peak in that window.
- Snap to the nearest downbeat.
- **Fallback** (no peak in window, or `NoveltyResult.confidence < threshold`): `cue_in + 16 bars`, clamped to the window.

**`outro_start_seconds`:**

- Window of interest: `[cue_out - MAX_OUTRO_SECONDS, cue_out - MIN_OUTRO_SECONDS]`.
- Candidate: the last novelty peak in that window.
- Snap to the nearest downbeat.
- **Fallback:** `cue_out - 16 bars`, clamped to the window.

### Why min/max guards

Raw novelty peaks occasionally fire on transients at the very first downbeat (false intro_end at ~2 s) or deep inside a long intro (false intro_end at 90 s). Clamping to `[8, 60]` s after `cue_in` keeps the result in the musically plausible zone even when the detector misfires. The same logic applies symmetrically to `outro_start`.

### Why downbeat snapping

The engine aligns the incoming track's `intro_end` to the outgoing track's `outro_start` on the bar grid. An off-grid `intro_end` would be silently re-snapped by the alignment logic, hiding the analyzer's true intent and making transition-quality regressions hard to diagnose. Snapping in the analyzer keeps the sidecar value honest.

### Why confidence threshold

When the strongest novelty peak is only `~1*MAD` above the median, the curve is essentially flat — usually because the track has no real structural changes (ambient, drone, a single chord progression repeated). In that case we fall back to the regular-grid `+/- 16 bars` rule rather than snapping to an arbitrary peak.

## Component 3 — Long-track shortening

Soft, post-step rule applied after components 1 and 2 have computed initial cues.

```python
LONG_TRACK_THRESHOLD_S = 360.0   # 6 min — Schwarz's value
LONG_TRACK_TARGET_MIN_S = 330.0  # 5:30
LONG_TRACK_TARGET_MAX_S = 420.0  # 7:00
```

**Rule:**

1. If `duration_seconds <= LONG_TRACK_THRESHOLD_S`: no change.
2. Otherwise, find novelty peaks whose timestamps fall in `[LONG_TRACK_TARGET_MIN_S, LONG_TRACK_TARGET_MAX_S]`. If any exist, set `cue_out = strongest peak in window`, snapped to nearest downbeat.
3. If no peak in window: `cue_out = LONG_TRACK_TARGET_MIN_S`, snapped to nearest downbeat.
4. Re-run the `outro_start` computation from Component 2 against the new (shorter) `cue_out`.

**Why strongest peak, not earliest:** Schwarz wants a structurally clean exit (a place the music turns — drop, breakdown, last-chorus end). The strongest novelty peak in the window is the most likely such moment.

**Why fall back to the floor (5:30):** when no structural event exists in the window, the music is likely uniform. Cutting earlier than later serves Schwarz's PoS variety goal and matches the user's intent for keeping the set moving.

**Current-library impact:** the user's library is mostly 2:20–4:00, with one outlier at 5:45 (track #04) which still falls below the 6-minute trigger. This code is dormant on today's library but harmless and future-proofs the analyzer.

**Interaction with Component 2:** long-track shortening overrides Component 2's `outro_start` because `cue_out` has moved. We recompute `outro_start` with the same Component 2 logic against the new `cue_out`.

## Schema impact

None. The four cue fields (`cue_in_seconds`, `cue_out_seconds`, `intro_end_seconds`, `outro_start_seconds`) already exist. No schema bump.

## Analyzer version bump

`__version__` in `dj_analyze/__init__.py`: current value → `0.3.0`.

This invalidates all existing sidecars (skip-if-fresh check compares `analyzer_version`). One full re-analysis required on first run after upgrade. Cost on the 24-track library: ~4–5 minutes on 2 workers (existing chroma SSM is reused; the only added work per track is the onset SSM + novelty + RMS-dB curve, ~1.5–2 s total).

## Module changes — detailed

### `detect_novelty.py` (NEW, ~120 LOC)

Public surface: `detect_novelty(...)` returning `NoveltyResult`.

Internal helpers: `_foote_kernel(size_beats)`, `_onset_ssm(audio, sr, beat_frames)`, `_fuse_ssms(chroma_ssm, onset_ssm)`, `_compute_novelty_curve(fused_ssm, kernel)`, `_pick_peaks(novelty_curve, beats)`.

### `detect_cues.py` (REWRITE, ~180 LOC)

New signature:

```python
def detect_cues(
    duration_seconds: float,
    beats: list[float],
    downbeats: list[float],
    energy_curve: list[float],          # kept for backwards-compat callers / tests
    bpm: float,
    novelty: NoveltyResult,
    rms_db_curve: list[float],          # 2-second-window peak RMS in dB
    max_rms_db: float,                  # max of rms_db_curve
) -> dict[str, float]: ...
```

Internal helpers: `_loudness_anchor_cue_in`, `_loudness_anchor_cue_out`, `_structural_intro_end`, `_structural_outro_start`, `_shorten_if_long_track`, `_snap_down_to_downbeat`, `_snap_up_to_downbeat`.

### `detect_downbeats.py` (MOD, ~20 LOC)

Extract the chroma SSM computation into a top-level function:

```python
def compute_chroma_ssm(audio: np.ndarray, sr: int, beats: list[float]) -> np.ndarray | None:
    """Beat-sync chroma SSM. Returns None if the track is too short to build one."""
```

`detect_phrases` accepts an optional `chroma_ssm` parameter; falls back to computing fresh if not supplied. Public behavior unchanged.

### `detect_energy.py` (MOD, ~25 LOC)

Add a sibling output to the existing per-second normalized energy curve: per-window peak RMS in dB, in 2-second windows, plus the max of that curve.

```python
def detect_energy(audio: np.ndarray, sr: int) -> EnergyResult:
    """
    EnergyResult adds:
      rms_db_curve: list[float]   # peak RMS per 2-second window, in dB
      max_rms_db:   float
    """
```

Existing normalized curve and mean are untouched (still used by selector and engine).

### `analyze.py` (MOD, ~30 LOC change)

Wire the new flow:

```python
ssm = compute_chroma_ssm(audio, sr, beats)
phrase = detect_phrases(audio, sr, beats, chroma_ssm=ssm)
novelty = detect_novelty(audio, sr, beats, chroma_ssm=ssm)
energy = detect_energy(audio, sr)
cues = detect_cues(
    duration_seconds, beats, downbeats,
    energy.curve, bpm,
    novelty, energy.rms_db_curve, energy.max_rms_db,
)
```

One SSM computation per track instead of two.

## Testing strategy

### New unit tests

| Test | Fixture | Expected |
|---|---|---|
| `test_detect_novelty.py::test_two_section_track` | 30 s A-minor sine + 30 s E-major sine | One peak at ~30 s, strength well above background |
| `test_detect_novelty.py::test_uniform_track` | 60 s single sine | No peaks above prominence threshold |
| `test_detect_novelty.py::test_drum_drop` | 16 s sine alone + 16 s sine with 4/4 click | One peak at ~16 s (onset-SSM contribution catches the drum entry) |
| `test_detect_novelty.py::test_confidence_low_on_uniform` | Same uniform fixture | `confidence < NOVELTY_CONFIDENCE_MAD` threshold |
| `test_detect_cues.py::test_loudness_anchor_silent_intro` | 10 s of −40 dB noise, then 60 s of −6 dB tone | `cue_in` lands at ~10 s, snapped to nearest downbeat |
| `test_detect_cues.py::test_loudness_anchor_silent_outro` | 60 s of −6 dB tone, then 10 s of −40 dB noise | `cue_out` lands at ~60 s, snapped to nearest downbeat |
| `test_detect_cues.py::test_intro_end_uses_novelty` | Two-section synth, novelty peak at 24 s | `intro_end` is within 1 downbeat of 24 s |
| `test_detect_cues.py::test_outro_start_uses_novelty` | Two-section synth, novelty peak near end | `outro_start` snaps to it |
| `test_detect_cues.py::test_intro_end_falls_back_no_peak` | Uniform track | `intro_end = cue_in + 16 bars`, clamped |
| `test_detect_cues.py::test_intro_end_falls_back_low_confidence` | Track with peaks just above noise floor | falls back when confidence below threshold |
| `test_detect_cues.py::test_min_intro_guard` | Strong novelty peak at 3 s after cue_in | `intro_end >= cue_in + MIN_INTRO_SECONDS` (peak ignored) |
| `test_detect_cues.py::test_max_intro_guard` | Strong novelty peak at 90 s after cue_in | `intro_end <= cue_in + MAX_INTRO_SECONDS` (peak ignored) |
| `test_detect_cues.py::test_long_track_shortened` | 480 s synth with peak at 380 s | `cue_out` snaps to 380 s |
| `test_detect_cues.py::test_long_track_floor` | 480 s synth with no peak in window | `cue_out = 330 s`, snapped |
| `test_detect_cues.py::test_short_track_unaffected_by_long_rule` | 200 s track | `cue_out` unchanged by Component 3 |
| `test_detect_cues.py::test_downbeat_snapping` | novelty peak between two downbeats | cue lands on the closer downbeat |
| `test_detect_cues.py::test_silent_track_safe_defaults` | All-silent audio | `cue_in = 0`, `cue_out = duration` |

### Updated tests

The existing cases in `test_detect_cues.py` that pin the 10%-of-mean behavior are replaced by the loudness-anchor cases above. Tests for the `+/- 16 bars` blind offset are replaced by the novelty-driven cases plus the fallback cases.

### Schema validation

Unchanged. The schema test still validates v2 sidecars produced by the new code without modification.

### End-to-end smoke (manual)

1. Bump version, `dj-analyze data/ analyzed-real/` (incremental detects the version mismatch and re-analyzes all 24 tracks).
2. Inspect 3–5 sidecars manually. Verify:
   - `intro_end_seconds` differs from `cue_in + 16 bars` on tracks with clear drops (look at tracks #16, #02, #03 which had long quiet intros).
   - `cue_in` differs from current sidecar values where appropriate (silent intros now use the loudness anchor).
3. Restart the engine against the new sidecars. Listen for 15 minutes. Listen specifically for:
   - Incoming tracks entering on the drop/main groove, not mid-build.
   - Crossfade transitions landing musically.
4. If degraded, tune `LOUDNESS_THRESHOLD_DB` (try −12 dB for noisier mastering), `NOVELTY_CONFIDENCE_MAD` (lower to use more peaks, higher to be more conservative), or fall back via the analyzer version bump.

### Not tested

- Subjective DJ-feel quality. Manual listening is the gate.
- Threshold values themselves (the 9 dB, the 16-beat kernel, the 8 s / 60 s guards). These are named constants — easy one-line tuning post-listening.
- Long-track behavior on the current library (no track triggers); covered by the synthetic fixture only.

## Open knobs

All defined as module-top constants in `detect_cues.py` and `detect_novelty.py`:

| Constant | Default | Where | Purpose |
|---|---|---|---|
| `LOUDNESS_THRESHOLD_DB` | -9.0 | cues | dB below track max for the loudness floor |
| `LOUDNESS_WINDOW_S` | 2.0 | cues | peak-RMS window for the loudness curve |
| `MIN_INTRO_SECONDS` | 8.0 | cues | earliest `intro_end` after `cue_in` |
| `MAX_INTRO_SECONDS` | 60.0 | cues | latest `intro_end` after `cue_in` |
| `MIN_OUTRO_SECONDS` | 8.0 | cues | earliest distance from `cue_out` backwards |
| `MAX_OUTRO_SECONDS` | 60.0 | cues | latest distance from `cue_out` backwards |
| `NOVELTY_KERNEL_BEATS` | 16 | novelty | checkerboard kernel size (4 bars) |
| `NOVELTY_MIN_PROMINENCE_MAD` | 1.0 | novelty | peak prominence threshold above median, in MADs |
| `NOVELTY_MIN_PEAK_DISTANCE_BEATS` | 8 | novelty | minimum spacing between peaks (2 bars) |
| `NOVELTY_CONFIDENCE_MAD` | 2.0 | cues | strongest peak must exceed `median + N*MAD` to be accepted (else fallback) |
| `LONG_TRACK_THRESHOLD_S` | 360.0 | cues | Component 3 trigger duration |
| `LONG_TRACK_TARGET_MIN_S` | 330.0 | cues | Component 3 target window floor |
| `LONG_TRACK_TARGET_MAX_S` | 420.0 | cues | Component 3 target window ceiling |

## Open questions / future work

- **Loudness threshold for non-professional mastering.** Schwarz uses −9 dB for radio-/streaming-mastered music. If the user's library includes home recordings or low-bitrate rips that aren't loudness-normalized, −9 may be too tight; relax to −12 by ear.
- **Structural segmentation labels.** This spec finds boundaries but does not label segments (intro/verse/chorus/drop). Labels would enable smarter selector behavior (e.g., always start a set on a chorus). Separate spec if/when it's useful.
- **Per-section dynamics map.** The novelty curve itself, persisted to the sidecar, could let the engine make finer transition decisions (e.g., transition only on novelty peaks, not on arbitrary phrase boundaries). Not in this spec; adds a schema field. Revisit after listening.
- **Replace mean-energy with loudness-of-max throughout.** The selector still uses `energy_mean` from the (mean-based) energy curve. Same critique applies. Out of scope here.
- **Cue regions vs. cue points.** Schwarz's algorithm produces 10-second *regions* with fade curves; the engine consumes single *points*. If we ever want region-based fades that the analyzer authors rather than the engine, this is a natural next step.
