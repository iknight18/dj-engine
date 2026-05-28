# Analyzer BPM Octave-Error Correction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Catch octave errors in the librosa-derived global BPM by adding a second, independent estimate from `librosa.beat.beat_track` and an `resolve_octave` resolver that prefers candidates inside the perceptual range [70, 180] BPM.

**Architecture:** All changes inside `analyzer/src/dj_analyze/detect_bpm.py` and a matching extension to `analyzer/tests/test_detect_bpm.py`. Two new helpers (`bpm_from_beats`, `_snap_to_perceptual`) and one new resolver (`resolve_octave`) module-level functions. `detect_bpm` body extended to call `beat_track` and route the two estimates through the resolver. Public signature `detect_bpm(audio, sr) -> (bpm: float, confidence: float)` unchanged. No schema changes.

**Tech Stack:** Python 3.11+, librosa, numpy, pytest.

---

## File Structure

| File | New / Modify | Responsibility |
|---|---|---|
| `analyzer/src/dj_analyze/detect_bpm.py` | Modify | Add `PERCEPTUAL_LOW`, `PERCEPTUAL_HIGH`, `PERCEPTUAL_DEFAULT`, `AGREEMENT_TOLERANCE` constants. Add `bpm_from_beats`, `_snap_to_perceptual`, `resolve_octave` helpers. Extend `detect_bpm` to invoke them. |
| `analyzer/tests/test_detect_bpm.py` | Modify | Add unit tests for `bpm_from_beats`, `resolve_octave`, `_snap_to_perceptual`. Add an optional audio-level test using a synthesized 76 BPM click track if the fixture infrastructure supports adding files. |

---

### Task 1: Write failing tests for `bpm_from_beats`, `resolve_octave`, `_snap_to_perceptual`

**Files:**
- Modify: `analyzer/tests/test_detect_bpm.py`

- [ ] **Step 1: Append the test cases**

Append at the end of `analyzer/tests/test_detect_bpm.py`:

```python


import numpy as np
import pytest

from dj_analyze.detect_bpm import (
    bpm_from_beats,
    resolve_octave,
    _snap_to_perceptual,
    PERCEPTUAL_LOW,
    PERCEPTUAL_HIGH,
    PERCEPTUAL_DEFAULT,
)


def test_bpm_from_beats_120_bpm() -> None:
    # 4 beats at 0.5-second intervals → 120 BPM.
    sr = 22050
    beat_times = np.array([0.0, 0.5, 1.0, 1.5])
    beat_frames = (beat_times * sr).astype(int)
    bpm = bpm_from_beats(beat_frames, sr=sr)
    assert bpm is not None
    assert abs(bpm - 120.0) < 0.5


def test_bpm_from_beats_returns_none_for_empty() -> None:
    assert bpm_from_beats(np.array([]), sr=22050) is None


def test_bpm_from_beats_returns_none_for_single_beat() -> None:
    assert bpm_from_beats(np.array([1000]), sr=22050) is None


def test_bpm_from_beats_returns_none_for_zero_interval() -> None:
    # Two beats at the same frame → zero IBI.
    assert bpm_from_beats(np.array([100, 100]), sr=22050) is None


def test_resolve_octave_exact_agreement_passes_through() -> None:
    assert resolve_octave(120.0, 120.0) == pytest.approx(120.0, abs=0.5)


def test_resolve_octave_primary_doubled_secondary_correct() -> None:
    # primary librosa-locked at 152; beat_track sees 76 (truth).
    # primary÷2 = 76 matches secondary = 76 → return ~76.
    result = resolve_octave(152.0, 76.0)
    assert abs(result - 76.0) < 2.0


def test_resolve_octave_primary_correct_secondary_doubled() -> None:
    # primary = 95 (true); beat_track over-counted at 190.
    # secondary÷2 = 95 matches primary = 95 → return ~95.
    result = resolve_octave(95.0, 190.0)
    assert abs(result - 95.0) < 2.0


def test_resolve_octave_primary_halved_secondary_correct() -> None:
    # primary = 60 (out of range); beat_track = 120 (truth).
    # primary×2 = 120 matches secondary = 120 → return ~120.
    result = resolve_octave(60.0, 120.0)
    assert abs(result - 120.0) < 2.0


def test_resolve_octave_secondary_none_snaps_primary() -> None:
    # No beat_track signal — snap primary into perceptual range if possible.
    assert resolve_octave(60.0, None) == pytest.approx(120.0, abs=0.5)
    assert resolve_octave(140.0, None) == pytest.approx(140.0, abs=0.5)


def test_resolve_octave_disagreement_picks_closer_to_120() -> None:
    # primary = 95, secondary = 145 (no octave relationship).
    # 95 distance from 120 = 25; 145 distance from 120 = 25 — tie.
    # Whichever is picked must still be one of the two.
    result = resolve_octave(95.0, 145.0)
    assert result in pytest.approx((95.0, 145.0), abs=0.5)


def test_resolve_octave_handles_zero_primary() -> None:
    # Should not divide by zero or loop forever.
    result = resolve_octave(0.0, 120.0)
    assert result == pytest.approx(120.0, abs=0.5)


def test_snap_to_perceptual_below_range_doubles() -> None:
    assert _snap_to_perceptual(40.0) == pytest.approx(80.0, abs=0.001)


def test_snap_to_perceptual_above_range_halves() -> None:
    assert _snap_to_perceptual(240.0) == pytest.approx(120.0, abs=0.001)


def test_snap_to_perceptual_in_range_passes_through() -> None:
    assert _snap_to_perceptual(100.0) == pytest.approx(100.0, abs=0.001)
    assert _snap_to_perceptual(PERCEPTUAL_LOW) == pytest.approx(PERCEPTUAL_LOW, abs=0.001)
    assert _snap_to_perceptual(PERCEPTUAL_HIGH) == pytest.approx(PERCEPTUAL_HIGH, abs=0.001)


def test_snap_to_perceptual_zero_passes_through() -> None:
    assert _snap_to_perceptual(0.0) == 0.0


def test_snap_to_perceptual_negative_passes_through() -> None:
    assert _snap_to_perceptual(-10.0) == pytest.approx(-10.0, abs=0.001)
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run:
```
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer
.venv\Scripts\python.exe -m pytest tests/test_detect_bpm.py -v
```

Expected: `ImportError` on the new symbols (`bpm_from_beats`, `resolve_octave`, `_snap_to_perceptual`, `PERCEPTUAL_LOW`, ...) because they don't exist yet. That's the TDD RED phase.

- [ ] **Step 3: Commit the failing tests**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add analyzer/tests/test_detect_bpm.py
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "test(analyzer): failing tests for BPM octave resolution helpers (TDD)"
```

