# Madmom Analyzer Enhancement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace librosa's fixed-tempo beat tracker with madmom (RNN+DBN), expose per-bar local BPM and per-beat/downbeat confidences in the sidecar, and refactor the engine to compute time-stretch ratios from the actual transition window's local BPM with global-BPM fallback.

**Architecture:** A single new module `detect_beat_grid.py` centralizes the madmom call; `detect_beats.py` and `detect_downbeats.py` become thin wrappers; `analyze.py` calls madmom once and derives per-bar local BPM from non-uniform beats. Engine adds three optional sidecar fields, a `local_bpm_in_window` helper, and a refactored `TransitionContext` that precomputes the ratio so the ~14 recipes consume `ctx.ratio` instead of recomputing from globals. The scheduler logs both old and new ratios at arming for observability under audio playout delay.

**Tech Stack:** Python 3.10 analyzer (madmom 0.16.1, numpy<2, cython 0.29.36, librosa for non-beat detection); Rust 2021 engine (serde, tracing, tokio); pytest; cargo test.

**Spec:** `docs/superpowers/specs/2026-05-29-madmom-analyzer-enhancement-design.md`

---

## Task 1: Install madmom in the analyzer venv and smoke-test it

**Files:**
- Modify: `analyzer/pyproject.toml`
- No code yet — environmental setup + manual smoke

This task resolves the spec's "Open Items" point about pin combos. If install fails, the whole plan stalls; do this first.

- [ ] **Step 1: Activate the analyzer venv**

The analyzer venv lives at `analyzer/.venv` (Python 3.10). Activate:
```powershell
& analyzer\.venv\Scripts\Activate.ps1
python --version  # expect: Python 3.10.x
```

- [ ] **Step 2: Pin numpy and install cython**

```powershell
pip install "numpy>=1.26,<2.0" "cython==0.29.36"
```
Expected: clean install, no errors.

- [ ] **Step 3: Install madmom with --no-build-isolation**

```powershell
pip install "madmom>=0.16.1" --no-build-isolation
```
Expected: builds the Cython extension, finishes without errors. On Windows, requires MSVC Build Tools (already present via torch/demucs path).

If this step fails with Cython errors, try the github master branch as a fallback:
```powershell
pip install git+https://github.com/CPJKU/madmom.git --no-build-isolation
```

- [ ] **Step 4: Verify madmom imports and processes audio**

```powershell
python -c "import madmom; from madmom.features.downbeats import RNNDownBeatProcessor; from madmom.features.beats import DBNDownBeatTrackingProcessor; print('madmom version:', madmom.__version__)"
```
Expected: prints `madmom version: 0.16.1` (or similar). No import errors.

Then smoke-test against a fixture:
```powershell
python -c "
import numpy as np
from madmom.features.downbeats import RNNDownBeatProcessor
from madmom.features.beats import DBNDownBeatTrackingProcessor
from dj_analyze.decode import decode_audio
audio, sr = decode_audio('analyzer/tests/fixtures/click_120bpm.wav')
rnn = RNNDownBeatProcessor()
dbn = DBNDownBeatTrackingProcessor(beats_per_bar=[3, 4], fps=100)
acts = rnn(audio.astype(np.float32))
out = dbn(acts)
print(f'shape: {acts.shape}, beats found: {len(out)}')
print(f'first 5 beats: {out[:5]}')
"
```
Expected: nonzero beats array, activations shape `(N, 2)`, first beats clustering near multiples of 0.5s (120 BPM click track).

- [ ] **Step 5: Update pyproject.toml**

Edit `analyzer/pyproject.toml`. Re-pin numpy and add madmom + cython.

Replace the `numpy>=1.26,` line and the commented-out madmom line:
```toml
dependencies = [
    "numpy>=1.26,<2.0",
    "librosa>=0.10.2",
    "soundfile>=0.12.1",
    "audioread>=3.0.1",
    "rich>=13.7.0",
    "demucs>=4.0",
    "madmom>=0.16.1",
]

[build-system]
requires = ["hatchling", "cython==0.29.36"]
build-backend = "hatchling.build"
```

