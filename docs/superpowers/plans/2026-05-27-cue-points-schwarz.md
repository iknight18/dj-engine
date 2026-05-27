# Cue Points (Schwarz-Derived) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the analyzer's coarse 10%-of-mean energy cues and blind ±16-bar intro/outro offsets with Schwarz-derived rules (loudness anchor, Foote novelty on fused chroma+onset SSM, long-track shortening) — improving every transition the engine performs.

**Architecture:** Three additive changes to the analyzer. New `detect_novelty.py` module produces structural peak timestamps. `detect_cues.py` is rewritten to consume novelty + a per-window RMS-dB curve and emit better cues. `detect_downbeats.py` exports `compute_chroma_ssm` so the SSM is built once and reused. `detect_energy.py` returns a dataclass exposing both the existing normalized curve and a new RMS-dB curve. `analyze.py` orchestrates. No schema change; analyzer_version bumps 0.2.0 → 0.3.0 to invalidate all sidecars.

**Tech Stack:** Python 3.11+, librosa, scipy, numpy, pytest. No new dependencies.

**Reference:** `docs/superpowers/specs/2026-05-27-cue-points-schwarz-design.md`

---

## File map

**Create:**
- `analyzer/src/dj_analyze/detect_novelty.py` — Foote-novelty module
- `analyzer/tests/test_detect_novelty.py` — unit tests

**Modify:**
- `analyzer/src/dj_analyze/__init__.py` — version bump 0.2.0 → 0.3.0
- `analyzer/src/dj_analyze/detect_energy.py` — return `EnergyResult` dataclass; add `rms_db_curve` and `max_rms_db`
- `analyzer/src/dj_analyze/detect_downbeats.py` — extract `compute_chroma_ssm`; `detect_phrases` accepts optional `chroma_ssm`
- `analyzer/src/dj_analyze/detect_cues.py` — rewrite: loudness anchor + novelty-driven intro/outro + long-track shortening
- `analyzer/src/dj_analyze/analyze.py` — wire SSM + novelty + new energy result through to cues
- `analyzer/tests/test_detect_energy.py` — update for new return type + new fields
- `analyzer/tests/test_detect_downbeats.py` — verify `compute_chroma_ssm` extraction; existing `detect_phrases` tests still pass
- `analyzer/tests/test_detect_cues.py` — replace 10%-of-mean tests with loudness-anchor + novelty-driven cases
- `analyzer/tests/test_analyze.py` — verify orchestrator wires everything together

---

## Task 1: `detect_energy` adds RMS-dB curve and max

**Why first:** the rewritten `detect_cues` depends on `rms_db_curve` and `max_rms_db`. We refactor the return type now, then update callers, then keep building on a clean signature.

**Files:**
- Modify: `analyzer/src/dj_analyze/detect_energy.py`
- Modify: `analyzer/tests/test_detect_energy.py`
- Modify: `analyzer/src/dj_analyze/analyze.py` (caller — updates to use new return shape)

- [ ] **Step 1: Write failing tests for the new `EnergyResult` dataclass and dB fields**

Append to `analyzer/tests/test_detect_energy.py`:

```python
import numpy as np

from dj_analyze.detect_energy import EnergyResult, detect_energy


def test_detect_energy_returns_energy_result():
    sr = 22050
    audio = np.sin(2 * np.pi * 440 * np.arange(sr * 4) / sr).astype(np.float32) * 0.5
    result = detect_energy(audio, sr)
    assert isinstance(result, EnergyResult)
    assert hasattr(result, "curve")
    assert hasattr(result, "mean")
    assert hasattr(result, "rms_db_curve")
    assert hasattr(result, "max_rms_db")


def test_detect_energy_rms_db_curve_window_count():
    # 4-second audio, 2-second windows -> ~2 windows
    sr = 22050
    audio = np.sin(2 * np.pi * 440 * np.arange(sr * 4) / sr).astype(np.float32) * 0.5
    result = detect_energy(audio, sr)
    # Allow off-by-one due to hop/frame boundaries.
    assert 1 <= len(result.rms_db_curve) <= 3
    assert result.max_rms_db == max(result.rms_db_curve)


def test_detect_energy_silent_audio_dB_safe():
    sr = 22050
    audio = np.zeros(sr * 4, dtype=np.float32)
    result = detect_energy(audio, sr)
    # Silence must not produce NaN or +inf; we cap at a floor.
    assert all(np.isfinite(v) for v in result.rms_db_curve)
    assert np.isfinite(result.max_rms_db)


def test_detect_energy_loud_section_has_higher_db():
    sr = 22050
    quiet = np.sin(2 * np.pi * 440 * np.arange(sr * 2) / sr).astype(np.float32) * 0.05
    loud = np.sin(2 * np.pi * 440 * np.arange(sr * 2) / sr).astype(np.float32) * 0.5
    audio = np.concatenate([quiet, loud])
    result = detect_energy(audio, sr)
    # The loud half should be at least 15 dB above the quiet half.
    assert result.rms_db_curve[-1] - result.rms_db_curve[0] > 15.0
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzer
.venv/Scripts/python.exe -m pytest tests/test_detect_energy.py -v
```

Expected: import error or attribute error — `EnergyResult` doesn't exist yet.

- [ ] **Step 3: Refactor `detect_energy.py` to return `EnergyResult`**

Replace the contents of `analyzer/src/dj_analyze/detect_energy.py`:

```python
"""Per-second RMS energy curve + per-2s RMS-dB curve.

The normalized per-second curve (and its mean) feed the selector and engine.
The dB curve and the track-max dB feed Schwarz's loudness-anchor cue detection.
"""

from __future__ import annotations

from dataclasses import dataclass

import librosa
import numpy as np

_DB_FLOOR = -120.0  # dB floor for safe handling of digital silence
_RMS_DB_WINDOW_S = 2.0


@dataclass(frozen=True, slots=True)
class EnergyResult:
    curve: list[float]          # per-second normalized RMS, in [0, 1]
    mean: float                 # mean of curve
    rms_db_curve: list[float]   # per-2s-window peak RMS, in dB (FS reference: 1.0)
    max_rms_db: float           # max of rms_db_curve, or _DB_FLOOR if all silent


def detect_energy(audio: np.ndarray, sr: int) -> EnergyResult:
    """Return (per-second normalized curve + mean) and (per-2s dB curve + max).

    Per-second curve normalization divides by the 95th percentile to be robust
    to short loud transients. The dB curve is NOT normalized; it carries true
    loudness so downstream rules can threshold on -9 dB-of-max (Schwarz).
    """
    # ---- per-second normalized curve (existing behavior) ----
    sec_hop = sr
    sec_frame = sr
    sec_rms = librosa.feature.rms(y=audio, frame_length=sec_frame, hop_length=sec_hop)
    sec_raw = sec_rms[0].astype(np.float64)
    if len(sec_raw) == 0:
        return EnergyResult(curve=[], mean=0.0, rms_db_curve=[], max_rms_db=_DB_FLOOR)
    p95 = float(np.percentile(sec_raw, 95))
    if p95 <= 0:
        normalized = np.zeros_like(sec_raw)
    else:
        normalized = np.clip(sec_raw / p95, 0.0, 1.0)
    curve = [float(v) for v in normalized]
    mean = float(normalized.mean())

    # ---- per-2s peak-RMS curve in dB (new) ----
    win_frames = int(round(_RMS_DB_WINDOW_S * sr))
    if win_frames < 1 or len(audio) < win_frames:
        return EnergyResult(curve=curve, mean=mean, rms_db_curve=[], max_rms_db=_DB_FLOOR)

    db_rms = librosa.feature.rms(y=audio, frame_length=win_frames, hop_length=win_frames)
    db_raw = db_rms[0].astype(np.float64)
    # Convert to dB with a floor for safe log on zero.
    with np.errstate(divide="ignore"):
        db_vals = 20.0 * np.log10(np.maximum(db_raw, 1e-9))
    db_vals = np.maximum(db_vals, _DB_FLOOR)
    rms_db_curve = [float(v) for v in db_vals]
    max_rms_db = float(np.max(db_vals)) if len(db_vals) else _DB_FLOOR

    return EnergyResult(
        curve=curve,
        mean=mean,
        rms_db_curve=rms_db_curve,
        max_rms_db=max_rms_db,
    )
```