---

### Task 2: Implement the helpers in `detect_bpm.py`

**Files:**
- Modify: `analyzer/src/dj_analyze/detect_bpm.py`

- [ ] **Step 1: Add constants and helper functions**

In `analyzer/src/dj_analyze/detect_bpm.py`, ABOVE the existing `def detect_bpm(...)` function (around line 9, just after the imports), insert:

```python
PERCEPTUAL_LOW = 70.0
PERCEPTUAL_HIGH = 180.0
PERCEPTUAL_DEFAULT = 120.0
AGREEMENT_TOLERANCE = 0.06  # ±6%, matches _VOTE_TOLERANCE below.


def bpm_from_beats(beat_frames: np.ndarray, sr: int) -> float | None:
    """Derive a BPM estimate from beat-frame positions by taking 60 / median IBI.

    Returns ``None`` when the input has fewer than 2 beats or the median inter-beat
    interval is non-positive. Used as a second, independent BPM estimate to cross-check
    the global ``librosa.beat.tempo`` output for octave errors.
    """
    if beat_frames is None or len(beat_frames) < 2:
        return None
    beat_times = librosa.frames_to_time(beat_frames, sr=sr)
    intervals = np.diff(beat_times)
    if len(intervals) == 0:
        return None
    median_ibi = float(np.median(intervals))
    if median_ibi <= 0:
        return None
    return 60.0 / median_ibi


def _snap_to_perceptual(bpm: float) -> float:
    """Octave-shift ``bpm`` into the perceptual range [70, 180] when possible.

    Multiplies by 2 if too low, divides by 2 if too high, up to a safety bound to
    avoid infinite loops on degenerate inputs. Returns the input unchanged when it's
    zero, negative, or already in range.
    """
    if bpm <= 0:
        return bpm
    out = bpm
    safety = 10
    while out < PERCEPTUAL_LOW and safety > 0:
        out *= 2.0
        safety -= 1
    while out > PERCEPTUAL_HIGH and safety > 0:
        out /= 2.0
        safety -= 1
    return float(out)


def resolve_octave(primary: float, secondary: float | None) -> float:
    """Pick the best BPM given two estimates and the perceptual-range prior.

    When ``secondary`` is ``None``, returns ``_snap_to_perceptual(primary)``.

    Otherwise, generates ``{÷2, ×1, ×2}`` octave candidates for both estimates,
    filters to those inside ``[PERCEPTUAL_LOW, PERCEPTUAL_HIGH]``, and looks for an
    agreeing pair within ``AGREEMENT_TOLERANCE``. On agreement, returns the average.
    On disagreement, returns the in-range candidate closest to ``PERCEPTUAL_DEFAULT``.
    If nothing is in range, falls back to ``_snap_to_perceptual(primary)``.
    """
    if primary <= 0:
        # Trust the secondary if it exists and is positive, else give up.
        if secondary is not None and secondary > 0:
            return _snap_to_perceptual(secondary)
        return primary

    if secondary is None:
        return _snap_to_perceptual(primary)

    primary_candidates = (primary / 2.0, primary, primary * 2.0)
    secondary_candidates = (secondary / 2.0, secondary, secondary * 2.0)
    primary_in_range = [c for c in primary_candidates if PERCEPTUAL_LOW <= c <= PERCEPTUAL_HIGH]
    secondary_in_range = [c for c in secondary_candidates if PERCEPTUAL_LOW <= c <= PERCEPTUAL_HIGH]

    best_pair: tuple[float, float] | None = None
    best_rel_diff = float("inf")
    for p in primary_in_range:
        for s in secondary_in_range:
            rel = abs(p - s) / max(p, s, 1e-9)
            if rel < best_rel_diff:
                best_rel_diff = rel
                best_pair = (p, s)

    if best_pair is not None and best_rel_diff <= AGREEMENT_TOLERANCE:
        return float((best_pair[0] + best_pair[1]) / 2.0)

    all_in_range = primary_in_range + secondary_in_range
    if all_in_range:
        return float(min(all_in_range, key=lambda c: abs(c - PERCEPTUAL_DEFAULT)))

    return _snap_to_perceptual(primary)
```