(Remove the disabled-essentia and disabled-madmom comments at the end of the `dependencies` block; they're now obsolete history.)

- [ ] **Step 6: Verify `pip install -e .` still works**

```powershell
pip install -e analyzer
```
Expected: editable install completes without errors. Madmom is already installed; this just re-verifies the pyproject is consistent.

- [ ] **Step 7: Check the dj-analyze CLI behavior on analyzer_version mismatch**

```powershell
dj-analyze --help
```
Look for a `--force` or similar flag. Document the actual flag name as a comment in the plan if it diverges from `--force`. If it doesn't exist, plan to either (a) delete `analyzed-real/*.json` before re-analysis or (b) add a `--force` flag in a later task.

- [ ] **Step 8: Commit pyproject changes**

```powershell
git add analyzer/pyproject.toml
git commit -m "build(analyzer): hard-require madmom 0.16.1; pin numpy<2 and cython 0.29.36"
```

---

## Task 2: Create `detect_beat_grid.py` with full TDD

**Files:**
- Create: `analyzer/src/dj_analyze/detect_beat_grid.py`
- Create: `analyzer/tests/test_detect_beat_grid.py`

- [ ] **Step 1: Write the failing test file**

Create `analyzer/tests/test_detect_beat_grid.py`:
```python
from __future__ import annotations

from pathlib import Path

import numpy as np
import pytest

from dj_analyze.decode import decode_audio
from dj_analyze.detect_beat_grid import BeatGrid, detect_beat_grid

FIXTURES = Path(__file__).parent / "fixtures"


def test_detect_beat_grid_returns_populated_grid_on_click_fixture():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    grid = detect_beat_grid(audio, sr)
    assert isinstance(grid, BeatGrid)
    assert len(grid.beats) > 0
    assert len(grid.downbeats) > 0
    # downbeats are a subset of beats (by time, within numerical tolerance).
    for db in grid.downbeats:
        assert any(abs(db - b) < 1e-3 for b in grid.beats)


def test_detect_beat_grid_length_invariants():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    grid = detect_beat_grid(audio, sr)
    assert len(grid.beat_confidence) == len(grid.beats)
    assert len(grid.downbeat_confidence) == len(grid.downbeats)


def test_detect_beat_grid_confidence_in_unit_interval():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    grid = detect_beat_grid(audio, sr)
    for c in grid.beat_confidence:
        assert 0.0 <= c <= 1.0, f"beat confidence out of range: {c}"
    for c in grid.downbeat_confidence:
        assert 0.0 <= c <= 1.0, f"downbeat confidence out of range: {c}"


def test_detect_beat_grid_beats_are_sorted():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    grid = detect_beat_grid(audio, sr)
    for a, b in zip(grid.beats, grid.beats[1:]):
        assert a < b


def test_detect_beat_grid_handles_too_short_audio():
    """Audio under one second should not crash; may return empty arrays."""
    rng = np.random.default_rng(seed=42)
    audio = rng.uniform(-0.1, 0.1, size=22050).astype(np.float32)  # 0.5s
    grid = detect_beat_grid(audio, sr=44100)
    assert isinstance(grid, BeatGrid)
    # Don't assert non-empty — madmom may legitimately find nothing in noise.
    assert len(grid.beat_confidence) == len(grid.beats)
    assert len(grid.downbeat_confidence) == len(grid.downbeats)
```

- [ ] **Step 2: Run the test to see it fail**

```powershell
pytest analyzer/tests/test_detect_beat_grid.py -v
```
Expected: FAIL with `ModuleNotFoundError: No module named 'dj_analyze.detect_beat_grid'`.

- [ ] **Step 3: Create the implementation**

Create `analyzer/src/dj_analyze/detect_beat_grid.py`:
```python
"""Single madmom RNN+DBN pass producing beats, downbeats, and per-event confidences.

This module is the foundation for detect_beats and detect_downbeats; both call into
detect_beat_grid and select fields out of the shared result. Confidences are the
RNN activation values (per beat / per downbeat axis) sampled at each detected time.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

# Frame rate of the madmom RNN output. Used to convert detected times to indices
# into the activation array when sampling confidences.
_RNN_FPS = 100


@dataclass(frozen=True, slots=True)
class BeatGrid:
    beats: list[float]
    beat_confidence: list[float]
    downbeats: list[float]
    downbeat_confidence: list[float]


def detect_beat_grid(audio: np.ndarray, sr: int) -> BeatGrid:
    """Detect beats and downbeats with confidences via madmom RNN+DBN.

    Returns a BeatGrid where:
      - beats: all detected beat times (non-uniform; snapped to onsets)
      - downbeats: subset of beats marked as position 1 by the DBN
      - beat_confidence: RNN beat-axis activation sampled at each beat time, [0, 1]
      - downbeat_confidence: RNN downbeat-axis activation sampled at each downbeat, [0, 1]
    """
    from madmom.features.beats import DBNDownBeatTrackingProcessor
    from madmom.features.downbeats import RNNDownBeatProcessor

    rnn = RNNDownBeatProcessor()
    dbn = DBNDownBeatTrackingProcessor(beats_per_bar=[3, 4], fps=_RNN_FPS)
    activations = rnn(audio.astype(np.float32))

    if activations.size == 0:
        return BeatGrid([], [], [], [])

    beats_and_positions = dbn(activations)
    if len(beats_and_positions) == 0:
        return BeatGrid([], [], [], [])

    beat_act = activations[:, 0]
    downbeat_act = activations[:, 1]

    def sample_at(arr: np.ndarray, t: float) -> float:
        idx = int(round(t * _RNN_FPS))
        idx = max(0, min(idx, len(arr) - 1))
        return float(arr[idx])

    beats: list[float] = []
    beat_confidence: list[float] = []
    downbeats: list[float] = []
    downbeat_confidence: list[float] = []

    for t, pos in beats_and_positions:
        t_f = float(t)
        beats.append(t_f)
        beat_confidence.append(sample_at(beat_act, t_f))
        if int(round(pos)) == 1:
            downbeats.append(t_f)
            downbeat_confidence.append(sample_at(downbeat_act, t_f))

    return BeatGrid(
        beats=beats,
        beat_confidence=beat_confidence,
        downbeats=downbeats,
        downbeat_confidence=downbeat_confidence,
    )
```

- [ ] **Step 4: Run the test to verify it passes**

```powershell
pytest analyzer/tests/test_detect_beat_grid.py -v
```
Expected: all 5 tests PASS. (May take ~10–30s on first run as madmom loads model weights.)

- [ ] **Step 5: Commit**

```powershell
git add analyzer/src/dj_analyze/detect_beat_grid.py analyzer/tests/test_detect_beat_grid.py
git commit -m "feat(analyzer): detect_beat_grid module with madmom RNN+DBN single pass"
```

---

## Task 3: Refactor `detect_beats.py` to use `detect_beat_grid`

**Files:**
- Modify: `analyzer/src/dj_analyze/detect_beats.py`
- Modify: `analyzer/tests/test_detect_beats.py`

- [ ] **Step 1: Inspect existing test expectations**

```powershell
Get-Content analyzer/tests/test_detect_beats.py
```
Note which tests depend on librosa-specific behavior or the `bpm` parameter; these get rewritten.

- [ ] **Step 2: Rewrite the test file to assert madmom-derived outputs**

Replace the entire contents of `analyzer/tests/test_detect_beats.py` with:
```python
from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_beats import detect_beats

FIXTURES = Path(__file__).parent / "fixtures"


def test_detect_beats_returns_sorted_floats():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = detect_beats(audio, sr)
    assert isinstance(beats, list)
    assert all(isinstance(b, float) for b in beats)
    assert all(a < b for a, b in zip(beats, beats[1:]))


def test_detect_beats_finds_at_least_8_beats_on_click_fixture():
    """A 120 BPM click fixture of reasonable length should yield ≥8 beats."""
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = detect_beats(audio, sr)
    assert len(beats) >= 8, f"expected ≥8 beats on click track, got {len(beats)}"
```

- [ ] **Step 3: Run tests to see them fail**

```powershell
pytest analyzer/tests/test_detect_beats.py -v
```
Expected: FAIL — `detect_beats` still has the old `(audio, sr, bpm)` signature, or the librosa import differs.

- [ ] **Step 4: Rewrite `detect_beats.py`**

Replace entire contents of `analyzer/src/dj_analyze/detect_beats.py`:
```python
"""Beat-time detection. Thin wrapper over detect_beat_grid (madmom RNN+DBN)."""

from __future__ import annotations

import numpy as np

from .detect_beat_grid import detect_beat_grid


def detect_beats(audio: np.ndarray, sr: int) -> list[float]:
    """Return madmom-detected beat times in seconds. Non-uniform: snapped to onsets."""
    return detect_beat_grid(audio, sr).beats
```

- [ ] **Step 5: Verify tests pass**

```powershell
pytest analyzer/tests/test_detect_beats.py -v
```
Expected: both tests PASS.

- [ ] **Step 6: Run the broader test suite to find what else broke**

```powershell
pytest analyzer/tests/ -x --tb=short
```
Expect failures in `test_analyze.py` (if it exists) or any test that calls `detect_beats(audio, sr, bpm=...)`. Note them; we fix them in their respective tasks.

If `analyze.py` is the only caller and it provides `bpm=`, that's a known fix in Task 7. Don't fix here.

- [ ] **Step 7: Commit**

```powershell
git add analyzer/src/dj_analyze/detect_beats.py analyzer/tests/test_detect_beats.py
git commit -m "refactor(analyzer): detect_beats becomes thin wrapper over detect_beat_grid"
```

---

## Task 4: Refactor `detect_downbeats.py` — keep `detect_phrases`, drop downbeat fallbacks

**Files:**
- Modify: `analyzer/src/dj_analyze/detect_downbeats.py`
- Modify: `analyzer/tests/test_detect_downbeats.py`

- [ ] **Step 1: Rewrite test file**

Replace entire contents of `analyzer/tests/test_detect_downbeats.py`:
```python
from __future__ import annotations

from pathlib import Path

import numpy as np

from dj_analyze.decode import decode_audio
from dj_analyze.detect_downbeats import (
    DownbeatResult,
    compute_chroma_ssm,
    detect_downbeats,
    detect_phrases,
)

FIXTURES = Path(__file__).parent / "fixtures"


def test_detect_downbeats_method_is_always_madmom():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    result: DownbeatResult = detect_downbeats(audio, sr)
    assert result.method == "madmom"


def test_detect_downbeats_returns_sorted_times():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    result = detect_downbeats(audio, sr)
    assert all(a <= b for a, b in zip(result.downbeats, result.downbeats[1:]))


# detect_phrases / compute_chroma_ssm tests preserved — used independently by analyze.py
# for phrase-boundary detection. These do not depend on downbeat fallback paths.


def test_detect_phrases_on_clean_click_returns_high_confidence_and_boundaries():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = [0.5 * i for i in range(16)]
    result = detect_phrases(audio, sr, beats=beats, output_stride_bars=1)
    assert result.confidence > 0.3
    assert len(result.boundaries) >= 2


def test_detect_phrases_16_bar_stride_yields_fewer_boundaries():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = [0.5 * i for i in range(16)]
    result = detect_phrases(audio, sr, beats=beats)
    assert len(result.boundaries) <= 1


def test_detect_phrases_on_noise_returns_low_confidence():
    rng = np.random.default_rng(seed=42)
    audio = rng.uniform(-0.5, 0.5, size=44100 * 4).astype(np.float32)
    beats = [0.5 * i for i in range(8)]
    result = detect_phrases(audio, sr=44100, beats=beats, output_stride_bars=1)
    assert result.confidence < 0.6


def test_detect_phrases_handles_too_few_beats():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    result = detect_phrases(audio, sr, beats=[0.0, 0.5], output_stride_bars=1)
    assert result.boundaries == []
    assert result.confidence == 0.0


def test_detect_phrases_phrase_bars_legacy_alias_still_works():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = [0.5 * i for i in range(16)]
    via_legacy = detect_phrases(audio, sr, beats=beats, phrase_bars=1)
    via_new = detect_phrases(audio, sr, beats=beats, output_stride_bars=1)
    assert via_legacy.boundaries == via_new.boundaries
    assert via_legacy.confidence == via_new.confidence


def test_compute_chroma_ssm_returns_square_matrix():
    sr = 22050
    seg = sr * 4
    a = np.sin(2 * np.pi * 220 * np.arange(seg) / sr).astype(np.float32) * 0.5
    b = np.sin(2 * np.pi * 330 * np.arange(seg) / sr).astype(np.float32) * 0.5
    audio = np.concatenate([a, b])
    beats = [0.5 * i for i in range(16)]
    ssm = compute_chroma_ssm(audio, sr, beats)
    assert ssm is not None
    assert ssm.ndim == 2
    assert ssm.shape[0] == ssm.shape[1]


def test_compute_chroma_ssm_too_short_returns_none():
    sr = 22050
    audio = np.zeros(100, dtype=np.float32)
    assert compute_chroma_ssm(audio, sr, beats=[]) is None
```

- [ ] **Step 2: Run tests to see them fail**

```powershell
pytest analyzer/tests/test_detect_downbeats.py -v
```
Expected: FAIL — `detect_downbeats` signature still expects `beats=` and `force_fallback=`.

- [ ] **Step 3: Rewrite `detect_downbeats.py`**

Edit `analyzer/src/dj_analyze/detect_downbeats.py`. Replace the top of the file (docstring, `DownbeatResult`, `detect_downbeats`, `_madmom_downbeats`) with:

```python
"""Downbeat detection (madmom) + phrase-structure detection (librosa SSM).

After the madmom-required switch, downbeats come exclusively from
detect_beat_grid. The librosa-based phrase / chroma-SSM helpers below are
retained — they are used by analyze.py to derive phrase boundaries and to
feed novelty detection, independent of the downbeat detector.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

import numpy as np

from .detect_beat_grid import detect_beat_grid


@dataclass(frozen=True, slots=True)
class DownbeatResult:
    downbeats: list[float]
    method: Literal["madmom"]


@dataclass(frozen=True, slots=True)
class PhraseResult:
    boundaries: list[float]
    confidence: float


def detect_downbeats(audio: np.ndarray, sr: int) -> DownbeatResult:
    """Return downbeat times from the madmom beat-grid pass."""
    grid = detect_beat_grid(audio, sr)
    return DownbeatResult(downbeats=grid.downbeats, method="madmom")
```

Keep the existing `_SEARCH_PERIODS_BEATS`, `compute_chroma_ssm`, and `detect_phrases` functions intact (lines 49–139 of the current file). Delete the `_madmom_downbeats` function at the bottom — it's superseded by detect_beat_grid.

- [ ] **Step 4: Run tests to verify they pass**

```powershell
pytest analyzer/tests/test_detect_downbeats.py -v
```
Expected: all PASS.

- [ ] **Step 5: Commit**

```powershell
git add analyzer/src/dj_analyze/detect_downbeats.py analyzer/tests/test_detect_downbeats.py
git commit -m "refactor(analyzer): detect_downbeats becomes thin madmom wrapper; drop fallbacks"
```

---

## Task 5: Switch `detect_bpm.py` to derive global BPM from madmom beats

**Files:**
- Modify: `analyzer/src/dj_analyze/detect_bpm.py`
- Modify: `analyzer/tests/test_detect_bpm.py` (if it exists; check first)

- [ ] **Step 1: Check existing test file**

```powershell
Test-Path analyzer/tests/test_detect_bpm.py
```
If present, read it to know which behaviors are asserted. Most likely it asserts the global BPM falls within a tolerance of the fixture's true BPM.

- [ ] **Step 2: Write/update a focused test**

If `test_detect_bpm.py` exists, preserve the high-level tempo-tolerance test. Either way, ensure this test is present:
```python
from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_bpm import detect_bpm

FIXTURES = Path(__file__).parent / "fixtures"


def test_detect_bpm_on_120_click_returns_near_120():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    bpm, conf = detect_bpm(audio, sr)
    assert 119.0 < bpm < 121.0, f"expected ~120, got {bpm}"
    assert 0.0 <= conf <= 1.0
```

- [ ] **Step 3: Refactor `detect_bpm.py`**

The existing `_snap_to_perceptual`, `resolve_octave`, and `_tempo_stability` helpers stay. Replace `detect_bpm` (lines 98–125) with:

```python
def detect_bpm(audio: np.ndarray, sr: int) -> tuple[float, float]:
    """Estimate global BPM and a stability confidence in [0, 1].

    BPM is derived from madmom beats (median IBI across the whole song), then
    routed through resolve_octave to guard against octave errors. Confidence is
    the existing 10-second-window voting metric, unchanged.
    """
    from .detect_beat_grid import detect_beat_grid

    grid = detect_beat_grid(audio, sr)
    if len(grid.beats) < 2:
        # Fallback: librosa global tempo when madmom finds no beats. Should be
        # exceptionally rare on real audio; mirrors the previous safety net.
        onset_env = librosa.onset.onset_strength(y=audio, sr=sr)
        global_tempo = librosa.beat.tempo(onset_envelope=onset_env, sr=sr, aggregate=np.median)
        primary = float(global_tempo[0])
        resolved = _snap_to_perceptual(primary)
        return resolved, _tempo_stability(audio, sr, resolved)

    intervals = np.diff(np.asarray(grid.beats))
    median_ibi = float(np.median(intervals))
    primary = 60.0 / median_ibi if median_ibi > 0 else 0.0

    # Secondary: 10s-window librosa estimate kept as the octave cross-check.
    onset_env = librosa.onset.onset_strength(y=audio, sr=sr)
    global_tempo = librosa.beat.tempo(onset_envelope=onset_env, sr=sr, aggregate=np.median)
    secondary = float(global_tempo[0]) if len(global_tempo) > 0 else None

    resolved = resolve_octave(primary, secondary)
    confidence = _tempo_stability(audio, sr, resolved)
    return resolved, confidence
```

`bpm_from_beats` can stay (still useful as a helper). The `_octave_normalize` and `_VOTE_TOLERANCE` constants stay (used by `_tempo_stability`).

- [ ] **Step 4: Run tests**

```powershell
pytest analyzer/tests/test_detect_bpm.py -v
```
Expected: tests PASS, bpm within ±1 of 120 on the click fixture.

- [ ] **Step 5: Commit**

```powershell
git add analyzer/src/dj_analyze/detect_bpm.py analyzer/tests/test_detect_bpm.py
git commit -m "refactor(analyzer): detect_bpm derives global tempo from madmom beats"
```

---

## Task 6: Add `compute_local_bpm` helper with tests

**Files:**
- Create: helper function in `analyzer/src/dj_analyze/analyze.py` (top of file, exported)
- Create: `analyzer/tests/test_compute_local_bpm.py`

- [ ] **Step 1: Write the failing test**

Create `analyzer/tests/test_compute_local_bpm.py`:
```python
from __future__ import annotations

from dj_analyze.analyze import compute_local_bpm


def test_compute_local_bpm_uniform_120():
    """Four bars of 120 BPM: each bar has 4 beats at 0.5s spacing."""
    downbeats = [0.0, 2.0, 4.0, 6.0, 8.0]  # 4 bars
    beats = [0.5 * i for i in range(17)]  # 0.0, 0.5, ..., 8.0
    result = compute_local_bpm(beats=beats, downbeats=downbeats)
    assert len(result) == 4  # one entry per bar
    for bpm in result:
        assert bpm is not None
        assert abs(bpm - 120.0) < 0.5


def test_compute_local_bpm_underdetermined_bar_returns_none():
    """A bar with fewer than 2 beats yields None."""
    downbeats = [0.0, 2.0, 4.0]
    # Bar 0 has 4 beats; bar 1 has only 1 beat.
    beats = [0.0, 0.5, 1.0, 1.5, 2.0, 3.0]
    result = compute_local_bpm(beats=beats, downbeats=downbeats)
    assert len(result) == 2
    assert result[0] is not None
    assert result[1] is None


def test_compute_local_bpm_empty_inputs():
    assert compute_local_bpm(beats=[], downbeats=[]) == []
    assert compute_local_bpm(beats=[0.0, 0.5], downbeats=[]) == []
    assert compute_local_bpm(beats=[], downbeats=[0.0, 2.0]) == [None]


def test_compute_local_bpm_drift_within_bar():
    """A bar whose IBIs are 0.5s/0.5s/0.5s vs another whose IBIs vary."""
    downbeats = [0.0, 2.0, 4.0]
    # Bar 0: 120 BPM clean. Bar 1: slowing down (140 → 120 → 100 BPM equivalents).
    beats = [
        0.0, 0.5, 1.0, 1.5,
        2.0, 2.43, 2.93, 3.53, 4.0,  # IBIs 0.43, 0.5, 0.6, 0.47 — median ~0.485 → ~123 BPM
    ]
    result = compute_local_bpm(beats=beats, downbeats=downbeats)
    assert result[0] is not None
    assert abs(result[0] - 120.0) < 0.5
    assert result[1] is not None
    # Median of [0.43, 0.5, 0.6, 0.47] = 0.485 → 60/0.485 ≈ 123.7
    assert 120.0 < result[1] < 128.0
```

- [ ] **Step 2: Run to see fail**

```powershell
pytest analyzer/tests/test_compute_local_bpm.py -v
```
Expected: FAIL — `cannot import compute_local_bpm`.

- [ ] **Step 3: Add the helper to `analyze.py`**

Add at the top of `analyzer/src/dj_analyze/analyze.py`, after the imports:
```python
def compute_local_bpm(
    *, beats: list[float], downbeats: list[float]
) -> list[float | None]:
    """Per-bar local BPM from median inter-beat interval within each bar.

    Returns one entry per fully-defined bar (between consecutive downbeats).
    Entry is None when the bar contains fewer than 2 beats. The partial bar
    after the last downbeat is dropped, matching the existing per-bar arrays.
    """
    import statistics

    if len(downbeats) < 2:
        # Zero or one downbeat: no fully-defined bars except possibly a single
        # "bar 0" bounded above by nothing. By the spec, drop that case.
        if len(downbeats) == 1:
            return [None]
        return []

    out: list[float | None] = []
    for i in range(len(downbeats) - 1):
        t0, t1 = downbeats[i], downbeats[i + 1]
        in_bar = [b for b in beats if t0 <= b <= t1]
        if len(in_bar) < 2:
            out.append(None)
            continue
        ibis = [in_bar[j + 1] - in_bar[j] for j in range(len(in_bar) - 1)]
        if not ibis:
            out.append(None)
            continue
        median = statistics.median(ibis)
        if median <= 0:
            out.append(None)
            continue
        out.append(60.0 / median)
    return out
```

- [ ] **Step 4: Run tests**

```powershell
pytest analyzer/tests/test_compute_local_bpm.py -v
```
Expected: 4 tests PASS.

- [ ] **Step 5: Commit**

```powershell
git add analyzer/src/dj_analyze/analyze.py analyzer/tests/test_compute_local_bpm.py
git commit -m "feat(analyzer): compute_local_bpm helper for per-bar tempo"
```

---

## Task 7: Wire the new fields into `analyze.py` output

**Files:**
- Modify: `analyzer/src/dj_analyze/analyze.py`

- [ ] **Step 1: Add a smoke test for the new sidecar fields**

Add to `analyzer/tests/test_analyze.py` (create if it doesn't exist):
```python
from __future__ import annotations

from pathlib import Path

from dj_analyze.analyze import analyze_track

FIXTURES = Path(__file__).parent / "fixtures"


def test_analyze_track_emits_new_madmom_fields():
    sidecar = analyze_track(FIXTURES / "click_120bpm.wav")
    assert "local_bpm" in sidecar
    assert "beat_confidence" in sidecar
    assert "downbeat_confidence" in sidecar
    # Length invariants.
    assert len(sidecar["beat_confidence"]) == len(sidecar["beats_seconds"])
    assert len(sidecar["downbeat_confidence"]) == len(sidecar["downbeats_seconds"])
    if len(sidecar["downbeats_seconds"]) >= 2:
        assert len(sidecar["local_bpm"]) == len(sidecar["downbeats_seconds"]) - 1
```

If `test_analyze.py` already exists, append the test function (don't replace the file).

- [ ] **Step 2: Run to see fail**

```powershell
pytest analyzer/tests/test_analyze.py::test_analyze_track_emits_new_madmom_fields -v
```
Expected: FAIL — fields don't exist yet.

- [ ] **Step 3: Refactor analyze.py to call detect_beat_grid once**

Edit `analyzer/src/dj_analyze/analyze.py`. Replace the existing detection sequence (lines around 41–53):
```python
    duration = len(audio) / sr
    bpm, bpm_conf = detect_bpm(audio, sr)
    beats = detect_beats(audio, sr, bpm=bpm)
    db_result = detect_downbeats(audio, sr, beats=beats)
```

With:
```python
    duration = len(audio) / sr

    # Single madmom RNN+DBN pass — used for beats, downbeats, and confidences.
    grid = detect_beat_grid(audio, sr)
    beats = grid.beats
    db_result = detect_downbeats(audio, sr)  # thin wrapper, reuses madmom internally

    bpm, bpm_conf = detect_bpm(audio, sr)

    local_bpm = compute_local_bpm(beats=beats, downbeats=db_result.downbeats)
```

Add an import at the top of the file:
```python
from .detect_beat_grid import detect_beat_grid
```

Then in the output dict (around lines 93–113), add three new entries after `"downbeats_seconds": db_result.downbeats,`:
```python
        "local_bpm": local_bpm,
        "beat_confidence": grid.beat_confidence,
        "downbeat_confidence": grid.downbeat_confidence,
```

Note: `detect_beat_grid` is called once (for `grid.beats`, confidences) and `detect_downbeats` is called once (a thin wrapper that calls `detect_beat_grid` again internally — for now this is a duplicate call). To avoid two madmom invocations per track:

Replace the `detect_downbeats(audio, sr)` line with:
```python
    db_result = DownbeatResult(downbeats=grid.downbeats, method="madmom")
```

And add to imports:
```python
from .detect_downbeats import DownbeatResult, compute_chroma_ssm, detect_phrases
```
(Remove `detect_downbeats` from the existing import line.)

- [ ] **Step 4: Run the smoke test + the broader analyzer suite**

```powershell
pytest analyzer/tests/test_analyze.py -v
pytest analyzer/tests/ --tb=short
```
Expected: all PASS, no regressions.

- [ ] **Step 5: Commit**

```powershell
git add analyzer/src/dj_analyze/analyze.py analyzer/tests/test_analyze.py
git commit -m "feat(analyzer): single madmom call in analyze_track; emit local_bpm + confidences"
```

---

## Task 8: Simplify `mixability.py` after hard-required madmom

**Files:**
- Modify: `analyzer/src/dj_analyze/mixability.py`
- Modify: `analyzer/tests/test_mixability.py` (verify)

- [ ] **Step 1: Read the current `mixability.py` and decide the simplification**

Current parameter:
```python
downbeat_method: Literal["madmom", "librosa_phrase", "fallback_4_4"],
```
Used to:
```python
is_phrase_aligned = downbeat_method in {"madmom", "librosa_phrase"}
```

After madmom is required, every track gets `method="madmom"`. The `is_phrase_aligned` flag is always True from this gate (other criteria may still flip it). The parameter becomes dead weight.

Decision: keep the parameter (so the call site doesn't need restructuring), narrow its type to `Literal["madmom"]`, and replace the conditional with a constant `True`. This preserves the field for future expansion (e.g., when downbeat_confidence drives phrase_aligned).

- [ ] **Step 2: Update `mixability.py`**

Edit `analyzer/src/dj_analyze/mixability.py`:

Replace:
```python
    downbeat_method: Literal["madmom", "librosa_phrase", "fallback_4_4"],
```
with:
```python
    downbeat_method: Literal["madmom"],
```

Replace the block:
```python
    # Phrase alignment requires actual structural data; fallback_4_4 doesn't count.
    is_phrase_aligned = downbeat_method in {"madmom", "librosa_phrase"}
    if not is_phrase_aligned:
        reasons.append("phrase_low_confidence")
```
with:
```python
    # After the madmom-required switch, downbeat_method is always "madmom" — the
    # phrase-alignment gate now depends entirely on the other criteria below.
    # Future: gate on downbeat_confidence threshold once we have a baseline.
    is_phrase_aligned = True
```

(Don't append `"phrase_low_confidence"` to reasons unconditionally — that would flip the bool to False elsewhere.)

- [ ] **Step 3: Run existing mixability tests**

```powershell
pytest analyzer/tests/test_mixability.py -v
```
If any tests passed in `downbeat_method="fallback_4_4"` or `"librosa_phrase"` and asserted `is_phrase_aligned=False`, they'll fail. Update them: change the parameter to `"madmom"` and adjust the assertion to reflect that phrase_aligned now depends on other criteria only.

- [ ] **Step 4: Run the whole analyzer suite**

```powershell
pytest analyzer/tests/ -x --tb=short
```
Expected: all PASS.

- [ ] **Step 5: Commit**

```powershell
git add analyzer/src/dj_analyze/mixability.py analyzer/tests/test_mixability.py
git commit -m "refactor(analyzer): mixability.downbeat_method narrowed to Literal['madmom']"
```

---

## Task 9: Bump analyzer `__version__` to 0.5.0

**Files:**
- Modify: `analyzer/src/dj_analyze/__init__.py`

- [ ] **Step 1: Edit the version**

Replace contents of `analyzer/src/dj_analyze/__init__.py`:
```python
"""DJ Engine analyzer — emits sidecar JSONs from a directory of audio files."""

__version__ = "0.5.0"
```

- [ ] **Step 2: Verify import**

```powershell
python -c "import dj_analyze; print(dj_analyze.__version__)"
```
Expected: `0.5.0`.

- [ ] **Step 3: Commit**

```powershell
git add analyzer/src/dj_analyze/__init__.py
git commit -m "chore(analyzer): bump version to 0.5.0 (madmom-required)"
```

---

## Task 10: Update JSON schema

**Files:**
- Modify: `schema/sidecar.schema.json`

- [ ] **Step 1: Inspect the existing schema**

```powershell
Get-Content schema/sidecar.schema.json
```
Note where the existing `band_energy_curves`, `band_energy_means`, `stems` (the additive optional fields) are defined.

- [ ] **Step 2: Add the three new fields**

Add the following entries inside the `properties` block of the schema, alongside the other optional v2 fields:
```json
"local_bpm": {
    "type": "array",
    "items": { "type": ["number", "null"] },
    "description": "Per-bar local BPM derived from non-uniform madmom beats. Length = len(downbeats_seconds) - 1. null when a bar has fewer than 2 beats."
},
"beat_confidence": {
    "type": "array",
    "items": { "type": "number", "minimum": 0, "maximum": 1 },
    "description": "Per-beat RNN activation confidence from madmom, parallel to beats_seconds."
},
"downbeat_confidence": {
    "type": "array",
    "items": { "type": "number", "minimum": 0, "maximum": 1 },
    "description": "Per-downbeat RNN activation confidence from madmom, parallel to downbeats_seconds."
}
```

Make sure NOT to add them to a `required` list. They are optional additions.

- [ ] **Step 3: Validate the schema is syntactically valid JSON**

```powershell
python -c "import json; json.load(open('schema/sidecar.schema.json')); print('valid')"
```
Expected: `valid`.

- [ ] **Step 4: Commit**

```powershell
git add schema/sidecar.schema.json
git commit -m "docs(schema): add local_bpm, beat_confidence, downbeat_confidence (optional)"
```

---

## Task 11: Add new fields and helper to `engine/src/sidecar.rs`

**Files:**
- Modify: `engine/src/sidecar.rs`
- Modify: `engine/tests/sidecar_test.rs` (verify; create test if absent)

- [ ] **Step 1: Write failing tests**

Add to `engine/tests/sidecar_test.rs` (create the file if absent — match existing test layout):
```rust
use dj_engine::sidecar::Sidecar;

fn make_sidecar(
    downbeats: Vec<f64>,
    local_bpm: Vec<Option<f64>>,
) -> Sidecar {
    Sidecar {
        schema_version: 2,
        analyzer_version: "0.5.0".into(),
        source_path: "test".into(),
        source_mtime: 0.0,
        source_size_bytes: 0,
        error: None,
        duration_seconds: Some(120.0),
        sample_rate: Some(44100),
        bpm: Some(120.0),
        bpm_confidence: Some(1.0),
        beats_seconds: vec![],
        downbeats_seconds: downbeats,
        key: None,
        key_label: None,
        key_confidence: None,
        energy_curve: vec![],
        energy_mean: None,
        cue_in_seconds: None,
        cue_out_seconds: None,
        intro_end_seconds: None,
        outro_start_seconds: None,
        is_mixable: None,
        mixable_reasons: vec![],
        phrase_boundaries_seconds: None,
        phrase_confidence: None,
        band_energy_curves: None,
        band_energy_means: None,
        is_beatmatchable: None,
        is_phrase_aligned: None,
        stems: None,
        local_bpm,
        beat_confidence: vec![],
        downbeat_confidence: vec![],
    }
}

#[test]
fn local_bpm_in_window_uniform_returns_value() {
    let s = make_sidecar(
        vec![0.0, 2.0, 4.0, 6.0, 8.0],
        vec![Some(120.0), Some(120.0), Some(120.0), Some(120.0)],
    );
    let v = s.local_bpm_in_window(0.0, 8.0).unwrap();
    assert!((v - 120.0).abs() < 1e-9);
}

#[test]
fn local_bpm_in_window_skips_nones() {
    let s = make_sidecar(
        vec![0.0, 2.0, 4.0, 6.0],
        vec![Some(120.0), None, Some(124.0)],
    );
    let v = s.local_bpm_in_window(0.0, 6.0).unwrap();
    assert!((v - 122.0).abs() < 1e-9); // average of 120 and 124
}

#[test]
fn local_bpm_in_window_no_overlap_returns_none() {
    let s = make_sidecar(
        vec![0.0, 2.0, 4.0],
        vec![Some(120.0), Some(120.0)],
    );
    assert!(s.local_bpm_in_window(100.0, 200.0).is_none());
}

#[test]
fn local_bpm_in_window_empty_array_returns_none() {
    let s = make_sidecar(vec![0.0, 2.0], vec![]);
    assert!(s.local_bpm_in_window(0.0, 2.0).is_none());
}

#[test]
fn local_bpm_in_window_all_nones_returns_none() {
    let s = make_sidecar(vec![0.0, 2.0, 4.0], vec![None, None]);
    assert!(s.local_bpm_in_window(0.0, 4.0).is_none());
}
```

- [ ] **Step 2: Run to see fail**

```powershell
# First stop any running engine process so cargo can build.
Stop-Process -Name dj-engine -Force -ErrorAction SilentlyContinue
cargo test --manifest-path engine/Cargo.toml --test sidecar_test 2>&1 | Select-Object -First 30
```
Expected: compile errors — `Sidecar` struct doesn't have `local_bpm`, `beat_confidence`, `downbeat_confidence`; `local_bpm_in_window` not defined.

- [ ] **Step 3: Add fields and helper to `engine/src/sidecar.rs`**

Edit `engine/src/sidecar.rs`. In the `Sidecar` struct (after the `stems` field), add:
```rust
    /// Per-bar local BPM from madmom-derived non-uniform beats.
    /// Length = len(downbeats_seconds) - 1 when ≥2 downbeats; empty otherwise.
    /// Some(bpm) when the bar had ≥2 beats; None for underdetermined bars.
    #[serde(default)]
    pub local_bpm: Vec<Option<f64>>,

    /// Per-beat RNN activation confidence, parallel to beats_seconds.
    #[serde(default)]
    pub beat_confidence: Vec<f64>,

    /// Per-downbeat RNN activation confidence, parallel to downbeats_seconds.
    #[serde(default)]
    pub downbeat_confidence: Vec<f64>,
```

In the `impl Sidecar` block (after `phrase_aligned`), add the helper:
```rust
    /// Average `local_bpm` over bars whose interval overlaps
    /// `[start_seconds, end_seconds]`, skipping `None` entries.
    ///
    /// Returns `None` when no Some(bpm) entry overlaps the window — caller
    /// should fall back to `self.bpm`.
    pub fn local_bpm_in_window(&self, start_seconds: f64, end_seconds: f64) -> Option<f64> {
        if self.local_bpm.is_empty() || self.downbeats_seconds.len() < 2 {
            return None;
        }
        let mut sum = 0.0;
        let mut count = 0usize;
        for (i, value) in self.local_bpm.iter().enumerate() {
            let Some(bpm) = value else { continue };
            // Bar i spans [downbeats[i], downbeats[i+1]).
            let Some(bar_start) = self.downbeats_seconds.get(i) else { continue };
            let Some(bar_end) = self.downbeats_seconds.get(i + 1) else { continue };
            // Inclusive overlap.
            if *bar_end < start_seconds || *bar_start > end_seconds {
                continue;
            }
            sum += bpm;
            count += 1;
        }
        if count == 0 { None } else { Some(sum / count as f64) }
    }
```

- [ ] **Step 4: Verify build + tests pass**

```powershell
cargo test --manifest-path engine/Cargo.toml --test sidecar_test
```
Expected: all 5 new tests PASS.

- [ ] **Step 5: Commit**

```powershell
git add engine/src/sidecar.rs engine/tests/sidecar_test.rs
git commit -m "feat(engine): Sidecar fields local_bpm + confidences + local_bpm_in_window helper"
```

---

## Task 12: Update scheduler + TransitionContext to compute and log both ratios

**Files:**
- Modify: `engine/src/recipe.rs` (struct shape only — recipes are updated in Task 13)
- Modify: `engine/src/scheduler.rs`

- [ ] **Step 1: Write a failing test for context ratio computation**

Add to `engine/tests/scheduler_test.rs` (append; do NOT replace existing tests):
```rust
// Verifies that when local_bpm data is present, the ratio computed for a
// transition reflects the local section average — not the global BPM.
//
// This test inspects the context fields directly; the recipe updates that
// consume them are exercised in recipe_test.rs.
#[test]
fn ratio_uses_local_section_bpm_when_present() {
    use dj_engine::recipe::compute_section_ratio;

    // Outgoing has local_bpm 130 in its outro window; global BPM is 120.
    // Incoming has local_bpm 125 in its intro window; global BPM is 124.
    let result = compute_section_ratio(
        /*outgoing_section_bpm=*/ Some(130.0),
        /*outgoing_global_bpm=*/ 120.0,
        /*incoming_section_bpm=*/ Some(125.0),
        /*incoming_global_bpm=*/ 124.0,
    );
    assert!((result.ratio_local - (130.0 / 125.0)).abs() < 1e-9);
    assert!((result.ratio_global - (120.0 / 124.0)).abs() < 1e-9);
}

#[test]
fn ratio_falls_back_to_global_when_section_missing() {
    use dj_engine::recipe::compute_section_ratio;

    let result = compute_section_ratio(None, 120.0, None, 124.0);
    assert!((result.ratio_local - (120.0 / 124.0)).abs() < 1e-9);
    assert!((result.ratio_global - (120.0 / 124.0)).abs() < 1e-9);
}
```

- [ ] **Step 2: Run to see fail**

```powershell
cargo test --manifest-path engine/Cargo.toml --test scheduler_test ratio_ 2>&1 | Select-Object -First 20
```
Expected: compile error — `compute_section_ratio` not defined.

- [ ] **Step 3: Add `compute_section_ratio` helper to `engine/src/recipe.rs`**

Add near the top of `engine/src/recipe.rs`, after the `RecipeOutput` struct definition:
```rust
/// Result of computing both the new (local-section) and old (global) ratios
/// for a transition. The scheduler stashes both on TransitionContext and logs
/// them at arming.
#[derive(Clone, Copy, Debug)]
pub struct SectionRatio {
    pub ratio_local: f64,
    pub ratio_global: f64,
}

/// Compute the time-stretch ratio from local section BPMs with global fallback.
///
/// - When both section BPMs are `Some`, the local ratio is `outgoing / incoming`.
/// - When either is `None`, falls back to the global ratio for the local field.
/// - The global field is always `outgoing_global / incoming_global`.
pub fn compute_section_ratio(
    outgoing_section_bpm: Option<f64>,
    outgoing_global_bpm: f64,
    incoming_section_bpm: Option<f64>,
    incoming_global_bpm: f64,
) -> SectionRatio {
    let ratio_global = outgoing_global_bpm / incoming_global_bpm.max(1.0);
    let ratio_local = match (outgoing_section_bpm, incoming_section_bpm) {
        (Some(o), Some(i)) => o / i.max(1.0),
        _ => ratio_global,
    };
    SectionRatio { ratio_local, ratio_global }
}
```

Also extend `TransitionContext` with cached ratio fields:
```rust
pub struct TransitionContext<'a> {
    pub outgoing: &'a LoadedTrack,
    pub incoming: &'a LoadedTrack,
    pub plan: &'a TransitionPlan,
    pub master_clock_frames: u64,
    pub outgoing_deck: DeckId,
    pub incoming_deck: DeckId,
    pub sample_rate: u32,
    /// Precomputed ratio used by recipes. Reflects local section BPM when
    /// available, else global. Logged as `ratio_local`.
    pub ratio: f64,
    /// The old global-BPM ratio, kept for observability logging.
    pub ratio_global: f64,
}
```

- [ ] **Step 4: Verify the test passes**

```powershell
cargo test --manifest-path engine/Cargo.toml --test scheduler_test ratio_
```
Expected: both new tests PASS. Existing scheduler_test tests may fail compilation due to `TransitionContext` construction — they need the new fields. Fix them by adding `ratio: 1.0, ratio_global: 1.0,` to TransitionContext literals in those tests (mechanical edit).

Re-run after the fix:
```powershell
cargo test --manifest-path engine/Cargo.toml --test scheduler_test
```
All tests should PASS.

- [ ] **Step 5: Update `scheduler.rs::arm` to compute and log both ratios**

In `engine/src/scheduler.rs`, replace the block that computes `ratio` (around lines 31–34):
```rust
        let outgoing_bpm = outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
        let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
```

With:
```rust
        let outgoing_global_bpm = outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_global_bpm = incoming.sidecar.bpm.unwrap_or(outgoing_global_bpm);
        let bar_seconds = (60.0 / outgoing_global_bpm) * 4.0;

        // Determine the transition window on each track for local-BPM averaging.
        let length_seconds = (plan.length_bars as f64) * bar_seconds;
        let outgoing_window_start = plan.outgoing_crossfade_start_seconds;
        let outgoing_window_end = outgoing_window_start + length_seconds;
        let incoming_window_start = plan.incoming_crossfade_start_seconds;
        let incoming_window_end = incoming_window_start + length_seconds;

        let outgoing_section_bpm = outgoing
            .sidecar
            .local_bpm_in_window(outgoing_window_start, outgoing_window_end);
        let incoming_section_bpm = incoming
            .sidecar
            .local_bpm_in_window(incoming_window_start, incoming_window_end);

        let section_ratio = crate::recipe::compute_section_ratio(
            outgoing_section_bpm,
            outgoing_global_bpm,
            incoming_section_bpm,
            incoming_global_bpm,
        );
        let ratio = section_ratio.ratio_local;
```

(`plan.outgoing_crossfade_start_seconds` and `plan.incoming_crossfade_start_seconds` are the existing fields in `engine/src/transition.rs::TransitionPlan` — verified.)

Then where `TransitionContext` is constructed (around lines 109–117), add the new fields:
```rust
        let ctx = TransitionContext {
            outgoing,
            incoming,
            plan,
            master_clock_frames: t_start,
            outgoing_deck,
            incoming_deck,
            sample_rate: SAMPLE_RATE,
            ratio,
            ratio_global: section_ratio.ratio_global,
        };
```

Then extend the `transition arming` log (already exists in the codebase — find the `tracing::info!` with `"transition arming"`). Add to its field list:
```rust
            ratio_local = section_ratio.ratio_local,
            ratio_global = section_ratio.ratio_global,
            bpm_delta_pct = (section_ratio.ratio_local - section_ratio.ratio_global).abs()
                / section_ratio.ratio_global.max(1e-9) * 100.0,
            outgoing_section_bpm = outgoing_section_bpm.unwrap_or(f64::NAN),
            incoming_section_bpm = incoming_section_bpm.unwrap_or(f64::NAN),
```

- [ ] **Step 6: Build to surface call-site breakages**

```powershell
cargo build --manifest-path engine/Cargo.toml --release 2>&1 | Select-Object -First 50
```
Expected: build fails on the 14 recipe `build_script` calls that don't yet use `ctx.ratio`, and possibly other call sites constructing `TransitionContext` literals (in tests). DO NOT FIX RECIPES YET — that's Task 13. Just fix non-recipe call sites (test helpers, etc.) by adding `ratio: 1.0, ratio_global: 1.0,` to literal constructions.

- [ ] **Step 7: Commit the scheduler + context shell**

```powershell
git add engine/src/recipe.rs engine/src/scheduler.rs engine/tests/scheduler_test.rs
git commit -m "feat(engine): TransitionContext carries ratio + ratio_global; scheduler logs both"
```

---

## Task 13: Update recipes to read `ctx.ratio`

**Files:**
- Modify: `engine/src/recipe.rs` (every recipe's `build_script`)

- [ ] **Step 1: Identify every recipe that recomputes ratio**

```powershell
Select-String -Path engine/src/recipe.rs -Pattern 'ratio = outgoing_bpm / incoming_bpm' -SimpleMatch | Select-Object LineNumber, Line
```
Expect ~14 matches.

- [ ] **Step 2: Replace each occurrence**

For each recipe's `build_script`, find and replace the existing ratio computation:
```rust
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let incoming_bpm = ctx.incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
        let ratio = outgoing_bpm / incoming_bpm.max(1.0);
```

With:
```rust
        let ratio = ctx.ratio;
```

If the recipe uses `outgoing_bpm` later (e.g., to compute `bar_seconds`, `beat_seconds`), keep just that line:
```rust
        let outgoing_bpm = ctx.outgoing.sidecar.bpm.unwrap_or(120.0);
        let ratio = ctx.ratio;
```

Recipes confirmed to use `outgoing_bpm` for bar/beat-seconds (search before editing): `EchoOut`, `EchoThrow`, `LoopOut`. Others can drop the variable.

- [ ] **Step 3: Build + run all engine tests**

```powershell
cargo build --manifest-path engine/Cargo.toml --release
cargo test --manifest-path engine/Cargo.toml
```
Expected: clean build, all tests PASS.

- [ ] **Step 4: Commit**

```powershell
git add engine/src/recipe.rs
git commit -m "refactor(engine): recipes consume ctx.ratio instead of recomputing from globals"
```

---

## Task 14: Snapshot existing sidecars and re-analyze the library

**Files:**
- Side-effect only: `analyzed-real/` updated; `analyzed-real.v0.4.0/` created.

- [ ] **Step 1: Snapshot existing sidecars**

```powershell
Copy-Item -Path analyzed-real -Destination analyzed-real.v0.4.0 -Recurse
```
Verify the snapshot:
```powershell
(Get-ChildItem analyzed-real.v0.4.0 -Filter '*.json').Count
```
Expected: 18 (matching the count of sidecars in `analyzed-real/`).

- [ ] **Step 2: Re-analyze the library**

Use the CLI flag identified in Task 1 Step 7. The most likely command:
```powershell
dj-analyze --music-root data --sidecars-out analyzed-real --force
```
If `--force` doesn't exist, delete the sidecars first:
```powershell
Remove-Item analyzed-real/*.json
dj-analyze --music-root data --sidecars-out analyzed-real
```

Expected: 18 sidecars regenerated. Time on CPU: 5–15 minutes (madmom RNN dominates).

- [ ] **Step 3: Spot-check a sidecar**

```powershell
python -c "
import json
d = json.load(open('analyzed-real/001-song.mp3.json'))
print('analyzer_version:', d.get('analyzer_version'))
print('beats:', len(d.get('beats_seconds', [])))
print('downbeats:', len(d.get('downbeats_seconds', [])))
print('beat_confidence:', len(d.get('beat_confidence', [])))
print('downbeat_confidence:', len(d.get('downbeat_confidence', [])))
print('local_bpm:', len(d.get('local_bpm', [])))
print('mean local_bpm (Some only):', sum(b for b in d['local_bpm'] if b is not None) / max(1, len([b for b in d['local_bpm'] if b is not None])))
"
```
Expected:
- `analyzer_version: 0.5.0`
- `beats == beat_confidence`
- `downbeats == downbeat_confidence`
- `local_bpm == downbeats - 1`
- `mean local_bpm` reasonable (close to the track's BPM).

- [ ] **Step 4: Sidecar diff vs v0.4.0 snapshot**

Quick comparison script (one-off; left to the implementer to write):
- For each of the 18 tracks, count downbeats that shifted by > 100ms between v0.4.0 and v0.5.0.
- Report the worst track. If madmom moves downbeats meaningfully on the tracks you complained about, that's evidence the analyzer was the bug. If they agree to within 100ms everywhere, the audible misalignment cause lies elsewhere.

- [ ] **Step 5: Commit the new sidecars**

```powershell
git add analyzed-real/
git commit -m "data: re-analyze library with analyzer 0.5.0 (madmom-based)"
```

(Optional: add `analyzed-real.v0.4.0/` to `.gitignore` if not already; it's a local snapshot.)

---

## Task 15: Restart engine and verify the new log fields appear

**Files:**
- Side-effect only: engine binary restarted.

- [ ] **Step 1: Stop the running engine**

```powershell
Stop-Process -Name dj-engine -Force -ErrorAction SilentlyContinue
```

- [ ] **Step 2: Rebuild the engine**

```powershell
cargo build --manifest-path engine/Cargo.toml --release
```
Expected: clean release build.

- [ ] **Step 3: Restart the engine pointing at the new sidecars**

```powershell
$env:ICECAST_URL = 'icecast://localhost:8000/dj.mp3'
$env:ICECAST_USER = 'source'
$env:ICECAST_PASSWORD = '<see icecast config>'  # redacted
& engine/target/release/dj-engine.exe --sidecars analyzed-real --music-root data --log-format pretty
```

(Run in background or in another shell so you can tail the log.)

- [ ] **Step 4: Watch for the new log fields**

In a separate shell:
```powershell
Get-Content -Wait <engine-log-path> | Select-String 'transition arming'
```

At the first transition, confirm the log includes `ratio_local`, `ratio_global`, `bpm_delta_pct`, `outgoing_section_bpm`, `incoming_section_bpm`. Note any transitions where `bpm_delta_pct > 0.5%` — those are the ones where the local-section refactor actually changed something.

- [ ] **Step 5: Verify post-implementation (open-ended observation)**

Spend a listening session (or have the user spend one) checking whether the "beats not aligned" complaint persists. If it does:
- Compare per-transition `downbeat_confidence` mean values. Tracks with low confidence are the next investigation target.
- If `bpm_delta_pct == 0` for every transition, the ratio refactor isn't moving anything; look at downbeat alignment or atempo slippage next.

- [ ] **Step 6: No commit for this task** — it's an observation/verification step. If you discover follow-up work, capture it in a new plan or as a memory entry.

---

## Spec-Coverage Self-Check (filled in during plan writing)

- ✅ Install madmom + pin numpy/cython → Task 1
- ✅ `detect_beat_grid.py` module → Task 2
- ✅ `detect_beats.py` thin wrapper → Task 3
- ✅ `detect_downbeats.py` thin wrapper, fallbacks deleted → Task 4
- ✅ `detect_bpm.py` derives from madmom → Task 5
- ✅ `compute_local_bpm` helper → Task 6
- ✅ Three new sidecar fields populated → Task 7
- ✅ `mixability.py` simplification → Task 8
- ✅ analyzer_version bump → Task 9
- ✅ Schema additions → Task 10
- ✅ Engine Sidecar fields + `local_bpm_in_window` → Task 11
- ✅ TransitionContext + scheduler logging → Task 12
- ✅ Recipes consume `ctx.ratio` → Task 13
- ✅ Library re-analysis + sidecar diff → Task 14
- ✅ Restart engine + verify new log fields → Task 15

Open items from the spec all converted to concrete steps in Task 1 (force-flag verification, pin-combo smoke test) and Task 8 (mixability audit + simplification).