- [ ] **Step 4: Update existing test cases that unpack the old tuple**

There are three existing patterns in `analyzer/tests/test_detect_energy.py` that break under the new return type:

```python
curve, mean = detect_energy(audio, sr)     # → result = detect_energy(audio, sr); curve, mean = result.curve, result.mean
curve, _ = detect_energy(audio, sr)        # → result = detect_energy(audio, sr); curve = result.curve
```

Replace those exact patterns in-place. The downstream assertions on `curve` (length, value range, indexed access) continue to work as-is once `curve` references `result.curve`.

- [ ] **Step 5: Update `analyze.py` to use new return type**

In `analyzer/src/dj_analyze/analyze.py`, find this line (near line 53):

```python
energy_curve, energy_mean = detect_energy(audio, sr)
```

Replace with:

```python
energy = detect_energy(audio, sr)
```

And anywhere `energy_curve` / `energy_mean` is referenced (passing to `detect_cues`, building the output dict), replace with `energy.curve` / `energy.mean`. Specifically:

- The `detect_cues(...)` call site: `energy_curve=energy_curve` → `energy_curve=energy.curve`
- The output dict: `"energy_curve": energy_curve, "energy_mean": float(energy_mean)` → `"energy_curve": energy.curve, "energy_mean": float(energy.mean)`

Do NOT yet wire `rms_db_curve` / `max_rms_db` into `detect_cues` — that happens in Task 7.

- [ ] **Step 6: Run all energy + analyze tests to confirm green**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzer
.venv/Scripts/python.exe -m pytest tests/test_detect_energy.py tests/test_analyze.py -v
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine
git add analyzer/src/dj_analyze/detect_energy.py analyzer/src/dj_analyze/analyze.py analyzer/tests/test_detect_energy.py
git commit -m "feat(analyzer): EnergyResult dataclass with per-2s RMS-dB curve"
```

---

## Task 2: extract `compute_chroma_ssm` from `detect_downbeats`

**Why:** the novelty detector (Task 3) needs the chroma SSM. Today `detect_phrases` builds one privately. We extract it so it can be computed once and threaded through to both `detect_phrases` and the new `detect_novelty`.

**Files:**
- Modify: `analyzer/src/dj_analyze/detect_downbeats.py`
- Modify: `analyzer/tests/test_detect_downbeats.py`

- [ ] **Step 1: Add failing test for `compute_chroma_ssm`**

Append to `analyzer/tests/test_detect_downbeats.py`:

```python
import numpy as np

from dj_analyze.detect_downbeats import compute_chroma_ssm, detect_phrases


def test_compute_chroma_ssm_returns_square_matrix():
    sr = 22050
    # 8 seconds of audio with two halves in different keys.
    seg = sr * 4
    a = np.sin(2 * np.pi * 220 * np.arange(seg) / sr).astype(np.float32) * 0.5  # A3
    b = np.sin(2 * np.pi * 330 * np.arange(seg) / sr).astype(np.float32) * 0.5  # E4
    audio = np.concatenate([a, b])
    # Beats every 0.5s (simulated 120 BPM)
    beats = [0.5 * i for i in range(16)]
    ssm = compute_chroma_ssm(audio, sr, beats)
    assert ssm is not None
    assert ssm.ndim == 2
    assert ssm.shape[0] == ssm.shape[1]
    # Affinity matrix in [0, 1].
    assert ssm.min() >= 0.0 - 1e-6
    assert ssm.max() <= 1.0 + 1e-6


def test_compute_chroma_ssm_too_short_returns_none():
    sr = 22050
    audio = np.zeros(100, dtype=np.float32)
    assert compute_chroma_ssm(audio, sr, beats=[]) is None


def test_detect_phrases_accepts_precomputed_ssm():
    sr = 22050
    seg = sr * 4
    a = np.sin(2 * np.pi * 220 * np.arange(seg) / sr).astype(np.float32) * 0.5
    b = np.sin(2 * np.pi * 330 * np.arange(seg) / sr).astype(np.float32) * 0.5
    audio = np.concatenate([a, b])
    beats = [0.5 * i for i in range(16)]
    ssm = compute_chroma_ssm(audio, sr, beats)
    # Passing the precomputed SSM should yield the same result as not passing it.
    r1 = detect_phrases(audio, sr, beats=beats, output_stride_bars=4)
    r2 = detect_phrases(audio, sr, beats=beats, output_stride_bars=4, chroma_ssm=ssm)
    assert r1.boundaries == r2.boundaries
    assert abs(r1.confidence - r2.confidence) < 1e-6
```

- [ ] **Step 2: Run to confirm fail**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzer
.venv/Scripts/python.exe -m pytest tests/test_detect_downbeats.py::test_compute_chroma_ssm_returns_square_matrix tests/test_detect_downbeats.py::test_detect_phrases_accepts_precomputed_ssm -v
```

Expected: ImportError (`compute_chroma_ssm` not defined) or TypeError (unexpected keyword `chroma_ssm`).

- [ ] **Step 3: Extract `compute_chroma_ssm` and add `chroma_ssm` param to `detect_phrases`**

In `analyzer/src/dj_analyze/detect_downbeats.py`, do the following structural change:

1. Add at module top, after the existing `_SEARCH_PERIODS_BEATS` constant:

```python
def compute_chroma_ssm(
    audio: np.ndarray, sr: int, beats: list[float]
) -> np.ndarray | None:
    """Beat-synchronous chroma self-similarity matrix.

    Returns None if the track is too short to build a meaningful SSM
    (fewer beats than the minimum search period, or sub-second audio).
    """
    if len(beats) < 4 or len(audio) < sr:
        return None
    import librosa

    beat_frames = librosa.time_to_frames(np.asarray(beats), sr=sr)
    chroma = librosa.feature.chroma_cqt(y=audio, sr=sr)
    chroma_sync = librosa.util.sync(chroma, beat_frames, aggregate=np.median)
    if chroma_sync.shape[1] < min(_SEARCH_PERIODS_BEATS) + 1:
        return None
    if hasattr(librosa.segment, "cross_similarity"):
        sim = librosa.segment.cross_similarity(
            chroma_sync, chroma_sync, mode="affinity"
        )
    else:
        sim = librosa.segment.recurrence_matrix(chroma_sync, mode="affinity")
    return np.asarray(sim, dtype=np.float64)
```

2. Change the `detect_phrases` signature and body — replace the existing function with:

```python
def detect_phrases(
    audio: np.ndarray,
    sr: int,
    beats: list[float],
    *,
    output_stride_bars: int = 16,
    phrase_bars: int | None = None,  # legacy alias for output_stride_bars
    chroma_ssm: np.ndarray | None = None,  # NEW: reuse a precomputed SSM if available
) -> PhraseResult:
    """Find repeating phrase structure and return boundaries at a fixed bar stride.

    See module docstring for details. Pass `chroma_ssm` to avoid recomputing the
    SSM when the caller (analyze.py) has already built it for novelty detection.
    """
    if phrase_bars is not None:
        output_stride_bars = phrase_bars
    if len(beats) < 4 or len(audio) < sr:
        return PhraseResult(boundaries=[], confidence=0.0)

    sim = chroma_ssm if chroma_ssm is not None else compute_chroma_ssm(audio, sr, beats)
    if sim is None:
        return PhraseResult(boundaries=[], confidence=0.0)

    candidate_periods = [p for p in _SEARCH_PERIODS_BEATS if p < sim.shape[0]]
    if not candidate_periods:
        return PhraseResult(boundaries=[], confidence=0.0)
    scores: dict[int, float] = {}
    for p in candidate_periods:
        diag = [sim[i, i + p] for i in range(sim.shape[0] - p)]
        scores[p] = float(np.max(diag)) if diag else 0.0

    best_period = max(scores, key=lambda k: scores[k])
    best_score = scores[best_period]
    confidence = float(max(0.0, min(1.0, best_score)))

    median = float(np.median(sim))
    anchor = 0
    for i in range(sim.shape[0] - best_period):
        if sim[i, i + best_period] > median:
            anchor = i
            break

    stride_beats = max(1, output_stride_bars) * 4
    boundary_indices = list(range(anchor, len(beats), stride_beats))
    boundaries = [beats[i] for i in boundary_indices]
    return PhraseResult(boundaries=boundaries, confidence=confidence)
```