- [ ] **Step 2: Run the unit tests to verify the helpers pass**

```
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer
.venv\Scripts\python.exe -m pytest tests/test_detect_bpm.py -v
```

Expected: the new tests pass. The old tests for `_octave_normalize` and `detect_bpm` should still pass (they're unchanged).

If any pre-existing test fails, the helper additions inadvertently broke something — STOP and inspect. The helpers are pure-functional and module-level; they should not affect other tests.

- [ ] **Step 3: Commit the helpers**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add analyzer/src/dj_analyze/detect_bpm.py
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(analyzer): bpm_from_beats / resolve_octave / _snap_to_perceptual helpers"
```

---

### Task 3: Integrate the helpers into `detect_bpm`

**Files:**
- Modify: `analyzer/src/dj_analyze/detect_bpm.py` (the `detect_bpm` function body).

- [ ] **Step 1: Extend `detect_bpm` to use the new helpers**

Replace the existing body of `detect_bpm`:

```python
def detect_bpm(audio: np.ndarray, sr: int) -> tuple[float, float]:
    """Estimate global BPM and a confidence score in [0, 1].

    Confidence is computed by running tempo detection on non-overlapping 10-second
    windows and measuring how tightly the window estimates cluster around the global
    estimate. A track with perfectly steady tempo gives confidence ≈ 1.0.
    """
    onset_env = librosa.onset.onset_strength(y=audio, sr=sr)
    global_tempo = librosa.beat.tempo(onset_envelope=onset_env, sr=sr, aggregate=np.median)
    bpm = float(global_tempo[0])

    confidence = _tempo_stability(audio, sr, bpm)
    return bpm, confidence
```

with the new body that cross-checks against `beat_track`:

```python
def detect_bpm(audio: np.ndarray, sr: int) -> tuple[float, float]:
    """Estimate global BPM and a confidence score in [0, 1].

    The global tempo from ``librosa.beat.tempo`` is octave-ambiguous on tracks where
    the onset envelope is consistent with both X BPM and 2X BPM. We cross-check
    against a second estimate from ``librosa.beat.beat_track`` (median inter-beat
    interval), then route both through ``resolve_octave`` which prefers candidates
    inside the perceptual range [70, 180] BPM.

    Confidence is then computed against the *resolved* BPM, so windows must agree
    with the corrected value (not the raw librosa output).
    """
    onset_env = librosa.onset.onset_strength(y=audio, sr=sr)

    # Primary estimate: median-aggregated tempo over the onset envelope.
    global_tempo = librosa.beat.tempo(onset_envelope=onset_env, sr=sr, aggregate=np.median)
    primary = float(global_tempo[0])

    # Secondary estimate from actual beat positions; may be None if beat_track fails.
    try:
        _, beat_frames = librosa.beat.beat_track(onset_envelope=onset_env, sr=sr)
    except Exception:
        beat_frames = None
    secondary = bpm_from_beats(beat_frames, sr) if beat_frames is not None else None

    resolved = resolve_octave(primary, secondary)
    confidence = _tempo_stability(audio, sr, resolved)
    return resolved, confidence
```

- [ ] **Step 2: Run the full analyzer test suite**

```
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer
.venv\Scripts\python.exe -m pytest -v
```

Expected: all tests pass. The existing `test_detects_120_bpm_within_tolerance` should still pass — 120 BPM is in the perceptual range, the primary estimate likely already returns ~120, and the resolver passes it through.

If `test_confidence_is_high_on_clean_click_track` fails (confidence drops below 0.7), inspect: the resolver might have changed the BPM target for `_tempo_stability` such that fewer windows vote. If so, the click track's resolved BPM is wrong (or the test fixture is octave-flipped), so investigate.

If the test suite reveals a regression (e.g., previously-correct 120 BPM tracks now return 60 because of an over-aggressive snap), STOP and inspect the resolver logic — the AGREEMENT_TOLERANCE branch should not fire incorrectly when primary == secondary.

- [ ] **Step 3: Commit the integration**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add analyzer/src/dj_analyze/detect_bpm.py
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(analyzer): detect_bpm cross-checks against beat_track to resolve octave errors"
```

---

### Task 4: Re-run the analyzer on the user's library

**Files:**
- Writes to: `analyzed-real/*.json` (refreshes the existing 24 sidecars with corrected BPMs).

- [ ] **Step 1: Bump the analyzer version so existing sidecars are invalidated**

The analyzer caches sidecars based on `source_mtime`, `source_size_bytes`, AND `analyzer_version`. Bumping the version invalidates the cache and forces re-analysis.

Find the version constant in the analyzer code. Likely candidates:
- `analyzer/src/dj_analyze/__init__.py` (probably has `__version__`)
- `analyzer/pyproject.toml` (the `[project] version = "..."` field)
- `analyzer/src/dj_analyze/sidecar.py` (a constant like `ANALYZER_VERSION`)

Bump the patch version. For example, `0.1.0` → `0.1.1`.

- [ ] **Step 2: Re-run the analyzer**

Run from the repo root:

```
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer
.venv\Scripts\python.exe -m dj_analyze C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\data C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzed-real
```

Expected:
- All 24 tracks get re-analyzed.
- Sidecars in `analyzed-real/` are updated with the new BPMs.
- Some tracks' `bpm` values may change (the whole point) — note which ones.
- Some tracks' `bpm_confidence` may change too (windows now vote against the resolved value, not the raw global).

If the analyzer takes >10 minutes, that's expected for 24 tracks. Wait it out.

- [ ] **Step 3: Diff the sidecars to see what changed**

For comparison, before this task ran, the BPMs were known:
- 5 tracks at 139.6748310810811
- 4 tracks at 151.99908088235293
- 2 tracks at 97.50884433962264
- 015 had confidence 0.5

Run:
```
grep -H '"bpm":' C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzed-real/*.mp3.json
```

Note any BPMs that moved by an octave (×0.5 or ×2). These are the tracks the fix corrected.

- [ ] **Step 4: Commit the regenerated sidecars**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add analyzed-real/
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "chore(analyzer): re-analyze library with octave-resolved BPMs"
```

(If `analyzed-real/` is gitignored, skip this commit and just leave the regenerated files in place — they'll be picked up by the engine on next restart.)

---

### Task 5: Tell the user to restart the engine

This task is just a reporting step — no code changes.

- [ ] **Step 1: Report to controller that the analyzer fix is complete and the library has been re-analyzed**

The controller will then either:
- Stop the running engine, restart it (so it loads the new sidecars), and report listening guidance to the user.
- Or report to the user that the fix is ready and the user can restart at their convenience.

---

## Self-review notes

**Spec coverage check:**
- Algorithm change → Tasks 2 & 3.
- Test coverage → Task 1.
- Re-analyze the library → Task 4.
- Engine restart → Task 5 (controller-driven, not subagent-driven).

**Placeholder scan:** No TBDs. Every step has a concrete command, code block, or assertion.

**Type consistency:**
- `bpm_from_beats` signature: `(beat_frames: np.ndarray, sr: int) -> float | None`. Tests pass `np.array` for beat_frames. Consistent.
- `resolve_octave` signature: `(primary: float, secondary: float | None) -> float`. Tests pass floats and `None`. Consistent.
- `_snap_to_perceptual` signature: `(bpm: float) -> float`. Consistent.

**Risk: existing tests change behavior.**
The 120 BPM click-track test should still pass — the resolver passes 120 through unchanged when both primary and secondary agree on it. But if the existing fixture is mis-detected (e.g., librosa returns 60 for a true-120 click), the resolver might *now* return 60×2=120 — which is the right answer but a different code path than before. Either way the test's assertion (118 ≤ bpm ≤ 122) should hold.