Everything outside `detect_phrases` and the new `compute_chroma_ssm` stays untouched.

- [ ] **Step 4: Run tests to confirm green**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzer
.venv/Scripts/python.exe -m pytest tests/test_detect_downbeats.py -v
```

Expected: all tests pass — both the new tests and the existing `detect_phrases` / `detect_downbeats` cases.

- [ ] **Step 5: Commit**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine
git add analyzer/src/dj_analyze/detect_downbeats.py analyzer/tests/test_detect_downbeats.py
git commit -m "refactor(analyzer): extract compute_chroma_ssm for reuse"
```

---

## Task 3: `detect_novelty` module with fused chroma+onset SSM

**Files:**
- Create: `analyzer/src/dj_analyze/detect_novelty.py`
- Create: `analyzer/tests/test_detect_novelty.py`

- [ ] **Step 1: Write failing tests for novelty detection**

Create `analyzer/tests/test_detect_novelty.py`:

```python
import numpy as np
import pytest

from dj_analyze.detect_downbeats import compute_chroma_ssm
from dj_analyze.detect_novelty import NoveltyResult, detect_novelty


def _two_section_audio(sr: int, dur_s: float, freq1: float, freq2: float) -> np.ndarray:
    seg = int(sr * dur_s / 2)
    t1 = np.arange(seg) / sr
    t2 = np.arange(seg) / sr
    a = (np.sin(2 * np.pi * freq1 * t1) * 0.5).astype(np.float32)
    b = (np.sin(2 * np.pi * freq2 * t2) * 0.5).astype(np.float32)
    return np.concatenate([a, b])


def _beats_for(dur_s: float, bpm: float = 120.0) -> list[float]:
    period = 60.0 / bpm
    n = int(dur_s / period)
    return [period * i for i in range(n)]


def test_detect_novelty_two_section_track_has_peak_near_boundary():
    sr = 22050
    dur_s = 32.0
    audio = _two_section_audio(sr, dur_s, freq1=220.0, freq2=440.0)
    beats = _beats_for(dur_s)
    result = detect_novelty(audio, sr, beats=beats)
    assert isinstance(result, NoveltyResult)
    assert len(result.peaks_seconds) >= 1
    # Boundary is at 16 s. Expect a peak within +/- 2 s.
    closest = min(result.peaks_seconds, key=lambda t: abs(t - 16.0))
    assert abs(closest - 16.0) <= 2.0
    assert result.confidence > 0.0


def test_detect_novelty_uniform_track_has_low_confidence():
    sr = 22050
    dur_s = 30.0
    audio = (np.sin(2 * np.pi * 440 * np.arange(int(sr * dur_s)) / sr) * 0.5).astype(np.float32)
    beats = _beats_for(dur_s)
    result = detect_novelty(audio, sr, beats=beats)
    # No real structural change -> confidence should be low.
    assert result.confidence < 0.5


def test_detect_novelty_drum_drop_detected():
    sr = 22050
    dur_s = 32.0
    # First half: pure sine. Second half: same sine + click on every 4 beats.
    half = int(sr * dur_s / 2)
    t = np.arange(half) / sr
    sine = np.sin(2 * np.pi * 220 * t) * 0.3
    sine2 = np.sin(2 * np.pi * 220 * (np.arange(half) / sr + dur_s / 2)) * 0.3
    # Add clicks at every 0.5s in the second half.
    clicks = np.zeros(half, dtype=np.float32)
    for k in range(int(dur_s / 2 / 0.5)):
        idx = int(k * 0.5 * sr)
        if idx < half:
            clicks[idx : idx + 50] = 0.5  # short impulse
    audio = np.concatenate([sine.astype(np.float32), (sine2 + clicks).astype(np.float32)])
    beats = _beats_for(dur_s)
    result = detect_novelty(audio, sr, beats=beats)
    # Expect at least one peak near the drum-entry boundary at 16 s.
    assert len(result.peaks_seconds) >= 1
    closest = min(result.peaks_seconds, key=lambda t: abs(t - 16.0))
    assert abs(closest - 16.0) <= 3.0


def test_detect_novelty_accepts_precomputed_ssm():
    sr = 22050
    dur_s = 32.0
    audio = _two_section_audio(sr, dur_s, freq1=220.0, freq2=440.0)
    beats = _beats_for(dur_s)
    ssm = compute_chroma_ssm(audio, sr, beats)
    r1 = detect_novelty(audio, sr, beats=beats)
    r2 = detect_novelty(audio, sr, beats=beats, chroma_ssm=ssm)
    assert r1.peaks_seconds == pytest.approx(r2.peaks_seconds, abs=0.01)
    assert abs(r1.confidence - r2.confidence) < 1e-6


def test_detect_novelty_empty_beats_returns_empty():
    sr = 22050
    audio = np.zeros(sr * 5, dtype=np.float32)
    result = detect_novelty(audio, sr, beats=[])
    assert result.peaks_seconds == []
    assert result.confidence == 0.0
```

- [ ] **Step 2: Run to confirm fail**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzer
.venv/Scripts/python.exe -m pytest tests/test_detect_novelty.py -v
```

Expected: ImportError — module doesn't exist.

- [ ] **Step 3: Implement `detect_novelty.py`**

Create `analyzer/src/dj_analyze/detect_novelty.py`:

```python
"""Foote-style novelty detection on a fused chroma + onset SSM.

Returns timestamped peaks where the music's structural character changes.
Used by detect_cues.py to anchor intro_end / outro_start at musical boundaries
rather than at regular-grid bar offsets.

Reference: J. Foote, "Visualizing Music and Audio Using Self-Similarity," 1999.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
from scipy.signal import find_peaks

from .detect_downbeats import compute_chroma_ssm

NOVELTY_KERNEL_BEATS = 16
NOVELTY_MIN_PROMINENCE_MAD = 1.0
NOVELTY_MIN_PEAK_DISTANCE_BEATS = 8


@dataclass(frozen=True, slots=True)
class NoveltyResult:
    peaks_seconds: list[float]    # sorted; one entry per accepted peak
    peak_strengths: list[float]   # parallel to peaks_seconds
    confidence: float             # 0..1; how clearly the strongest peak rises above the noise floor


def detect_novelty(
    audio: np.ndarray,
    sr: int,
    beats: list[float],
    *,
    chroma_ssm: np.ndarray | None = None,
) -> NoveltyResult:
    """Find structural-boundary timestamps via Foote novelty on a fused SSM.

    Args:
        audio: float32 mono PCM.
        sr: sample rate.
        beats: beat timestamps in seconds (from detect_beats).
        chroma_ssm: optional precomputed chroma SSM (from compute_chroma_ssm)
            to avoid duplicate work when the caller has one in hand.

    Returns:
        NoveltyResult with peaks (in seconds), peak strengths, and confidence.
        Returns empty result for sub-second audio or fewer than 4 beats.
    """
    if len(beats) < 4 or len(audio) < sr:
        return NoveltyResult(peaks_seconds=[], peak_strengths=[], confidence=0.0)

    chroma = chroma_ssm if chroma_ssm is not None else compute_chroma_ssm(audio, sr, beats)
    if chroma is None or chroma.shape[0] < NOVELTY_KERNEL_BEATS:
        return NoveltyResult(peaks_seconds=[], peak_strengths=[], confidence=0.0)

    onset = _compute_onset_ssm(audio, sr, beats)
    if onset is None or onset.shape != chroma.shape:
        # Fall back to chroma-only if onset SSM can't be aligned (shouldn't happen).
        fused = chroma
    else:
        fused = chroma * onset

    novelty = _compute_novelty_curve(fused, NOVELTY_KERNEL_BEATS)
    if novelty.size == 0:
        return NoveltyResult(peaks_seconds=[], peak_strengths=[], confidence=0.0)

    peak_indices, strengths = _pick_peaks(novelty)
    if peak_indices.size == 0:
        return NoveltyResult(peaks_seconds=[], peak_strengths=[], confidence=0.0)

    # Each novelty index corresponds to a beat index (the kernel is centered at the beat).
    # peak_indices is into the novelty array which is indexed by beat. Convert to seconds.
    peaks_seconds = [float(beats[min(i, len(beats) - 1)]) for i in peak_indices]
    peak_strengths = [float(s) for s in strengths]

    # Confidence: how high the strongest peak rises above the median of the whole curve,
    # expressed in MADs, clipped to [0, 1].
    median = float(np.median(novelty))
    mad = float(np.median(np.abs(novelty - median)))
    if mad <= 1e-12:
        confidence = 0.0
    else:
        confidence = float(np.clip((max(peak_strengths) - median) / (mad * 4.0), 0.0, 1.0))

    return NoveltyResult(
        peaks_seconds=peaks_seconds,
        peak_strengths=peak_strengths,
        confidence=confidence,
    )


def _compute_onset_ssm(audio: np.ndarray, sr: int, beats: list[float]) -> np.ndarray | None:
    """Beat-synchronous onset-strength SSM (affinity in [0, 1])."""
    import librosa

    onset_env = librosa.onset.onset_strength(y=audio, sr=sr)
    if onset_env.size == 0:
        return None
    beat_frames = librosa.time_to_frames(np.asarray(beats), sr=sr)
    # librosa.util.sync over beats; aggregate=np.mean keeps onset density per beat.
    onset_sync = librosa.util.sync(onset_env, beat_frames, aggregate=np.mean)
    if onset_sync.ndim == 1:
        onset_sync = onset_sync[np.newaxis, :]
    if onset_sync.shape[1] < 4:
        return None
    # Cosine-similarity affinity matrix.
    if hasattr(librosa.segment, "cross_similarity"):
        sim = librosa.segment.cross_similarity(onset_sync, onset_sync, mode="affinity")
    else:
        sim = librosa.segment.recurrence_matrix(onset_sync, mode="affinity")
    return np.asarray(sim, dtype=np.float64)


def _foote_kernel(size_beats: int) -> np.ndarray:
    """Gaussian-tapered checkerboard kernel of size (size_beats x size_beats)."""
    if size_beats % 2 != 0:
        size_beats += 1
    half = size_beats // 2
    # Checkerboard: +1 in TL/BR quadrants, -1 in TR/BL.
    checker = np.ones((size_beats, size_beats), dtype=np.float64)
    checker[:half, half:] = -1.0
    checker[half:, :half] = -1.0
    # Gaussian taper centered at the middle.
    coords = np.arange(size_beats) - (size_beats - 1) / 2.0
    sigma = size_beats / 4.0
    g = np.exp(-(coords**2) / (2.0 * sigma**2))
    taper = np.outer(g, g)
    kernel = checker * taper
    return kernel


def _compute_novelty_curve(fused_ssm: np.ndarray, kernel_size: int) -> np.ndarray:
    """Slide the Foote kernel along the main diagonal; produce a 1-D novelty curve."""
    n = fused_ssm.shape[0]
    if n < kernel_size:
        return np.zeros(0, dtype=np.float64)
    kernel = _foote_kernel(kernel_size)
    half = kernel_size // 2
    curve = np.zeros(n, dtype=np.float64)
    for i in range(half, n - half):
        block = fused_ssm[i - half : i + half, i - half : i + half]
        curve[i] = float(np.sum(block * kernel))
    return curve


def _pick_peaks(novelty: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Return (peak_indices, peak_strengths) using MAD-based prominence threshold."""
    median = float(np.median(novelty))
    mad = float(np.median(np.abs(novelty - median)))
    if mad <= 1e-12:
        return np.empty(0, dtype=np.int64), np.empty(0, dtype=np.float64)
    prominence = NOVELTY_MIN_PROMINENCE_MAD * mad
    indices, props = find_peaks(
        novelty,
        prominence=prominence,
        distance=NOVELTY_MIN_PEAK_DISTANCE_BEATS,
    )
    if indices.size == 0:
        return np.empty(0, dtype=np.int64), np.empty(0, dtype=np.float64)
    strengths = novelty[indices]
    return indices, strengths
```

- [ ] **Step 4: Run tests to confirm green**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzer
.venv/Scripts/python.exe -m pytest tests/test_detect_novelty.py -v
```

Expected: all five tests pass.

If `test_detect_novelty_drum_drop_detected` is flaky (synthetic onsets are tough), accept the test as long as the chroma+onset fusion picks up the boundary within ±3 s. The test allows that margin.

- [ ] **Step 5: Commit**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine
git add analyzer/src/dj_analyze/detect_novelty.py analyzer/tests/test_detect_novelty.py
git commit -m "feat(analyzer): detect_novelty module (Foote on fused chroma+onset SSM)"
```

---

## Task 4: `detect_cues` — loudness anchor (Component 1)

**Goal:** replace the 10%-of-mean energy threshold with Schwarz's −9 dB-of-max rule. Intro/outro fields stay on the old `±16 bars` heuristic for now; Task 5 swaps those.

**Files:**
- Modify: `analyzer/src/dj_analyze/detect_cues.py`
- Modify: `analyzer/tests/test_detect_cues.py`

- [ ] **Step 1: Replace existing `detect_cues.py` tests with loudness-anchor cases**

The existing tests pin the 10%-of-mean rule. Replace the contents of `analyzer/tests/test_detect_cues.py` with:

```python
import math

import pytest

from dj_analyze.detect_cues import detect_cues


def _downbeats_for(duration_s: float, bpm: float = 120.0) -> list[float]:
    beat_period = 60.0 / bpm
    bar_period = beat_period * 4.0
    n = int(duration_s / bar_period)
    return [bar_period * i for i in range(n)]


def _silent_rms_db_curve(duration_s: float, window_s: float = 2.0) -> list[float]:
    n = max(1, int(duration_s / window_s))
    return [-100.0] * n


def _loud_rms_db_curve(duration_s: float, window_s: float = 2.0, db: float = -6.0) -> list[float]:
    n = max(1, int(duration_s / window_s))
    return [db] * n


def test_detect_cues_silent_track_safe_defaults():
    """All-silent track: cue_in=0, cue_out=duration."""
    dur = 60.0
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(120)],
        downbeats=_downbeats_for(dur),
        energy_curve=[0.0] * 60,
        bpm=120.0,
        novelty_peaks=[],
        novelty_strengths=[],
        novelty_confidence=0.0,
        rms_db_curve=_silent_rms_db_curve(dur),
        max_rms_db=-120.0,
    )
    assert cues["cue_in_seconds"] == 0.0
    assert cues["cue_out_seconds"] == pytest.approx(dur, abs=0.01)


def test_detect_cues_loudness_anchor_silent_intro():
    """10s of silence at start, then loud audio.

    Expected: cue_in snaps up to the first downbeat at or after t=10s.
    """
    dur = 60.0
    rms = [-100.0] * 5 + [-6.0] * 25  # 30 windows of 2s = 60s
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(120)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 60,  # unused but required
        bpm=120.0,
        novelty_peaks=[],
        novelty_strengths=[],
        novelty_confidence=0.0,
        rms_db_curve=rms,
        max_rms_db=-6.0,
    )
    # First loud window starts at index 5 -> t=10s. First downbeat >= 10s at 120 BPM
    # (bar period 2.0s) is t=10.0s exactly.
    assert cues["cue_in_seconds"] == pytest.approx(10.0, abs=2.0)


def test_detect_cues_loudness_anchor_silent_outro():
    """Loud for 50s, then 10s silence at end.

    Expected: cue_out snaps down to the last downbeat at or before the silent boundary.
    """
    dur = 60.0
    rms = [-6.0] * 25 + [-100.0] * 5
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(120)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 60,
        bpm=120.0,
        novelty_peaks=[],
        novelty_strengths=[],
        novelty_confidence=0.0,
        rms_db_curve=rms,
        max_rms_db=-6.0,
    )
    # Last loud window ends at t=50s. Last downbeat <= 50s at 120 BPM is t=50.0s.
    assert cues["cue_out_seconds"] == pytest.approx(50.0, abs=2.0)


def test_detect_cues_loudness_below_threshold_yields_no_match():
    """Whole track is more than 9 dB below max -> falls back to no-loud-window behavior."""
    dur = 30.0
    # Every window is exactly at -20 dB; max is -20 dB. -20 - (-20) = 0 >= -9 -> all windows qualify.
    rms = [-20.0] * 15
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(60)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 30,
        bpm=120.0,
        novelty_peaks=[],
        novelty_strengths=[],
        novelty_confidence=0.0,
        rms_db_curve=rms,
        max_rms_db=-20.0,
    )
    # Uniformly loud: cue_in snaps to first downbeat (t=0), cue_out to last (<= duration).
    assert cues["cue_in_seconds"] == pytest.approx(0.0, abs=0.01)
    assert cues["cue_out_seconds"] <= dur + 0.01
```

- [ ] **Step 2: Run tests to confirm fail**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzer
.venv/Scripts/python.exe -m pytest tests/test_detect_cues.py -v
```

Expected: TypeError or "unexpected keyword argument" — the current `detect_cues` does not accept `novelty_peaks` / `rms_db_curve` / etc.

- [ ] **Step 3: Rewrite `detect_cues.py` with the new signature, loudness anchor only (no novelty yet)**

Replace the contents of `analyzer/src/dj_analyze/detect_cues.py`:

```python
"""Cue point detection from RMS-dB curve, downbeats, and novelty peaks.

Schwarz-derived rules:
  - cue_in/out anchored by -9 dB-of-max loudness floor (2s peak-RMS windows)
  - intro_end/outro_start from Foote novelty peaks, snapped to downbeats
  - long tracks (>6 min) shortened to a structural exit within [5:30, 7:00]
"""

from __future__ import annotations

from typing import Sequence

LOUDNESS_THRESHOLD_DB = -9.0
LOUDNESS_WINDOW_S = 2.0

MIN_INTRO_SECONDS = 8.0
MAX_INTRO_SECONDS = 60.0
MIN_OUTRO_SECONDS = 8.0
MAX_OUTRO_SECONDS = 60.0

NOVELTY_CONFIDENCE_THRESHOLD = 0.25  # below this, fall back to +/- 16 bars

LONG_TRACK_THRESHOLD_S = 360.0
LONG_TRACK_TARGET_MIN_S = 330.0
LONG_TRACK_TARGET_MAX_S = 420.0


def detect_cues(
    duration_seconds: float,
    beats: list[float],
    downbeats: list[float],
    energy_curve: list[float],
    bpm: float,
    novelty_peaks: list[float],
    novelty_strengths: list[float],
    novelty_confidence: float,
    rms_db_curve: list[float],
    max_rms_db: float,
) -> dict[str, float]:
    """Compute cue_in, cue_out, intro_end, outro_start (seconds).

    cue_in/out are anchored by the loudness floor (max - 9 dB) snapped to downbeats.
    intro_end is the first novelty peak after cue_in within [MIN_INTRO, MAX_INTRO]s,
    snapped to the nearest downbeat; falls back to cue_in + 16 bars when no usable peak.
    outro_start is symmetric backwards from cue_out.
    For tracks longer than 6 min, cue_out is moved into [5:30, 7:00] at the strongest
    novelty peak in that window (or the floor 5:30 if none).
    """
    cue_in, cue_out = _loudness_anchor_cues(
        duration_seconds, downbeats, rms_db_curve, max_rms_db
    )

    # intro_end / outro_start computed in Task 5; for now retain old +/- 16 bars heuristic.
    bar_seconds = (60.0 / bpm) * 4.0 if bpm > 0 else 2.0
    intro_end = cue_in + 16.0 * bar_seconds
    outro_start = cue_out - 16.0 * bar_seconds

    # Clamp.
    if intro_end > outro_start:
        midpoint = (cue_in + cue_out) / 2.0
        intro_end = midpoint
        outro_start = midpoint
    intro_end = max(cue_in, min(intro_end, cue_out))
    outro_start = max(cue_in, min(outro_start, cue_out))

    return {
        "cue_in_seconds": float(cue_in),
        "cue_out_seconds": float(cue_out),
        "intro_end_seconds": float(intro_end),
        "outro_start_seconds": float(outro_start),
    }


def _loudness_anchor_cues(
    duration: float,
    downbeats: list[float],
    rms_db_curve: list[float],
    max_rms_db: float,
) -> tuple[float, float]:
    """Return (cue_in, cue_out) anchored to -9 dB-of-max windows, snapped to downbeats."""
    if not rms_db_curve or max_rms_db <= LOUDNESS_THRESHOLD_DB - 50:
        # No usable loudness signal (silent track or decode error).
        return 0.0, duration

    threshold = max_rms_db + LOUDNESS_THRESHOLD_DB
    first_loud_idx = next(
        (i for i, v in enumerate(rms_db_curve) if v >= threshold), None
    )
    last_loud_idx = next(
        (i for i in range(len(rms_db_curve) - 1, -1, -1) if rms_db_curve[i] >= threshold),
        None,
    )

    if first_loud_idx is None or last_loud_idx is None:
        return 0.0, duration

    # Convert window indices to seconds (window starts at i * LOUDNESS_WINDOW_S,
    # ends at (i + 1) * LOUDNESS_WINDOW_S).
    first_loud_t = first_loud_idx * LOUDNESS_WINDOW_S
    last_loud_t = (last_loud_idx + 1) * LOUDNESS_WINDOW_S

    cue_in = _snap_up_to_downbeat(first_loud_t, downbeats, fallback=0.0)
    cue_out = _snap_down_to_downbeat(last_loud_t, downbeats, fallback=duration)
    if cue_out < cue_in:
        # Degenerate: just clamp to whole track.
        return 0.0, duration
    return cue_in, cue_out


def _snap_up_to_downbeat(t: float, downbeats: Sequence[float], fallback: float) -> float:
    """First downbeat >= t. If none, return fallback."""
    for db in downbeats:
        if db >= t:
            return float(db)
    return float(fallback)


def _snap_down_to_downbeat(t: float, downbeats: Sequence[float], fallback: float) -> float:
    """Last downbeat <= t. If none, return fallback."""
    last = None
    for db in downbeats:
        if db <= t:
            last = db
        else:
            break
    return float(last) if last is not None else float(fallback)


def _snap_nearest_downbeat(t: float, downbeats: Sequence[float], fallback: float) -> float:
    """Nearest downbeat (in either direction). If empty, return fallback."""
    if not downbeats:
        return float(fallback)
    return float(min(downbeats, key=lambda db: abs(db - t)))
```

- [ ] **Step 4: Run cue tests to confirm Component 1 cases pass**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzer
.venv/Scripts/python.exe -m pytest tests/test_detect_cues.py -v
```

Expected: all four cases above pass. The `intro_end_seconds` / `outro_start_seconds` are still on the old heuristic — that's fine; Task 5 swaps them.

Note: `analyze.py` won't compile yet because `detect_cues` now takes new kwargs. That's resolved in Task 7. If running the full test suite now, expect `test_analyze.py` failures; ignore them until Task 7.

- [ ] **Step 5: Commit**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine
git add analyzer/src/dj_analyze/detect_cues.py analyzer/tests/test_detect_cues.py
git commit -m "feat(analyzer): loudness-anchor cue_in/cue_out (Schwarz -9 dB)"
```

---

## Task 5: `detect_cues` — structural intro_end / outro_start (Component 2)

**Files:**
- Modify: `analyzer/src/dj_analyze/detect_cues.py`
- Modify: `analyzer/tests/test_detect_cues.py`

- [ ] **Step 1: Add failing tests for novelty-driven intro_end / outro_start**

Append to `analyzer/tests/test_detect_cues.py`:

```python
def test_intro_end_uses_novelty_peak():
    """A novelty peak at 24s after cue_in -> intro_end snaps to nearest downbeat near 24s."""
    dur = 200.0
    novelty_peaks = [24.0, 150.0]
    novelty_strengths = [0.8, 0.6]
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(400)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 200,
        bpm=120.0,
        novelty_peaks=novelty_peaks,
        novelty_strengths=novelty_strengths,
        novelty_confidence=0.9,
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    # intro_end should be within 1 bar (2s at 120 BPM) of 24s.
    assert abs(cues["intro_end_seconds"] - 24.0) <= 2.0


def test_outro_start_uses_last_novelty_peak():
    """A novelty peak 30s before cue_out -> outro_start snaps to it."""
    dur = 200.0
    novelty_peaks = [30.0, 170.0]
    novelty_strengths = [0.5, 0.8]
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(400)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 200,
        bpm=120.0,
        novelty_peaks=novelty_peaks,
        novelty_strengths=novelty_strengths,
        novelty_confidence=0.9,
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    # Last loud window centers near 200s; outro_start should snap to ~170s.
    assert abs(cues["outro_start_seconds"] - 170.0) <= 2.0


def test_intro_end_falls_back_when_no_peak_in_window():
    """No peak in [cue_in+8, cue_in+60] -> fall back to cue_in + 16 bars."""
    dur = 200.0
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(400)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 200,
        bpm=120.0,
        novelty_peaks=[5.0, 180.0],  # both outside the [8, 60]s window
        novelty_strengths=[0.8, 0.8],
        novelty_confidence=0.9,
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    # 16 bars at 120 BPM = 32s. cue_in is 0 here, so intro_end ~= 32s.
    assert abs(cues["intro_end_seconds"] - 32.0) <= 2.0


def test_intro_end_falls_back_when_confidence_low():
    """Peaks exist but confidence is below threshold -> fall back to +16 bars."""
    dur = 200.0
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(400)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 200,
        bpm=120.0,
        novelty_peaks=[24.0],
        novelty_strengths=[0.1],
        novelty_confidence=0.1,  # below NOVELTY_CONFIDENCE_THRESHOLD
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    assert abs(cues["intro_end_seconds"] - 32.0) <= 2.0  # fallback


def test_intro_end_respects_min_guard():
    """Peak at 4s (below MIN_INTRO=8) is ignored."""
    dur = 200.0
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(400)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 200,
        bpm=120.0,
        novelty_peaks=[4.0, 40.0],
        novelty_strengths=[0.8, 0.8],
        novelty_confidence=0.9,
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    # First peak 4s ignored (under MIN_INTRO=8). 40s peak used.
    assert abs(cues["intro_end_seconds"] - 40.0) <= 2.0


def test_intro_end_respects_max_guard():
    """Peak at 90s (above MAX_INTRO=60) is ignored; falls back."""
    dur = 200.0
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(400)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 200,
        bpm=120.0,
        novelty_peaks=[90.0],
        novelty_strengths=[0.8],
        novelty_confidence=0.9,
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    # 90s above MAX_INTRO; fall back to +16 bars (~32s).
    assert abs(cues["intro_end_seconds"] - 32.0) <= 2.0
```

- [ ] **Step 2: Run to confirm new tests fail**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzer
.venv/Scripts/python.exe -m pytest tests/test_detect_cues.py -v
```

Expected: the six new tests fail (intro_end still on `+16 bars` heuristic regardless of novelty input).

- [ ] **Step 3: Replace the intro_end / outro_start computation in `detect_cues.py`**

In `analyzer/src/dj_analyze/detect_cues.py`, replace the body of `detect_cues` from the `# intro_end / outro_start ...` comment through the final return statement with:

```python
    # intro_end / outro_start: prefer novelty peaks; fall back to +/- 16 bars.
    bar_seconds = (60.0 / bpm) * 4.0 if bpm > 0 else 2.0
    intro_end = _structural_intro_end(
        cue_in=cue_in,
        cue_out=cue_out,
        bar_seconds=bar_seconds,
        novelty_peaks=novelty_peaks,
        novelty_confidence=novelty_confidence,
        downbeats=downbeats,
    )
    outro_start = _structural_outro_start(
        cue_in=cue_in,
        cue_out=cue_out,
        bar_seconds=bar_seconds,
        novelty_peaks=novelty_peaks,
        novelty_strengths=novelty_strengths,
        novelty_confidence=novelty_confidence,
        downbeats=downbeats,
    )

    if intro_end > outro_start:
        midpoint = (cue_in + cue_out) / 2.0
        intro_end = midpoint
        outro_start = midpoint
    intro_end = max(cue_in, min(intro_end, cue_out))
    outro_start = max(cue_in, min(outro_start, cue_out))

    return {
        "cue_in_seconds": float(cue_in),
        "cue_out_seconds": float(cue_out),
        "intro_end_seconds": float(intro_end),
        "outro_start_seconds": float(outro_start),
    }
```

Then add the two helper functions at the end of the module:

```python
def _structural_intro_end(
    *,
    cue_in: float,
    cue_out: float,
    bar_seconds: float,
    novelty_peaks: list[float],
    novelty_confidence: float,
    downbeats: Sequence[float],
) -> float:
    """First novelty peak in [cue_in + MIN_INTRO, cue_in + MAX_INTRO], snapped to downbeat.
    Falls back to cue_in + 16 bars when no peak in window or confidence too low.
    """
    fallback = cue_in + 16.0 * bar_seconds
    if novelty_confidence < NOVELTY_CONFIDENCE_THRESHOLD:
        return fallback
    lo = cue_in + MIN_INTRO_SECONDS
    hi = min(cue_in + MAX_INTRO_SECONDS, cue_out)
    in_window = [p for p in novelty_peaks if lo <= p <= hi]
    if not in_window:
        return fallback
    chosen = min(in_window)  # earliest peak in window
    return _snap_nearest_downbeat(chosen, downbeats, fallback=chosen)


def _structural_outro_start(
    *,
    cue_in: float,
    cue_out: float,
    bar_seconds: float,
    novelty_peaks: list[float],
    novelty_strengths: list[float],
    novelty_confidence: float,
    downbeats: Sequence[float],
) -> float:
    """Last novelty peak in [cue_out - MAX_OUTRO, cue_out - MIN_OUTRO], snapped to downbeat.
    Falls back to cue_out - 16 bars when no peak in window or confidence too low.
    """
    fallback = cue_out - 16.0 * bar_seconds
    if novelty_confidence < NOVELTY_CONFIDENCE_THRESHOLD:
        return fallback
    lo = max(cue_out - MAX_OUTRO_SECONDS, cue_in)
    hi = cue_out - MIN_OUTRO_SECONDS
    in_window = [p for p in novelty_peaks if lo <= p <= hi]
    if not in_window:
        return fallback
    chosen = max(in_window)  # latest peak in window
    return _snap_nearest_downbeat(chosen, downbeats, fallback=chosen)
```

- [ ] **Step 4: Run cue tests to confirm green**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzer
.venv/Scripts/python.exe -m pytest tests/test_detect_cues.py -v
```

Expected: all cue tests pass (loudness-anchor cases from Task 4 + the six new structural cases).

- [ ] **Step 5: Commit**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine
git add analyzer/src/dj_analyze/detect_cues.py analyzer/tests/test_detect_cues.py
git commit -m "feat(analyzer): novelty-driven intro_end/outro_start with fallback"
```

---

## Task 6: `detect_cues` — long-track shortening (Component 3)

**Files:**
- Modify: `analyzer/src/dj_analyze/detect_cues.py`
- Modify: `analyzer/tests/test_detect_cues.py`

- [ ] **Step 1: Add failing tests for long-track behavior**

Append to `analyzer/tests/test_detect_cues.py`:

```python
def test_short_track_unaffected_by_long_rule():
    """200s track: long-track rule must not fire."""
    dur = 200.0
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(400)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 200,
        bpm=120.0,
        novelty_peaks=[40.0, 160.0],
        novelty_strengths=[0.7, 0.7],
        novelty_confidence=0.9,
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    # cue_out from loudness anchor: last loud window <= 200 -> cue_out close to 200s.
    assert cues["cue_out_seconds"] >= 180.0


def test_long_track_shortened_to_strongest_peak_in_window():
    """480s track with a strong peak at 380s -> cue_out snaps to 380s."""
    dur = 480.0
    novelty_peaks = [60.0, 380.0, 460.0]
    novelty_strengths = [0.5, 0.9, 0.5]
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(960)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 480,
        bpm=120.0,
        novelty_peaks=novelty_peaks,
        novelty_strengths=novelty_strengths,
        novelty_confidence=0.9,
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    # Strongest peak in [330, 420] is 380s (strength 0.9).
    assert abs(cues["cue_out_seconds"] - 380.0) <= 2.0


def test_long_track_no_peak_in_window_falls_to_floor():
    """480s track with no peaks in [330, 420] -> cue_out = 330s, snapped."""
    dur = 480.0
    novelty_peaks = [60.0, 460.0]  # neither in [330, 420]
    novelty_strengths = [0.5, 0.5]
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(960)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 480,
        bpm=120.0,
        novelty_peaks=novelty_peaks,
        novelty_strengths=novelty_strengths,
        novelty_confidence=0.9,
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    assert abs(cues["cue_out_seconds"] - 330.0) <= 2.0


def test_long_track_outro_start_recomputed_against_new_cue_out():
    """After cue_out moves, outro_start must be recomputed against the new cue_out."""
    dur = 480.0
    # Peak at 360s (used as cue_out) and 340s (would be in outro window of new cue_out).
    novelty_peaks = [60.0, 340.0, 360.0]
    novelty_strengths = [0.5, 0.7, 0.9]
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(960)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 480,
        bpm=120.0,
        novelty_peaks=novelty_peaks,
        novelty_strengths=novelty_strengths,
        novelty_confidence=0.9,
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    # cue_out = 360. outro_start window = [360-60, 360-8] = [300, 352].
    # 340 is in that window; expect outro_start ~= 340.
    assert abs(cues["cue_out_seconds"] - 360.0) <= 2.0
    assert abs(cues["outro_start_seconds"] - 340.0) <= 2.0
```

- [ ] **Step 2: Run to confirm fail**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzer
.venv/Scripts/python.exe -m pytest tests/test_detect_cues.py::test_long_track_shortened_to_strongest_peak_in_window tests/test_detect_cues.py::test_long_track_no_peak_in_window_falls_to_floor tests/test_detect_cues.py::test_long_track_outro_start_recomputed_against_new_cue_out -v
```

Expected: the long-track tests fail (no shortening logic yet). The `test_short_track_unaffected_by_long_rule` test should already pass.

- [ ] **Step 3: Add `_shorten_if_long_track` and wire it into `detect_cues`**

In `analyzer/src/dj_analyze/detect_cues.py`, modify `detect_cues` to insert the long-track step between loudness-anchor and intro/outro computation. Replace the body so the order is:

```python
    cue_in, cue_out = _loudness_anchor_cues(
        duration_seconds, downbeats, rms_db_curve, max_rms_db
    )

    # Long-track shortening (overrides cue_out before intro/outro are computed).
    cue_out = _shorten_if_long_track(
        cue_out=cue_out,
        cue_in=cue_in,
        duration=duration_seconds,
        novelty_peaks=novelty_peaks,
        novelty_strengths=novelty_strengths,
        downbeats=downbeats,
    )

    bar_seconds = (60.0 / bpm) * 4.0 if bpm > 0 else 2.0
    intro_end = _structural_intro_end(
        cue_in=cue_in,
        cue_out=cue_out,
        bar_seconds=bar_seconds,
        novelty_peaks=novelty_peaks,
        novelty_confidence=novelty_confidence,
        downbeats=downbeats,
    )
    outro_start = _structural_outro_start(
        cue_in=cue_in,
        cue_out=cue_out,
        bar_seconds=bar_seconds,
        novelty_peaks=novelty_peaks,
        novelty_strengths=novelty_strengths,
        novelty_confidence=novelty_confidence,
        downbeats=downbeats,
    )

    if intro_end > outro_start:
        midpoint = (cue_in + cue_out) / 2.0
        intro_end = midpoint
        outro_start = midpoint
    intro_end = max(cue_in, min(intro_end, cue_out))
    outro_start = max(cue_in, min(outro_start, cue_out))

    return {
        "cue_in_seconds": float(cue_in),
        "cue_out_seconds": float(cue_out),
        "intro_end_seconds": float(intro_end),
        "outro_start_seconds": float(outro_start),
    }
```

Then append the helper at the end of the module:

```python
def _shorten_if_long_track(
    *,
    cue_out: float,
    cue_in: float,
    duration: float,
    novelty_peaks: list[float],
    novelty_strengths: list[float],
    downbeats: Sequence[float],
) -> float:
    """For tracks > 6 min, force cue_out into [5:30, 7:00] using the strongest peak
    in that window; floor at 5:30 if no peak is available.
    """
    if duration <= LONG_TRACK_THRESHOLD_S:
        return cue_out
    lo = LONG_TRACK_TARGET_MIN_S
    hi = LONG_TRACK_TARGET_MAX_S
    in_window = [
        (p, s) for p, s in zip(novelty_peaks, novelty_strengths) if lo <= p <= hi
    ]
    if in_window:
        chosen = max(in_window, key=lambda ps: ps[1])[0]
    else:
        chosen = lo
    snapped = _snap_down_to_downbeat(chosen, downbeats, fallback=chosen)
    return min(snapped, cue_out, duration) if snapped >= cue_in else cue_out
```

- [ ] **Step 4: Run all cue tests**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzer
.venv/Scripts/python.exe -m pytest tests/test_detect_cues.py -v
```

Expected: all tests pass (Task 4 + Task 5 + Task 6 cases).

- [ ] **Step 5: Commit**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine
git add analyzer/src/dj_analyze/detect_cues.py analyzer/tests/test_detect_cues.py
git commit -m "feat(analyzer): long-track shortening (Schwarz 6-min rule)"
```

---

## Task 7: `analyze.py` orchestration — wire SSM, novelty, RMS-dB through

**Files:**
- Modify: `analyzer/src/dj_analyze/analyze.py`
- Modify: `analyzer/tests/test_analyze.py`

- [ ] **Step 1: Add a failing test verifying the new orchestration**

Append to `analyzer/tests/test_analyze.py` (existing tests should still work — they only check the output dict shape):

```python
def test_analyze_emits_cue_fields_consistent_with_novelty():
    """End-to-end orchestration: cue fields are computed by the new code path
    (loudness anchor + novelty) and present in the output dict.
    """
    # Use whatever test fixture path the existing tests use (a small WAV under
    # analyzer/tests/fixtures/). The exact path varies; the assertion below is
    # general and works against any short fixture.
    from pathlib import Path

    from dj_analyze.analyze import analyze_track

    fixture_dir = Path(__file__).parent / "fixtures"
    candidates = sorted(fixture_dir.glob("*.wav"))
    if not candidates:
        import pytest
        pytest.skip("No WAV fixture available for end-to-end check.")
    out = analyze_track(candidates[0])
    # No error from decode.
    assert "error" not in out
    # All four cue fields are present, ordered: cue_in <= intro_end <= outro_start <= cue_out.
    cue_in = out["cue_in_seconds"]
    intro_end = out["intro_end_seconds"]
    outro_start = out["outro_start_seconds"]
    cue_out = out["cue_out_seconds"]
    assert cue_in <= intro_end <= outro_start + 0.01  # tolerate equality at the midpoint clamp
    assert outro_start <= cue_out + 0.01
```

- [ ] **Step 2: Run to confirm fail**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzer
.venv/Scripts/python.exe -m pytest tests/test_analyze.py -v
```

Expected: `test_analyze.py` raises an error from `detect_cues` missing required kwargs (caller still passes the old args).

- [ ] **Step 3: Update `analyze.py` to compute SSM once and pass novelty + RMS-dB through**

Replace the body of `analyze_track` in `analyzer/src/dj_analyze/analyze.py` with:

```python
def analyze_track(path: Path) -> dict[str, Any]:
    """Analyze one audio file. Returns a sidecar-shaped dict (schema v2)."""
    stat = path.stat()
    base: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "analyzer_version": __version__,
        "source_path": str(path.resolve()),
        "source_mtime": stat.st_mtime,
        "source_size_bytes": stat.st_size,
    }

    try:
        audio, sr = decode_audio(path)
    except DecodeError as e:
        return {**base, "error": str(e)}

    duration = len(audio) / sr
    bpm, bpm_conf = detect_bpm(audio, sr)
    beats = detect_beats(audio, sr, bpm=bpm)
    db_result = detect_downbeats(audio, sr, beats=beats)

    # Build the chroma SSM ONCE; both detect_phrases and detect_novelty reuse it.
    chroma_ssm = compute_chroma_ssm(audio, sr, beats) if beats else None

    phrase_result = (
        detect_phrases(audio, sr, beats=beats, output_stride_bars=16, chroma_ssm=chroma_ssm)
        if beats
        else None
    )

    novelty = (
        detect_novelty(audio, sr, beats=beats, chroma_ssm=chroma_ssm)
        if beats
        else None
    )

    key_result = detect_key(audio, sr)
    energy = detect_energy(audio, sr)
    bands = detect_band_energies(audio, sr)

    cues = detect_cues(
        duration_seconds=duration,
        beats=beats,
        downbeats=db_result.downbeats,
        energy_curve=energy.curve,
        bpm=bpm,
        novelty_peaks=novelty.peaks_seconds if novelty else [],
        novelty_strengths=novelty.peak_strengths if novelty else [],
        novelty_confidence=novelty.confidence if novelty else 0.0,
        rms_db_curve=energy.rms_db_curve,
        max_rms_db=energy.max_rms_db,
    )

    mix = classify(
        bpm=bpm,
        bpm_confidence=bpm_conf,
        duration_seconds=duration,
        downbeats_count=len(db_result.downbeats),
        beats_count=len(beats),
        downbeat_method=db_result.method,
        key_confidence=key_result.confidence,
    )

    out: dict[str, Any] = {
        **base,
        "duration_seconds": float(duration),
        "sample_rate": int(sr),
        "bpm": float(bpm),
        "bpm_confidence": float(bpm_conf),
        "beats_seconds": beats,
        "downbeats_seconds": db_result.downbeats,
        "key": key_result.camelot,
        "key_label": key_result.label,
        "key_confidence": float(key_result.confidence),
        "energy_curve": energy.curve,
        "energy_mean": float(energy.mean),
        "band_energy_curves": bands["curves"],
        "band_energy_means": bands["means"],
        **cues,
        "is_beatmatchable": mix["is_beatmatchable"],
        "is_phrase_aligned": mix["is_phrase_aligned"],
        "is_mixable": mix["is_mixable"],
        "mixable_reasons": mix["reasons"],
    }

    if phrase_result is not None:
        out["phrase_confidence"] = float(phrase_result.confidence)
        if mix["is_phrase_aligned"] and phrase_result.boundaries:
            out["phrase_boundaries_seconds"] = phrase_result.boundaries

    return out
```

Then update the imports at the top of `analyze.py`:

```python
from . import __version__
from .decode import DecodeError, decode_audio
from .detect_bands import detect_band_energies
from .detect_beats import detect_beats
from .detect_bpm import detect_bpm
from .detect_cues import detect_cues
from .detect_downbeats import compute_chroma_ssm, detect_downbeats, detect_phrases
from .detect_energy import detect_energy
from .detect_key import detect_key
from .detect_novelty import detect_novelty
from .mixability import classify
```

- [ ] **Step 4: Run the full test suite to confirm everything still green**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzer
.venv/Scripts/python.exe -m pytest -v
```

Expected: all analyzer tests pass.

- [ ] **Step 5: Commit**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine
git add analyzer/src/dj_analyze/analyze.py analyzer/tests/test_analyze.py
git commit -m "feat(analyzer): wire SSM + novelty + RMS-dB through analyze_track"
```

---

## Task 8: Version bump + end-to-end smoke

**Files:**
- Modify: `analyzer/src/dj_analyze/__init__.py`

- [ ] **Step 1: Bump analyzer version**

Edit `analyzer/src/dj_analyze/__init__.py`:

```python
"""DJ Engine analyzer — emits sidecar JSONs from a directory of audio files."""

__version__ = "0.3.0"
```

- [ ] **Step 2: Run all analyzer tests once more**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzer
.venv/Scripts/python.exe -m pytest -v
```

Expected: all green.

- [ ] **Step 3: Run schema validation to confirm sidecars still match v2**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine/analyzer
.venv/Scripts/python.exe -m pytest tests/test_schema_validation.py -v
```

Expected: pass. No schema field added or removed; only values change.

- [ ] **Step 4: Re-analyze the user's library**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine
analyzer/.venv/Scripts/python.exe -m dj_analyze data/ analyzed-real/ --workers 2
```

Expected: all 24 tracks re-processed (the version bump invalidates all existing sidecars). Wall-clock ~4–5 minutes. 0 errors.

- [ ] **Step 5: Spot-check the new cue values**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine
analyzer/.venv/Scripts/python.exe -c "import json,glob; sc=[json.load(open(p,encoding='utf-8')) for p in sorted(glob.glob('analyzed-real/*.json'))[:24]]; [print(f\"{i+1:02} dur={s.get('duration_seconds',0):.0f}s cue_in={s.get('cue_in_seconds',0):.1f} intro_end={s.get('intro_end_seconds',0):.1f} outro_start={s.get('outro_start_seconds',0):.1f} cue_out={s.get('cue_out_seconds',0):.1f}\") for i,s in enumerate(sc)]"
```

Compare against the pre-change values (recorded during plan review):

| Track | Old intro_end | New intro_end (expected) |
|---|---|---|
| 02 (long quiet intro) | 60.4 | likely differs — anchored to a real drop |
| 03 (long quiet intro) | 68.0 | likely differs |
| 16 (32s intro) | 57.2 | likely differs |
| Others | various | most still close to `cue_in + 16 bars` (fallback path) when novelty confidence is low |

Sanity check: every track satisfies `cue_in <= intro_end <= outro_start <= cue_out`.

- [ ] **Step 6: Listening smoke**

Manual step (no automation). Start the engine; listen for 15 minutes via VLC at `http://localhost:27897/dj.mp3`. Listen specifically for:

- Incoming tracks entering at musically meaningful points (on a drop or chorus, not mid-build).
- Cue-in no longer landing on inaudible noise floor on tracks with quiet intros.
- No regressions in transition smoothness (the engine code is unchanged; transitions should be at least as good as before).

If any track sounds wrong, capture which one, the timestamp, and the `intro_end` / `outro_start` from its sidecar. The knobs at the top of `detect_cues.py` and `detect_novelty.py` are the tuning surface.

- [ ] **Step 7: Commit version bump**

```bash
cd C:/Users/f.nefzi/Desktop/Personal/DJ-Engine
git add analyzer/src/dj_analyze/__init__.py
git commit -m "chore(analyzer): bump version 0.2.0 -> 0.3.0 (invalidates sidecars)"
```

---

## Out

After Task 8, the analyzer emits Schwarz-derived cue points consumed unchanged by the engine. Subjective listening determines whether default thresholds (−9 dB, 16-beat kernel, 8/60 s guards, 0.25 confidence) need tuning before the change merges to main.
