# Analyzer + Selector Smarter-Transitions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop the engine from defaulting to hard cuts by relaxing the analyzer's classification, replacing the crude downbeat fallback with librosa-based phrase detection, adding per-band energy, and teaching the selector + transition planner to prefer smooth transitions.

**Architecture:** Schema bumps to v2 (additive — v1 still validates). Python analyzer gains `detect_bands.py`, a librosa phrase detector inside `detect_downbeats.py`, split `is_beatmatchable` / `is_phrase_aligned` flags, and version bump 0.1.0 → 0.2.0 (forces one full re-analysis). Rust engine adds optional fields to `Sidecar`, threads `band_energy_means.bass` through `mixer → crossfade::mix_crossfade_eq`, lets `phrase.rs` prefer sidecar-set boundaries, gives the selector a transition-aware score term, and relaxes the hard-cut trigger in `transition.rs` (non-beatmatchable → 16-bar fade not hard cut; hard-cut energy threshold 0.35 → 0.50).

**Tech Stack:** Python 3.13 (analyzer), Rust stable 1.95 (engine). No new external deps. Uses existing librosa, numpy, scipy, soundfile (analyzer) and existing serde, ringbuf, tokio (engine).

**Spec:** `docs/superpowers/specs/2026-05-27-analyzer-selector-enhancement-design.md`

**Prerequisite:** Both halves built and tests green at HEAD (`ac88f56`).

---

## File structure (what this plan creates or modifies)

```
DJ-Engine/
  schema/
    sidecar.schema.json          # MODIFIED — schema_version bumps to 2
  analyzer/
    src/dj_analyze/
      __init__.py                # MODIFIED — version 0.1.0 → 0.2.0
      detect_bands.py            # NEW
      detect_downbeats.py        # MODIFIED — replace 4/4 fallback with librosa phrase detector
      mixability.py              # MODIFIED — split flags
      analyze.py                 # MODIFIED — orchestrator emits new fields
    tests/
      fixtures/
        generate_fixtures.py     # MODIFIED — add bass_only_80hz.wav + high_only_8khz.wav
        bass_only_80hz.wav       # GENERATED
        high_only_8khz.wav       # GENERATED
      test_detect_bands.py       # NEW
      test_detect_downbeats.py   # MODIFIED
      test_mixability.py         # MODIFIED
      test_analyze.py            # MODIFIED
      test_schema_validation.py  # MODIFIED
  engine/
    src/
      sidecar.rs                 # MODIFIED — new optional fields + beatmatchable() / phrase_aligned()
      phrase.rs                  # MODIFIED — prefer sidecar-set boundaries
      selector.rs                # MODIFIED — transition-aware score term
      transition.rs              # MODIFIED — relaxed hard-cut, gate use_eq_swap, return BassMixHint
      crossfade.rs               # MODIFIED — mix_crossfade_eq takes Option<BassMixHint>
      mixer.rs                   # MODIFIED — wires bass_hint through
    tests/
      sidecar_test.rs            # MODIFIED — v1 + v2 cases
      phrase_test.rs             # MODIFIED — sidecar-set case
      selector_test.rs           # MODIFIED — transition-aware
      transition_test.rs         # MODIFIED — new thresholds
      crossfade_eq_test.rs       # MODIFIED — bass_hint cases
```

---

### Task 1: Schema v1 → v2

**Files:**
- Modify: `schema/sidecar.schema.json`

- [ ] **Step 1: Update the schema**

Open `schema/sidecar.schema.json`. Apply these changes:

1. Change `"schema_version": { "const": 1 }` → `"schema_version": { "const": 2 }`

2. Add to `"properties"` (in order, keep alphabetical-ish or just append before the closing brace of `properties`):

```json
"phrase_boundaries_seconds": {
  "type": "array",
  "items": { "type": "number", "minimum": 0 }
},
"phrase_confidence": { "type": "number", "minimum": 0, "maximum": 1 },
"band_energy_curves": {
  "type": "object",
  "required": ["bass", "mid", "high"],
  "properties": {
    "bass": { "type": "array", "items": { "type": "number", "minimum": 0, "maximum": 1 } },
    "mid":  { "type": "array", "items": { "type": "number", "minimum": 0, "maximum": 1 } },
    "high": { "type": "array", "items": { "type": "number", "minimum": 0, "maximum": 1 } }
  },
  "additionalProperties": false
},
"band_energy_means": {
  "type": "object",
  "required": ["bass", "mid", "high"],
  "properties": {
    "bass": { "type": "number", "minimum": 0, "maximum": 1 },
    "mid":  { "type": "number", "minimum": 0, "maximum": 1 },
    "high": { "type": "number", "minimum": 0, "maximum": 1 }
  },
  "additionalProperties": false
},
"is_beatmatchable": { "type": "boolean" },
"is_phrase_aligned": { "type": "boolean" }
```

3. Extend the `mixable_reasons` items `enum` to include `"phrase_low_confidence"` and `"bass_only_track"`. The full enum becomes:
```
["bpm_low_confidence", "bpm_variable", "no_clear_downbeat", "too_short",
 "key_low_confidence", "phrase_low_confidence", "bass_only_track"]
```

4. Do NOT modify the `allOf` `required` list. The new fields are optional even on the no-error branch (so legacy v1 sidecars with `schema_version=2` but missing new fields still validate; the analyzer always emits them in v2).

- [ ] **Step 2: Verify the schema is itself valid JSON Schema**

```
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" -c "import json; from jsonschema import Draft202012Validator; s=json.load(open(r'C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\schema\sidecar.schema.json')); Draft202012Validator.check_schema(s); print('schema OK')"
```

Expected: `schema OK`.

- [ ] **Step 3: Commit**

```
git add schema/sidecar.schema.json
git commit -m "feat(schema): bump to v2 — bands, phrase boundaries, split mixability flags"
```

---

### Task 2: Bump analyzer version + new audio fixtures

**Files:**
- Modify: `analyzer/src/dj_analyze/__init__.py`
- Modify: `analyzer/tests/fixtures/generate_fixtures.py`
- Generated: `analyzer/tests/fixtures/bass_only_80hz.wav`
- Generated: `analyzer/tests/fixtures/high_only_8khz.wav`

- [ ] **Step 1: Bump the version**

Replace `analyzer/src/dj_analyze/__init__.py`:
```python
"""DJ Engine analyzer — emits sidecar JSONs from a directory of audio files."""

__version__ = "0.2.0"
```

- [ ] **Step 2: Add fixture generators**

Open `analyzer/tests/fixtures/generate_fixtures.py`. Append two new generator functions and add them to the `__main__` block:

```python
def bass_only_80hz(duration_s: float = 6.0) -> None:
    """80 Hz sine — purely bass-band content. Energy should sit in `bass` band only."""
    n = int(duration_s * SR)
    t = np.arange(n, dtype=np.float32) / SR
    audio = 0.5 * np.sin(2 * np.pi * 80.0 * t)
    fade = int(0.05 * SR)
    audio[:fade] *= np.linspace(0, 1, fade)
    audio[-fade:] *= np.linspace(1, 0, fade)
    _save("bass_only_80hz.wav", audio.astype(np.float32))


def high_only_8khz(duration_s: float = 6.0) -> None:
    """8 kHz sine — purely high-band content. Energy should sit in `high` band only."""
    n = int(duration_s * SR)
    t = np.arange(n, dtype=np.float32) / SR
    audio = 0.5 * np.sin(2 * np.pi * 8000.0 * t)
    fade = int(0.05 * SR)
    audio[:fade] *= np.linspace(0, 1, fade)
    audio[-fade:] *= np.linspace(1, 0, fade)
    _save("high_only_8khz.wav", audio.astype(np.float32))
```

In the `if __name__ == "__main__":` block, add these calls right before `corrupt_mp3()`:
```python
    bass_only_80hz()
    high_only_8khz()
```

- [ ] **Step 3: Run the generator**

```
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\tests\fixtures\generate_fixtures.py
```

Expected: prints "Fixtures regenerated in ...". Two new WAVs present.

- [ ] **Step 4: Verify the new fixtures exist**

```
Test-Path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\tests\fixtures\bass_only_80hz.wav
Test-Path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\tests\fixtures\high_only_8khz.wav
```

Both should return `True`.

- [ ] **Step 5: Commit**

```
git add analyzer/src/dj_analyze/__init__.py analyzer/tests/fixtures/generate_fixtures.py analyzer/tests/fixtures/bass_only_80hz.wav analyzer/tests/fixtures/high_only_8khz.wav
git commit -m "chore(analyzer): bump to 0.2.0 + add band-specific test fixtures"
```

---

### Task 3: `detect_bands.py` — per-band RMS energy

**Files:**
- Create: `analyzer/src/dj_analyze/detect_bands.py`
- Create: `analyzer/tests/test_detect_bands.py`

- [ ] **Step 1: Write the failing tests**

Create `analyzer/tests/test_detect_bands.py`:
```python
from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_bands import detect_band_energies

FIXTURES = Path(__file__).parent / "fixtures"


def test_bass_only_track_has_high_bass_low_mid_low_high() -> None:
    audio, sr = decode_audio(FIXTURES / "bass_only_80hz.wav")
    result = detect_band_energies(audio, sr)
    means = result["means"]
    assert means["bass"] > 0.5, f"expected bass mean > 0.5, got {means['bass']}"
    assert means["mid"] < 0.1, f"expected mid mean < 0.1, got {means['mid']}"
    assert means["high"] < 0.1, f"expected high mean < 0.1, got {means['high']}"


def test_high_only_track_has_high_high_low_bass_low_mid() -> None:
    audio, sr = decode_audio(FIXTURES / "high_only_8khz.wav")
    result = detect_band_energies(audio, sr)
    means = result["means"]
    assert means["high"] > 0.5, f"expected high mean > 0.5, got {means['high']}"
    assert means["mid"] < 0.1, f"expected mid mean < 0.1, got {means['mid']}"
    assert means["bass"] < 0.1, f"expected bass mean < 0.1, got {means['bass']}"


def test_curves_have_one_value_per_second() -> None:
    audio, sr = decode_audio(FIXTURES / "bass_only_80hz.wav")  # 6s fixture
    result = detect_band_energies(audio, sr)
    for band in ("bass", "mid", "high"):
        curve = result["curves"][band]
        assert 5 <= len(curve) <= 7, f"{band}: expected ~6 entries, got {len(curve)}"


def test_curve_values_in_unit_range() -> None:
    audio, sr = decode_audio(FIXTURES / "bass_only_80hz.wav")
    result = detect_band_energies(audio, sr)
    for band in ("bass", "mid", "high"):
        for v in result["curves"][band]:
            assert 0.0 <= v <= 1.0, f"{band}: value {v} out of [0, 1]"
```

- [ ] **Step 2: Run tests to confirm they fail**

```
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" -m pytest C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\tests\test_detect_bands.py -v
```

Expected: import error — `detect_bands` not found.

- [ ] **Step 3: Implement `detect_bands.py`**

Create `analyzer/src/dj_analyze/detect_bands.py`:
```python
"""Per-band (bass / mid / high) RMS energy. Bands match standard DJ EQ split."""

from __future__ import annotations

from typing import Any

import librosa
import numpy as np
from scipy.signal import butter, sosfiltfilt

# Band edges in Hz.
BASS_HIGH = 250.0
MID_LOW = 250.0
MID_HIGH = 2000.0
HIGH_LOW = 2000.0

# Filter order. 4 is a good balance for Butterworth — flat passband, ~24 dB/oct rolloff.
_FILTER_ORDER = 4


def detect_band_energies(audio: np.ndarray, sr: int) -> dict[str, Any]:
    """Return per-band RMS curves (per-second) and means, normalized to [0, 1].

    Output shape:
        {
          "curves": {"bass": [float, ...], "mid": [float, ...], "high": [float, ...]},
          "means":  {"bass": float, "mid": float, "high": float},
        }
    """
    bass = _filter_band(audio, sr, kind="lowpass", cutoff=BASS_HIGH)
    mid = _filter_band(audio, sr, kind="bandpass", low=MID_LOW, high=MID_HIGH)
    high = _filter_band(audio, sr, kind="highpass", cutoff=HIGH_LOW)

    bass_curve, bass_mean = _per_second_rms_normalized(bass, sr)
    mid_curve, mid_mean = _per_second_rms_normalized(mid, sr)
    high_curve, high_mean = _per_second_rms_normalized(high, sr)

    return {
        "curves": {"bass": bass_curve, "mid": mid_curve, "high": high_curve},
        "means": {"bass": bass_mean, "mid": mid_mean, "high": high_mean},
    }


def _filter_band(
    audio: np.ndarray,
    sr: int,
    *,
    kind: str,
    cutoff: float | None = None,
    low: float | None = None,
    high: float | None = None,
) -> np.ndarray:
    nyq = sr / 2.0
    if kind == "lowpass":
        sos = butter(_FILTER_ORDER, cutoff / nyq, btype="lowpass", output="sos")
    elif kind == "highpass":
        sos = butter(_FILTER_ORDER, cutoff / nyq, btype="highpass", output="sos")
    elif kind == "bandpass":
        sos = butter(_FILTER_ORDER, [low / nyq, high / nyq], btype="bandpass", output="sos")
    else:
        raise ValueError(f"unknown filter kind: {kind}")
    return sosfiltfilt(sos, audio).astype(np.float32)


def _per_second_rms_normalized(audio: np.ndarray, sr: int) -> tuple[list[float], float]:
    rms = librosa.feature.rms(y=audio, frame_length=sr, hop_length=sr)[0].astype(np.float64)
    if len(rms) == 0:
        return [], 0.0
    p95 = float(np.percentile(rms, 95))
    if p95 <= 0.0:
        return [0.0] * len(rms), 0.0
    normalized = np.clip(rms / p95, 0.0, 1.0)
    return [float(v) for v in normalized], float(normalized.mean())
```

- [ ] **Step 4: Run tests, verify pass**

```
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" -m pytest C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\tests\test_detect_bands.py -v
```

Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```
git add analyzer/src/dj_analyze/detect_bands.py analyzer/tests/test_detect_bands.py
git commit -m "feat(analyzer): per-band RMS energy (bass/mid/high)"
```

---

### Task 4: `detect_downbeats.py` — librosa phrase detection

**Files:**
- Modify: `analyzer/src/dj_analyze/detect_downbeats.py`
- Modify: `analyzer/tests/test_detect_downbeats.py`

- [ ] **Step 1: Add the failing tests**

Append to `analyzer/tests/test_detect_downbeats.py`:
```python
from dj_analyze.detect_downbeats import detect_phrases


def test_detect_phrases_on_clean_click_returns_high_confidence_and_boundaries() -> None:
    """Click track has a perfect repeating 1-bar pattern → should pass confidence threshold."""
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = [0.5 * i for i in range(16)]  # 16 beats at 120 BPM
    result = detect_phrases(audio, sr, beats=beats, phrase_bars=4)
    # 4-bar phrases means every 4th beat — boundaries at 0, 2, 4, 6 seconds.
    assert result.confidence > 0.3, f"expected confidence > 0.3, got {result.confidence}"
    assert len(result.boundaries) >= 2


def test_detect_phrases_on_noise_returns_low_confidence_no_boundaries() -> None:
    """White noise has no structure → confidence should be low, no boundaries."""
    import numpy as np
    rng = np.random.default_rng(seed=42)
    audio = rng.uniform(-0.5, 0.5, size=44100 * 4).astype(np.float32)  # 4s of noise
    beats = [0.5 * i for i in range(8)]
    result = detect_phrases(audio, sr=44100, beats=beats, phrase_bars=4)
    # Loose threshold — librosa may find weak similarity in random data.
    assert result.confidence < 0.6, f"expected low confidence on noise, got {result.confidence}"


def test_detect_phrases_handles_too_few_beats() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    result = detect_phrases(audio, sr, beats=[0.0, 0.5], phrase_bars=4)
    assert result.boundaries == []
    assert result.confidence == 0.0
```

Update the existing `test_madmom_path_or_fallback_returns_valid_result` test — the fallback path is being changed (no longer "every 4th beat"). Replace its body with:
```python
def test_downbeats_path_or_phrase_fallback_returns_valid_result() -> None:
    """Whichever path runs, result is a valid DownbeatResult with sorted times."""
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = [0.0 + 0.5 * i for i in range(16)]
    result = detect_downbeats(audio, sr, beats=beats)
    assert all(d1 <= d2 for d1, d2 in zip(result.downbeats, result.downbeats[1:]))
    assert result.method in {"madmom", "librosa_phrase", "fallback_4_4"}
```

Also remove (or update) any test that asserts the fallback returns `every 4th beat exactly`, since the fallback is replaced by `librosa_phrase` (or kept only as a last-resort if phrase confidence is very low).

- [ ] **Step 2: Run tests, confirm new failures**

```
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" -m pytest C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\tests\test_detect_downbeats.py -v
```

Expected: at least the new 3 tests fail (no `detect_phrases`). Existing `test_fallback_returns_every_fourth_beat` may also fail if you removed the every-4th-beat fallback.

- [ ] **Step 3: Implement `detect_phrases` and rewire `detect_downbeats`**

Open `analyzer/src/dj_analyze/detect_downbeats.py`. Add a new `PhraseResult` dataclass and a `detect_phrases` function. Modify `detect_downbeats` to call the librosa phrase path when madmom isn't available. Replace the file contents with:

```python
"""Downbeat detection.

Tries madmom (gold standard) first; falls back to a librosa-based phrase detector
that finds repetition periods via self-similarity. Both produce sorted timestamps.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

import numpy as np


@dataclass(frozen=True, slots=True)
class DownbeatResult:
    downbeats: list[float]
    method: Literal["madmom", "librosa_phrase", "fallback_4_4"]


@dataclass(frozen=True, slots=True)
class PhraseResult:
    boundaries: list[float]
    confidence: float  # 0..1; ratio of best to second-best period score


def detect_downbeats(
    audio: np.ndarray | None,
    sr: int,
    beats: list[float],
    *,
    force_fallback: bool = False,
) -> DownbeatResult:
    """Return downbeat times. Prefers madmom, then librosa phrase, then 4/4 fallback."""
    if not force_fallback and audio is not None:
        try:
            return _madmom_downbeats(audio, sr)
        except Exception:
            pass
    if audio is not None and beats:
        phrase = detect_phrases(audio, sr, beats=beats, phrase_bars=4)
        if phrase.confidence >= 0.3 and phrase.boundaries:
            return DownbeatResult(downbeats=phrase.boundaries, method="librosa_phrase")
    # Last-resort fallback: every 4th beat. Used only when librosa phrase fails too.
    return DownbeatResult(downbeats=beats[::4], method="fallback_4_4")


def detect_phrases(
    audio: np.ndarray,
    sr: int,
    beats: list[float],
    *,
    phrase_bars: int = 4,
) -> PhraseResult:
    """Find repeating phrase structure in `audio` at the beat scale.

    Returns boundaries (in seconds) and a confidence score in [0, 1] derived from
    the strongest periodicity in the beat-synchronous self-similarity matrix.
    Confidence is low (~0) when no repeating structure is found.
    """
    if len(beats) < 4 or len(audio) < sr:
        return PhraseResult(boundaries=[], confidence=0.0)

    import librosa  # local import keeps module import cheap when only used incidentally

    beat_frames = librosa.time_to_frames(np.asarray(beats), sr=sr)
    chroma = librosa.feature.chroma_cqt(y=audio, sr=sr)
    # Beat-synchronous chroma — one column per beat.
    chroma_sync = librosa.util.sync(chroma, beat_frames, aggregate=np.median)
    if chroma_sync.shape[1] < 2 * phrase_bars + 1:
        return PhraseResult(boundaries=[], confidence=0.0)

    # Self-similarity matrix on cosine distance.
    sim = librosa.segment.cross_similarity(chroma_sync, chroma_sync, mode="affinity")
    # For each candidate period P in {4, 8, 16} beats, score the mean diagonal at offset P.
    candidate_periods = [p for p in (4, 8, 16) if p < sim.shape[0]]
    if not candidate_periods:
        return PhraseResult(boundaries=[], confidence=0.0)
    scores: dict[int, float] = {}
    for p in candidate_periods:
        diag = [sim[i, i + p] for i in range(sim.shape[0] - p)]
        scores[p] = float(np.mean(diag)) if diag else 0.0

    best_period = max(scores, key=lambda k: scores[k])
    best_score = scores[best_period]
    other_scores = [scores[p] for p in scores if p != best_period]
    second = max(other_scores) if other_scores else 0.0
    if best_score <= 0.0:
        confidence = 0.0
    else:
        confidence = float(max(0.0, min(1.0, (best_score - second) / best_score)))

    # Anchor: first beat where similarity to (beat + best_period) is above the matrix median.
    median = float(np.median(sim))
    anchor = 0
    for i in range(sim.shape[0] - best_period):
        if sim[i, i + best_period] > median:
            anchor = i
            break

    boundary_indices = list(range(anchor, len(beats), best_period))
    boundaries = [beats[i] for i in boundary_indices]
    return PhraseResult(boundaries=boundaries, confidence=confidence)


def _madmom_downbeats(audio: np.ndarray, sr: int) -> DownbeatResult:
    # Imported lazily so madmom absence doesn't break this module.
    from madmom.features.beats import DBNDownBeatTrackingProcessor  # type: ignore[import-not-found]
    from madmom.features.downbeats import RNNDownBeatProcessor  # type: ignore[import-not-found]

    rnn = RNNDownBeatProcessor()
    dbn = DBNDownBeatTrackingProcessor(beats_per_bar=[3, 4], fps=100)
    activations = rnn(audio.astype(np.float32))
    beats_and_positions = dbn(activations)
    downbeats = [float(t) for t, pos in beats_and_positions if int(round(pos)) == 1]
    return DownbeatResult(downbeats=downbeats, method="madmom")
```

- [ ] **Step 4: Run tests, verify pass**

```
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" -m pytest C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\tests\test_detect_downbeats.py -v
```

Expected: all tests pass (the 3 new `detect_phrases` tests + the relaxed existing tests).

- [ ] **Step 5: Commit**

```
git add analyzer/src/dj_analyze/detect_downbeats.py analyzer/tests/test_detect_downbeats.py
git commit -m "feat(analyzer): librosa-based phrase detection replaces 4/4 fallback"
```

---

### Task 5: `mixability.py` — split flags

**Files:**
- Modify: `analyzer/src/dj_analyze/mixability.py`
- Modify: `analyzer/tests/test_mixability.py`

- [ ] **Step 1: Update the failing tests**

Replace `analyzer/tests/test_mixability.py` with:
```python
from __future__ import annotations

from dj_analyze.mixability import classify


def test_clean_track_is_beatmatchable_and_phrase_aligned() -> None:
    result = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.8,
    )
    assert result["is_beatmatchable"] is True
    assert result["is_phrase_aligned"] is True
    assert result["is_mixable"] is True
    assert result["reasons"] == []


def test_librosa_phrase_method_is_phrase_aligned() -> None:
    result = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="librosa_phrase",
        key_confidence=0.8,
    )
    assert result["is_beatmatchable"] is True
    assert result["is_phrase_aligned"] is True


def test_fallback_4_4_is_beatmatchable_but_not_phrase_aligned() -> None:
    """Crucial: the old fallback should NOT disqualify beatmatching."""
    result = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="fallback_4_4",
        key_confidence=0.8,
    )
    assert result["is_beatmatchable"] is True
    assert result["is_phrase_aligned"] is False
    assert "phrase_low_confidence" in result["reasons"]


def test_low_bpm_confidence_below_relaxed_threshold_disqualifies() -> None:
    """Below 0.4 (the new relaxed threshold) → not beatmatchable."""
    result = classify(
        bpm=120.0,
        bpm_confidence=0.3,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.8,
    )
    assert result["is_beatmatchable"] is False
    assert "bpm_low_confidence" in result["reasons"]


def test_bpm_confidence_in_grey_zone_passes() -> None:
    """0.5 used to disqualify (threshold was 0.6); should now pass (threshold is 0.4)."""
    result = classify(
        bpm=120.0,
        bpm_confidence=0.5,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.8,
    )
    assert result["is_beatmatchable"] is True


def test_too_short_disqualifies() -> None:
    result = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=60.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.8,
    )
    assert result["is_beatmatchable"] is False
    assert "too_short" in result["reasons"]


def test_low_key_confidence_is_advisory_not_disqualifying() -> None:
    result = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.2,
    )
    assert result["is_beatmatchable"] is True
    assert "key_low_confidence" in result["reasons"]
```

- [ ] **Step 2: Run tests, confirm failures**

```
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" -m pytest C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\tests\test_mixability.py -v
```

Expected: tests fail because `classify` returns a tuple (old shape), not a dict.

- [ ] **Step 3: Rewrite `mixability.py`**

Replace `analyzer/src/dj_analyze/mixability.py`:
```python
"""Classify whether a track is suitable for beatmatched DJ mixing.

Returns split flags:
  - is_beatmatchable: safe to time-stretch + crossfade
  - is_phrase_aligned: downbeats accurate enough to snap to phrase boundaries
  - is_mixable: derived alias of is_beatmatchable (kept for v1 schema compat)
"""

from __future__ import annotations

from typing import Any, Literal

# Relaxed thresholds (was 0.6, 5%; now 0.4 to match the spec).
BPM_CONFIDENCE_THRESHOLD = 0.4
MIN_DURATION_SECONDS = 90.0
KEY_CONFIDENCE_ADVISORY_THRESHOLD = 0.4


def classify(
    *,
    bpm: float,
    bpm_confidence: float,
    duration_seconds: float,
    downbeats_count: int,
    beats_count: int,
    downbeat_method: Literal["madmom", "librosa_phrase", "fallback_4_4"],
    key_confidence: float,
) -> dict[str, Any]:
    """Classify the track. See module docstring for output shape."""
    reasons: list[str] = []

    if bpm_confidence < BPM_CONFIDENCE_THRESHOLD:
        reasons.append("bpm_low_confidence")
    if duration_seconds < MIN_DURATION_SECONDS:
        reasons.append("too_short")
    if key_confidence < KEY_CONFIDENCE_ADVISORY_THRESHOLD:
        reasons.append("key_low_confidence")

    # Phrase alignment requires actual structural data — fallback_4_4 doesn't count.
    is_phrase_aligned = downbeat_method in {"madmom", "librosa_phrase"}
    if not is_phrase_aligned:
        reasons.append("phrase_low_confidence")

    # Beatmatchability is much more permissive than before.
    disqualifying = {"bpm_low_confidence", "too_short"}
    is_beatmatchable = not any(r in disqualifying for r in reasons)

    return {
        "is_beatmatchable": is_beatmatchable,
        "is_phrase_aligned": is_phrase_aligned and is_beatmatchable,
        "is_mixable": is_beatmatchable,  # alias for v1 compat
        "reasons": reasons,
    }
```

Note that `is_phrase_aligned` only counts when `is_beatmatchable` is also true — otherwise the engine would try to snap to phrases on an unplayable track. The schema v2 `is_phrase_aligned` is `false` for any non-beatmatchable track.

- [ ] **Step 4: Run tests, verify pass**

```
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" -m pytest C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\tests\test_mixability.py -v
```

Expected: all 7 tests pass.

- [ ] **Step 5: Commit**

```
git add analyzer/src/dj_analyze/mixability.py analyzer/tests/test_mixability.py
git commit -m "feat(analyzer): split is_mixable into is_beatmatchable + is_phrase_aligned"
```

---

### Task 6: `analyze.py` orchestrator — emit new fields

**Files:**
- Modify: `analyzer/src/dj_analyze/analyze.py`
- Modify: `analyzer/tests/test_analyze.py`

- [ ] **Step 1: Update the failing tests**

Append to `analyzer/tests/test_analyze.py`:
```python
def test_analyze_emits_v2_schema_fields() -> None:
    result = analyze_track(FIXTURES / "click_120bpm.wav")
    assert result["schema_version"] == 2
    assert "band_energy_curves" in result
    assert set(result["band_energy_curves"].keys()) == {"bass", "mid", "high"}
    assert "band_energy_means" in result
    assert "is_beatmatchable" in result
    assert "is_phrase_aligned" in result
    # is_mixable is still emitted as alias.
    assert result["is_mixable"] == result["is_beatmatchable"]


def test_analyze_emits_phrase_boundaries_when_aligned() -> None:
    result = analyze_track(FIXTURES / "click_120bpm.wav")
    # If aligned, boundaries should be present (and could be empty for very short tracks).
    if result.get("is_phrase_aligned"):
        assert "phrase_boundaries_seconds" in result
```

Also update the existing `test_analyze_returns_dict_with_required_fields` — add assertions for new fields to that test instead of bloating the test count:
```python
    assert "band_energy_curves" in result
    assert "is_beatmatchable" in result
    assert "is_phrase_aligned" in result
```

- [ ] **Step 2: Run tests, confirm failure**

```
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" -m pytest C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\tests\test_analyze.py -v
```

Expected: failures because `analyze.py` doesn't emit the new fields yet.

- [ ] **Step 3: Update `analyze.py`**

Open `analyzer/src/dj_analyze/analyze.py`. Replace the existing module:

```python
"""Per-track orchestration: decode + run all detectors + classify mixability.

Returns a dict matching schema/sidecar.schema.json v2.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from . import __version__
from .decode import DecodeError, decode_audio
from .detect_bands import detect_band_energies
from .detect_beats import detect_beats
from .detect_bpm import detect_bpm
from .detect_cues import detect_cues
from .detect_downbeats import detect_downbeats, detect_phrases
from .detect_energy import detect_energy
from .detect_key import detect_key
from .mixability import classify

SCHEMA_VERSION = 2


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

    # Always compute phrase boundaries explicitly so we can emit them + confidence.
    phrase_result = detect_phrases(audio, sr, beats=beats, phrase_bars=4) if beats else None

    key_result = detect_key(audio, sr)
    energy_curve, energy_mean = detect_energy(audio, sr)
    bands = detect_band_energies(audio, sr)
    cues = detect_cues(
        duration_seconds=duration,
        beats=beats,
        downbeats=db_result.downbeats,
        energy_curve=energy_curve,
        bpm=bpm,
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
        "energy_curve": energy_curve,
        "energy_mean": float(energy_mean),
        "band_energy_curves": bands["curves"],
        "band_energy_means": bands["means"],
        **cues,
        "is_beatmatchable": mix["is_beatmatchable"],
        "is_phrase_aligned": mix["is_phrase_aligned"],
        "is_mixable": mix["is_mixable"],
        "mixable_reasons": mix["reasons"],
    }

    # Only emit phrase_boundaries_seconds + phrase_confidence when we have meaningful data.
    if phrase_result is not None:
        out["phrase_confidence"] = float(phrase_result.confidence)
        if mix["is_phrase_aligned"] and phrase_result.boundaries:
            out["phrase_boundaries_seconds"] = phrase_result.boundaries

    return out
```

- [ ] **Step 4: Run tests, verify pass**

```
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" -m pytest C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\tests\test_analyze.py -v
```

Expected: all tests pass (the existing 3 plus 2 new ones).

- [ ] **Step 5: Commit**

```
git add analyzer/src/dj_analyze/analyze.py analyzer/tests/test_analyze.py
git commit -m "feat(analyzer): orchestrator emits schema v2 fields"
```

---

### Task 7: Schema validation tests

**Files:**
- Modify: `analyzer/tests/test_schema_validation.py`

- [ ] **Step 1: Update the test file**

Replace `analyzer/tests/test_schema_validation.py`:
```python
from __future__ import annotations

import json
from pathlib import Path

import pytest
from jsonschema import Draft202012Validator

from dj_analyze.analyze import analyze_track

FIXTURES = Path(__file__).parent / "fixtures"
SCHEMA_PATH = Path(__file__).parents[2] / "schema" / "sidecar.schema.json"


@pytest.fixture(scope="module")
def validator() -> Draft202012Validator:
    schema = json.loads(SCHEMA_PATH.read_text())
    return Draft202012Validator(schema)


def test_v2_sidecar_validates(validator: Draft202012Validator) -> None:
    result = analyze_track(FIXTURES / "click_120bpm.wav")
    errors = list(validator.iter_errors(result))
    assert not errors, "Schema errors:\n" + "\n".join(e.message for e in errors)


def test_corrupt_sidecar_validates(validator: Draft202012Validator) -> None:
    """Error sidecars carry only the base fields + error."""
    result = analyze_track(FIXTURES / "corrupt.mp3")
    errors = list(validator.iter_errors(result))
    assert not errors, "Schema errors:\n" + "\n".join(e.message for e in errors)


def test_bass_only_sidecar_validates(validator: Draft202012Validator) -> None:
    result = analyze_track(FIXTURES / "bass_only_80hz.wav")
    errors = list(validator.iter_errors(result))
    assert not errors, "Schema errors:\n" + "\n".join(e.message for e in errors)


def test_legacy_v1_minimal_sidecar_validates_via_only_required_fields(validator: Draft202012Validator) -> None:
    """A v1-style sidecar that happens to set schema_version=2 should still validate
    as long as required fields are present; new fields are optional."""
    minimal = {
        "schema_version": 2,
        "analyzer_version": "0.1.0",
        "source_path": "x",
        "source_mtime": 0.0,
        "source_size_bytes": 0,
        "duration_seconds": 200.0,
        "sample_rate": 44100,
        "bpm": 120.0,
        "bpm_confidence": 0.9,
        "beats_seconds": [0.0],
        "downbeats_seconds": [0.0],
        "key": "8A",
        "key_label": "A minor",
        "key_confidence": 0.8,
        "energy_curve": [0.4],
        "energy_mean": 0.4,
        "cue_in_seconds": 0.0,
        "cue_out_seconds": 195.0,
        "intro_end_seconds": 16.0,
        "outro_start_seconds": 180.0,
        "is_mixable": True,
        "mixable_reasons": [],
    }
    errors = list(validator.iter_errors(minimal))
    assert not errors, "Minimal-fields sidecar should validate:\n" + "\n".join(e.message for e in errors)
```

- [ ] **Step 2: Run tests**

```
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" -m pytest C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\tests\test_schema_validation.py -v
```

Expected: 4 tests pass.

- [ ] **Step 3: Run the full analyzer test suite**

```
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" -m pytest C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\tests -v
```

Expected: all tests green.

- [ ] **Step 4: Commit**

```
git add analyzer/tests/test_schema_validation.py
git commit -m "test(analyzer): schema validation covers v2 + legacy-minimal sidecars"
```

---

### Task 8: Engine `sidecar.rs` — new optional fields

**Files:**
- Modify: `engine/src/sidecar.rs`
- Modify: `engine/tests/sidecar_test.rs`

- [ ] **Step 1: Update the failing tests**

Append to `engine/tests/sidecar_test.rs`:
```rust
const V2_JSON: &str = r#"{
    "schema_version": 2,
    "analyzer_version": "0.2.0",
    "source_path": "/music/a.mp3",
    "source_mtime": 0.0,
    "source_size_bytes": 1000,
    "duration_seconds": 200.0,
    "sample_rate": 44100,
    "bpm": 120.0,
    "bpm_confidence": 0.9,
    "beats_seconds": [0.0],
    "downbeats_seconds": [0.0],
    "key": "8A",
    "key_label": "A minor",
    "key_confidence": 0.8,
    "energy_curve": [0.4],
    "energy_mean": 0.4,
    "cue_in_seconds": 0.0,
    "cue_out_seconds": 195.0,
    "intro_end_seconds": 16.0,
    "outro_start_seconds": 180.0,
    "is_mixable": true,
    "mixable_reasons": [],
    "phrase_boundaries_seconds": [10.0, 26.0, 42.0],
    "phrase_confidence": 0.72,
    "band_energy_curves": {"bass": [0.5], "mid": [0.3], "high": [0.2]},
    "band_energy_means": {"bass": 0.55, "mid": 0.36, "high": 0.21},
    "is_beatmatchable": true,
    "is_phrase_aligned": true
}"#;

#[test]
fn parses_v2_sidecar() {
    let sc: Sidecar = serde_json::from_str(V2_JSON).unwrap();
    assert_eq!(sc.schema_version, 2);
    assert_eq!(sc.is_beatmatchable, Some(true));
    assert_eq!(sc.is_phrase_aligned, Some(true));
    assert_eq!(sc.phrase_boundaries_seconds.as_deref(), Some(&[10.0, 26.0, 42.0][..]));
    assert!(sc.band_energy_means.is_some());
}

#[test]
fn v2_beatmatchable_uses_new_field() {
    let sc: Sidecar = serde_json::from_str(V2_JSON).unwrap();
    assert!(sc.beatmatchable());
    assert!(sc.phrase_aligned());
}

#[test]
fn v1_beatmatchable_falls_back_to_is_mixable() {
    // VALID_JSON (the existing v1 const) has is_mixable=true but no is_beatmatchable.
    let sc: Sidecar = serde_json::from_str(VALID_JSON).unwrap();
    assert!(sc.beatmatchable(), "v1 with is_mixable=true should be beatmatchable");
    assert!(!sc.phrase_aligned(), "v1 should default to NOT phrase_aligned");
}

#[test]
fn beatmatchable_false_when_both_absent() {
    let json = r#"{
        "schema_version": 2,
        "analyzer_version": "0.2.0",
        "source_path": "/x.mp3",
        "source_mtime": 0.0,
        "source_size_bytes": 0,
        "error": "decode failed"
    }"#;
    let sc: Sidecar = serde_json::from_str(json).unwrap();
    assert!(!sc.beatmatchable());
    assert!(!sc.phrase_aligned());
}
```

- [ ] **Step 2: Run tests, confirm failure**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test sidecar_test
```

Expected: compile errors — new fields and methods don't exist.

- [ ] **Step 3: Update `sidecar.rs`**

Open `engine/src/sidecar.rs`. Add the band sub-structs, the new optional fields, and the helper methods. Replace the file contents with:

```rust
//! Serde types matching schema/sidecar.schema.json (v1 + v2).
//!
//! All v2-only fields are Option<...>; v1 sidecars deserialize cleanly with those
//! fields left as None. The `beatmatchable()` / `phrase_aligned()` helpers paper
//! over the v1/v2 difference for callers.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BandValues {
    pub bass: f64,
    pub mid: f64,
    pub high: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BandCurves {
    pub bass: Vec<f64>,
    pub mid: Vec<f64>,
    pub high: Vec<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Sidecar {
    pub schema_version: u32,
    pub analyzer_version: String,
    pub source_path: String,
    pub source_mtime: f64,
    pub source_size_bytes: u64,

    #[serde(default)]
    pub error: Option<String>,

    pub duration_seconds: Option<f64>,
    pub sample_rate: Option<u32>,

    pub bpm: Option<f64>,
    pub bpm_confidence: Option<f64>,
    #[serde(default)]
    pub beats_seconds: Vec<f64>,
    #[serde(default)]
    pub downbeats_seconds: Vec<f64>,

    pub key: Option<String>,
    pub key_label: Option<String>,
    pub key_confidence: Option<f64>,

    #[serde(default)]
    pub energy_curve: Vec<f64>,
    pub energy_mean: Option<f64>,

    pub cue_in_seconds: Option<f64>,
    pub cue_out_seconds: Option<f64>,
    pub intro_end_seconds: Option<f64>,
    pub outro_start_seconds: Option<f64>,

    pub is_mixable: Option<bool>,
    #[serde(default)]
    pub mixable_reasons: Vec<String>,

    // v2 additions
    pub phrase_boundaries_seconds: Option<Vec<f64>>,
    pub phrase_confidence: Option<f64>,
    pub band_energy_curves: Option<BandCurves>,
    pub band_energy_means: Option<BandValues>,
    pub is_beatmatchable: Option<bool>,
    pub is_phrase_aligned: Option<bool>,
}

impl Sidecar {
    pub fn is_playable(&self) -> bool {
        self.error.is_none()
            && self.bpm.is_some()
            && self.key.is_some()
            && self.cue_in_seconds.is_some()
            && self.cue_out_seconds.is_some()
            && self.intro_end_seconds.is_some()
            && self.outro_start_seconds.is_some()
    }

    /// True if this track is safe to time-stretch + EQ-swap crossfade.
    /// v2 uses is_beatmatchable; v1 falls back to is_mixable.
    pub fn beatmatchable(&self) -> bool {
        self.is_beatmatchable
            .or(self.is_mixable)
            .unwrap_or(false)
    }

    /// True if downbeats are accurate enough to snap to phrase boundaries.
    /// v2 uses is_phrase_aligned; v1 conservatively returns false.
    pub fn phrase_aligned(&self) -> bool {
        self.is_phrase_aligned.unwrap_or(false)
    }
}
```

- [ ] **Step 4: Run tests, verify pass**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test sidecar_test
```

Expected: 7 tests pass (3 existing + 4 new).

- [ ] **Step 5: Commit**

```
git add engine/src/sidecar.rs engine/tests/sidecar_test.rs
git commit -m "feat(engine): sidecar v2 fields + beatmatchable/phrase_aligned helpers"
```

---

### Task 9: `phrase.rs` — prefer sidecar-set boundaries

**Files:**
- Modify: `engine/src/phrase.rs`
- Modify: `engine/tests/phrase_test.rs`

- [ ] **Step 1: Update the failing tests**

Append to `engine/tests/phrase_test.rs`:
```rust
#[test]
fn uses_sidecar_phrase_boundaries_when_set() {
    let mut sc = make_sc(vec![0.0, 1.0, 2.0, 3.0], 0.0, 60.0);
    sc.phrase_boundaries_seconds = Some(vec![5.0, 10.0, 15.0]);
    sc.is_phrase_aligned = Some(true);
    let bounds = phrase_boundaries(&sc, 16);
    assert_eq!(bounds, vec![5.0, 10.0, 15.0]);
}

#[test]
fn empty_sidecar_phrase_boundaries_falls_back_to_heuristic() {
    let mut sc = make_sc(vec![0.0, 1.0, 2.0, 3.0], 0.0, 60.0);
    sc.phrase_boundaries_seconds = Some(vec![]);
    // The heuristic should kick in and produce [0.0] (anchor at intro_end=0).
    let bounds = phrase_boundaries(&sc, 4);
    assert_eq!(bounds, vec![0.0]);
}
```

- [ ] **Step 2: Run tests, confirm failure**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test phrase_test
```

Expected: 2 new failures. Existing 6 still pass.

- [ ] **Step 3: Update `phrase.rs`**

Open `engine/src/phrase.rs`. At the top of the existing `phrase_boundaries` function, add the prefer-sidecar branch. Replace the function with:

```rust
pub fn phrase_boundaries(sc: &Sidecar, phrase_bars: usize) -> Vec<f64> {
    // Prefer explicit sidecar-set boundaries (v2 analyzer with phrase confidence above threshold).
    if let Some(explicit) = &sc.phrase_boundaries_seconds {
        if !explicit.is_empty() {
            return explicit.clone();
        }
    }
    // Fallback heuristic: anchor on first downbeat at/after intro_end, step by phrase_bars.
    if phrase_bars == 0 || sc.downbeats_seconds.is_empty() {
        return Vec::new();
    }
    let intro_end = sc.intro_end_seconds.unwrap_or(0.0);
    let outro_start = sc.outro_start_seconds.unwrap_or(f64::INFINITY);
    let anchor_idx = sc
        .downbeats_seconds
        .iter()
        .position(|d| *d >= intro_end)
        .unwrap_or(sc.downbeats_seconds.len());
    let mut out = Vec::new();
    let mut idx = anchor_idx;
    while idx < sc.downbeats_seconds.len() {
        let t = sc.downbeats_seconds[idx];
        if t >= outro_start {
            break;
        }
        out.push(t);
        idx += phrase_bars;
    }
    out
}
```

- [ ] **Step 4: Run tests, verify pass**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test phrase_test
```

Expected: 8 tests pass (6 existing + 2 new).

- [ ] **Step 5: Commit**

```
git add engine/src/phrase.rs engine/tests/phrase_test.rs
git commit -m "feat(engine): phrase.rs prefers sidecar-set boundaries when available"
```

---

### Task 10: `transition.rs` — relaxed hard-cut + use_eq_swap gating + BassMixHint

**Files:**
- Modify: `engine/src/transition.rs`
- Modify: `engine/tests/transition_test.rs`

- [ ] **Step 1: Add the failing tests**

Open `engine/tests/transition_test.rs`. First, update the existing `sc()` helper at the top of the file to take and set the new fields (these are needed for the new tests). Replace the existing helper with:

```rust
fn sc(
    bpm: f64,
    key: &str,
    energy: f64,
    mixable: bool,
    downbeats: Vec<f64>,
    intro_end: f64,
    outro_start: f64,
) -> Sidecar {
    sc_v2(
        bpm,
        key,
        energy,
        mixable,
        true,  // is_phrase_aligned defaults to true for old tests
        downbeats,
        intro_end,
        outro_start,
    )
}

fn sc_v2(
    bpm: f64,
    key: &str,
    energy: f64,
    beatmatchable: bool,
    phrase_aligned: bool,
    downbeats: Vec<f64>,
    intro_end: f64,
    outro_start: f64,
) -> Sidecar {
    Sidecar {
        schema_version: 2,
        analyzer_version: "0.2.0".into(),
        source_path: "x".into(),
        source_mtime: 0.0,
        source_size_bytes: 0,
        error: None,
        duration_seconds: Some(200.0),
        sample_rate: Some(44100),
        bpm: Some(bpm),
        bpm_confidence: Some(0.9),
        beats_seconds: vec![],
        downbeats_seconds: downbeats,
        key: Some(key.into()),
        key_label: Some("".into()),
        key_confidence: Some(0.8),
        energy_curve: vec![],
        energy_mean: Some(energy),
        cue_in_seconds: Some(0.0),
        cue_out_seconds: Some(200.0),
        intro_end_seconds: Some(intro_end),
        outro_start_seconds: Some(outro_start),
        is_mixable: Some(beatmatchable),
        mixable_reasons: vec![],
        phrase_boundaries_seconds: None,
        phrase_confidence: None,
        band_energy_curves: None,
        band_energy_means: None,
        is_beatmatchable: Some(beatmatchable),
        is_phrase_aligned: Some(phrase_aligned),
    }
}
```

Now append new tests covering the relaxed thresholds:

```rust
#[test]
fn non_beatmatchable_incoming_no_longer_hard_cuts() {
    let out = sc(120.0, "8A", 0.4, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc_v2(120.0, "8A", 0.4, false, false, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 16, "non-beatmatchable should give 16-bar fade, not hard cut");
    assert!(!plan.use_eq_swap, "non-beatmatchable should NOT use EQ swap");
}

#[test]
fn energy_delta_below_new_hard_cut_threshold_gets_quick_cut() {
    // delta = 0.40, between QUICK_CUT (0.20) and new HARD_CUT (0.50)
    let out = sc(120.0, "8A", 0.2, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc(120.0, "8A", 0.6, true, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 8, "delta 0.40 should be quick cut not hard cut");
}

#[test]
fn energy_delta_above_new_hard_cut_threshold_hard_cuts() {
    // delta = 0.55, above new HARD_CUT (0.50)
    let out = sc(120.0, "8A", 0.1, true, dense_downbeats(), 0.0, 80.0);
    let inc = sc(120.0, "8A", 0.65, true, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 0);
}

#[test]
fn beatmatchable_but_not_phrase_aligned_skips_eq_swap() {
    let out = sc_v2(120.0, "8A", 0.4, true, false, dense_downbeats(), 0.0, 80.0);
    let inc = sc_v2(120.0, "8A", 0.4, true, false, dense_downbeats(), 0.0, 80.0);
    let plan = plan_transition(&out, &inc);
    assert_eq!(plan.length_bars, 32, "still gets long blend");
    assert!(!plan.use_eq_swap, "non-phrase-aligned should NOT EQ swap even on long blend");
}
```

Note that some EXISTING tests reference `length_bars=0` for hard cut on `is_mixable=false`. The semantics have changed: those tests need updating. Specifically:
- `non_mixable_incoming_triggers_hard_cut` → rename + change expectation: non-beatmatchable now triggers 16-bar fade. Already covered by `non_beatmatchable_incoming_no_longer_hard_cuts` above, so DELETE the old test.
- `big_energy_jump_triggers_hard_cut` already uses delta = 0.7 which is > 0.50 — still passes.

- [ ] **Step 2: Update the existing imports in the test file**

At the top of `engine/tests/transition_test.rs`, ensure imports include the Sidecar struct:
```rust
use dj_engine::sidecar::Sidecar;
use dj_engine::transition::{plan_transition, TransitionPlan};
```

(They should already be there from prior tasks.)

- [ ] **Step 3: Run tests, confirm new failures + identify existing test to remove**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test transition_test
```

Expected: 4 new tests fail (relaxed thresholds not yet wired). Also the old `non_mixable_incoming_triggers_hard_cut` test may still pass on the current code but will fail once we relax the thresholds — delete it now from `transition_test.rs` if it exists.

- [ ] **Step 4: Update `transition.rs`**

Open `engine/src/transition.rs`. Modify `pick_length_and_variant` and the `plan_transition` function to use the new flags + thresholds. Replace the relevant section with:

```rust
// Tunable thresholds. Adjust by ear after listening to several hours of output.
const HARD_CUT_ENERGY_DELTA: f64 = 0.50;       // was 0.35
const QUICK_CUT_ENERGY_DELTA: f64 = 0.20;
const LONG_BLEND_BPM_DELTA: f64 = 0.02;
const LONG_BLEND_ENERGY_DELTA: f64 = 0.10;
const PHRASE_BARS: usize = 16;

pub fn plan_transition(outgoing: &Sidecar, incoming: &Sidecar) -> TransitionPlan {
    let (length_bars, use_eq_swap) = pick_length_and_variant(outgoing, incoming);

    let raw_outgoing_start = outgoing.outro_start_seconds.unwrap_or(0.0);
    let raw_incoming_start = incoming.intro_end_seconds.unwrap_or(0.0);

    let outgoing_boundaries = phrase_boundaries(outgoing, PHRASE_BARS);
    let incoming_boundaries = phrase_boundaries(incoming, PHRASE_BARS);

    let outgoing_crossfade_start_seconds =
        nearest(&outgoing_boundaries, raw_outgoing_start).unwrap_or(raw_outgoing_start);

    let incoming_crossfade_start_seconds = incoming_boundaries
        .first()
        .copied()
        .unwrap_or(raw_incoming_start);

    TransitionPlan {
        outgoing_crossfade_start_seconds,
        incoming_crossfade_start_seconds,
        length_bars,
        use_eq_swap,
    }
}

fn pick_length_and_variant(out: &Sidecar, inc: &Sidecar) -> (usize, bool) {
    let is_beatmatchable = inc.beatmatchable();
    let is_phrase_aligned = inc.phrase_aligned();

    // Non-beatmatchable → safe 16-bar volume crossfade, no EQ, no hard cut.
    if !is_beatmatchable {
        return (16, false);
    }

    let out_bpm = out.bpm.unwrap_or(120.0);
    let inc_bpm = inc.bpm.unwrap_or(out_bpm);
    let bpm_delta = ((inc_bpm - out_bpm).abs() / out_bpm.max(1.0)).abs();

    let out_energy = out.energy_mean.unwrap_or(0.5);
    let inc_energy = inc.energy_mean.unwrap_or(out_energy);
    let energy_delta = (inc_energy - out_energy).abs();

    let key_match = match (
        out.key.as_deref().and_then(CamelotKey::parse),
        inc.key.as_deref().and_then(CamelotKey::parse),
    ) {
        (Some(a), Some(b)) => a.compatibility(&b),
        _ => KeyMatch::None,
    };

    let bars = if energy_delta > HARD_CUT_ENERGY_DELTA {
        0
    } else if energy_delta > QUICK_CUT_ENERGY_DELTA
        || matches!(key_match, KeyMatch::Relative | KeyMatch::None)
    {
        8
    } else if bpm_delta <= LONG_BLEND_BPM_DELTA
        && energy_delta <= LONG_BLEND_ENERGY_DELTA
        && matches!(key_match, KeyMatch::Identical)
    {
        32
    } else {
        16
    };

    // EQ swap gated on phrase alignment (so non-aligned blends are clean volume crossfades).
    let use_eq_swap = bars >= 8 && is_phrase_aligned;
    (bars, use_eq_swap)
}
```

- [ ] **Step 5: Run tests, verify pass**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test transition_test
```

Expected: all transition tests pass.

- [ ] **Step 6: Commit**

```
git add engine/src/transition.rs engine/tests/transition_test.rs
git commit -m "feat(engine): relax hard-cut trigger, gate EQ swap on phrase alignment"
```

---

### Task 11: `selector.rs` — transition-aware scoring

**Files:**
- Modify: `engine/src/selector.rs`
- Modify: `engine/tests/selector_test.rs`

- [ ] **Step 1: Add the failing test**

Append to `engine/tests/selector_test.rs`. (Re-use the existing `track()` helper.)

```rust
#[test]
fn prefers_smoother_transition_when_other_axes_tied() {
    // Two candidates that score identically on BPM/key/energy but differ wildly
    // in expected transition: one would produce a long blend, the other a hard cut.
    let library = vec![
        track("a", 120.0, "8A", 0.4, true),
        // "b": identical → long blend with current.
        track("b", 120.0, "8A", 0.4, true),
        // "c": same BPM/key/energy as b... but we force a huge energy gap into c by NOT setting
        // energy_mean — wait, that won't work; instead force is_mixable=false on c so transition
        // would be 16-bar fade (lower transition_score) instead of 32-bar long blend.
        track("c", 120.0, "8A", 0.4, false),
    ];
    let recency = RecencyRing::new(20);
    // Use a target_energy that scores both b and c equally on the energy axis.
    let mut sel = Selector::new(&library, &recency, 0.4);
    let current = &library[0];

    // Pick repeatedly; b should be chosen more often than c because b would produce a
    // long-blend transition (higher transition_score) while c would produce a 16-bar fade.
    let mut b_wins = 0;
    let mut c_wins = 0;
    for _ in 0..200 {
        let pick = sel.pick_next(current).unwrap();
        if pick.sidecar.source_path == "b" {
            b_wins += 1;
        } else if pick.sidecar.source_path == "c" {
            c_wins += 1;
        }
    }
    assert!(b_wins > c_wins, "expected b (long blend) > c (fade); got b={} c={}", b_wins, c_wins);
}
```

- [ ] **Step 2: Run tests, confirm failure**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test selector_test
```

Expected: the new test fails (current selector doesn't bias toward smoother transitions; picks should be ~50/50).

- [ ] **Step 3: Update `selector.rs`**

Open `engine/src/selector.rs`. Modify the `score` function to add the transition-aware term. The required change is in the body of `fn score(&self, current: &LoadedTrack, candidate: &LoadedTrack) -> f64`. Replace it with:

```rust
fn score(&self, current: &LoadedTrack, candidate: &LoadedTrack) -> f64 {
    let current_bpm = current.sidecar.bpm.unwrap_or(120.0);
    let cand_bpm = candidate.sidecar.bpm.unwrap_or(120.0);
    let rel = (cand_bpm - current_bpm).abs() / current_bpm.max(1.0);
    let bpm_score = (1.0 - rel / BPM_RELAX_TIGHT).max(0.0);

    let key_score = match (
        current.sidecar.key.as_deref().and_then(CamelotKey::parse),
        candidate.sidecar.key.as_deref().and_then(CamelotKey::parse),
    ) {
        (Some(a), Some(b)) => a.compatibility(&b).score(),
        _ => 0.0,
    };

    let cand_energy = candidate.sidecar.energy_mean.unwrap_or(0.5);
    let energy_score = (1.0 - (cand_energy - self.target_energy).abs() / 0.15).max(0.0);

    // Transition-aware term — look ahead at what plan_transition would produce.
    let transition_score = match crate::transition::plan_transition(
        &current.sidecar,
        &candidate.sidecar,
    )
    .length_bars
    {
        32 => 1.0,
        16 => 0.85,
        8 => 0.6,
        0 => 0.2,
        _ => 0.5,
    };

    bpm_score * 1.0 + key_score * 1.0 + energy_score * 0.5 + transition_score * 1.0
}
```

(The rest of `Selector` — filter, fallback chain, pick_next — remains unchanged.)

- [ ] **Step 4: Run tests, verify pass**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test selector_test
```

Expected: all selector tests pass.

- [ ] **Step 5: Commit**

```
git add engine/src/selector.rs engine/tests/selector_test.rs
git commit -m "feat(engine): selector adds transition-aware score term"
```

---

### Task 12: `crossfade.rs::mix_crossfade_eq` — bass_hint parameter

**Files:**
- Modify: `engine/src/crossfade.rs`
- Modify: `engine/tests/crossfade_eq_test.rs`

- [ ] **Step 1: Add the failing tests**

Append to `engine/tests/crossfade_eq_test.rs`:
```rust
use dj_engine::crossfade::BassMixHint;

#[test]
fn no_bass_hint_matches_previous_behavior() {
    // Pre-bass_hint signature equivalent: pass None and expect old default window result.
    let sr = 44100.0;
    let frames = sr as usize;
    let outgoing = sine_stereo(440.0, frames, sr);
    let incoming = sine_stereo(523.0, frames, sr);
    let mixed = mix_crossfade_eq(&outgoing, &incoming, frames, sr, None);
    assert_eq!(mixed.len(), frames * 2);
}

#[test]
fn both_bass_light_skips_eq_swap() {
    let sr = 44100.0;
    let frames = sr as usize;
    let outgoing = sine_stereo(440.0, frames, sr);
    let incoming = sine_stereo(523.0, frames, sr);
    let hint = Some(BassMixHint {
        outgoing_bass_mean: 0.1,
        incoming_bass_mean: 0.2,
    });
    let mixed_with_hint = mix_crossfade_eq(&outgoing, &incoming, frames, sr, hint);
    let mixed_plain = dj_engine::crossfade::mix_crossfade(&outgoing, &incoming, frames);
    // Both should match the pure equal-power crossfade exactly when bass is low.
    for (a, b) in mixed_with_hint.iter().zip(mixed_plain.iter()) {
        assert!((a - b).abs() < 1e-5, "expected identical output; got {} vs {}", a, b);
    }
}

#[test]
fn both_bass_heavy_uses_earlier_swap_window() {
    let sr = 44100.0;
    let frames = sr as usize;
    let outgoing = sine_stereo(80.0, frames, sr);   // bass
    let incoming = sine_stereo(80.0, frames, sr);
    let hint = Some(BassMixHint {
        outgoing_bass_mean: 0.7,
        incoming_bass_mean: 0.7,
    });
    let mixed_early = mix_crossfade_eq(&outgoing, &incoming, frames, sr, hint);
    let mixed_default = mix_crossfade_eq(&outgoing, &incoming, frames, sr, None);
    // Outputs should differ — at frame 25% mark, early-window has already kicked in but default has not.
    let q1 = (frames / 4) * 2;
    assert!(
        (mixed_early[q1] - mixed_default[q1]).abs() > 1e-4,
        "expected difference at 25% mark with early swap window"
    );
}
```

- [ ] **Step 2: Run tests, confirm compile failure**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test crossfade_eq_test
```

Expected: compile error — `BassMixHint` and the new signature don't exist.

- [ ] **Step 3: Update `crossfade.rs`**

Open `engine/src/crossfade.rs`. Add the `BassMixHint` struct and update `mix_crossfade_eq` to take it:

At the top (after the existing `use` lines), add:
```rust
#[derive(Debug, Clone, Copy)]
pub struct BassMixHint {
    pub outgoing_bass_mean: f64,
    pub incoming_bass_mean: f64,
}
```

Replace the `mix_crossfade_eq` function:
```rust
pub fn mix_crossfade_eq(
    outgoing: &[f32],
    incoming: &[f32],
    crossfade_frames: usize,
    sample_rate: f64,
    bass_hint: Option<BassMixHint>,
) -> Vec<f32> {
    // If both tracks are bass-light, the bass-clash problem doesn't exist — pure equal-power
    // crossfade sounds cleaner than running biquads on near-empty signals.
    if let Some(h) = bass_hint {
        if h.outgoing_bass_mean < 0.3 && h.incoming_bass_mean < 0.3 {
            return mix_crossfade(outgoing, incoming, crossfade_frames);
        }
    }

    // Choose swap-window timing based on bass content.
    let (swap_start, swap_end) = match bass_hint {
        Some(h) if h.outgoing_bass_mean > 0.5 && h.incoming_bass_mean > 0.5 => (0.25, 0.40),
        _ => (0.375, 0.5),
    };

    let mut outgoing_filtered = outgoing.to_vec();
    let mut incoming_filtered = incoming.to_vec();

    // Outgoing: lowpass sweeps from 22 kHz → 200 Hz across [swap_start, swap_end].
    apply_swept_filter(
        &mut outgoing_filtered,
        FilterKind::LowPass,
        sample_rate,
        |t| sweep_log(t, swap_start, swap_end, 22_000.0, 200.0, true),
    );

    // Incoming: highpass sweeps from 200 Hz → 22 kHz on the mirror window after swap_end.
    let in_swap_start = swap_end;
    let in_swap_end = (swap_end + (swap_end - swap_start)).min(1.0);
    apply_swept_filter(
        &mut incoming_filtered,
        FilterKind::HighPass,
        sample_rate,
        |t| sweep_log(t, in_swap_start, in_swap_end, 200.0, 22_000.0, false),
    );

    mix_crossfade(&outgoing_filtered, &incoming_filtered, crossfade_frames)
}

fn sweep_log(
    t: f64,
    start: f64,
    end: f64,
    from_hz: f64,
    to_hz: f64,
    plateau_after: bool,
) -> f64 {
    if t <= start {
        if plateau_after { from_hz } else { from_hz }
    } else if t >= end {
        if plateau_after { to_hz } else { to_hz }
    } else {
        let local = (t - start) / (end - start);
        let log_from = from_hz.log10();
        let log_to = to_hz.log10();
        10.0_f64.powf(log_from + (log_to - log_from) * local)
    }
}
```

(The `sweep_log` helper consolidates the inline lowpass/highpass closures from the old implementation; behavior matches the old `t ∈ [0.375, 0.5]` and `t ∈ [0.5, 0.625]` ranges when `bass_hint=None`.)

- [ ] **Step 4: Run tests, verify pass**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --test crossfade_eq_test
```

Expected: all crossfade_eq tests pass (the 3 existing + 3 new).

- [ ] **Step 5: Commit**

```
git add engine/src/crossfade.rs engine/tests/crossfade_eq_test.rs
git commit -m "feat(engine): mix_crossfade_eq accepts optional BassMixHint"
```

---

### Task 13: `mixer.rs` — wire BassMixHint through

**Files:**
- Modify: `engine/src/mixer.rs`

- [ ] **Step 1: Pass the bass hint to mix_crossfade_eq**

Open `engine/src/mixer.rs`. In `execute_crossfade`, locate the section that calls `mix_crossfade_eq`. Currently:

```rust
let mixed = if plan.use_eq_swap {
    mix_crossfade_eq(
        &outgoing_tail,
        &incoming_head,
        crossfade_frames,
        SAMPLE_RATE as f64,
    )
} else {
    mix_crossfade(&outgoing_tail, &incoming_head, crossfade_frames)
};
```

Replace with:

```rust
let bass_hint = match (
    outgoing.sidecar.band_energy_means.as_ref(),
    incoming.sidecar.band_energy_means.as_ref(),
) {
    (Some(o), Some(i)) => Some(crate::crossfade::BassMixHint {
        outgoing_bass_mean: o.bass,
        incoming_bass_mean: i.bass,
    }),
    _ => None,
};

let mixed = if plan.use_eq_swap {
    mix_crossfade_eq(
        &outgoing_tail,
        &incoming_head,
        crossfade_frames,
        SAMPLE_RATE as f64,
        bass_hint,
    )
} else {
    mix_crossfade(&outgoing_tail, &incoming_head, crossfade_frames)
};
```

- [ ] **Step 2: Build to verify**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" build --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml
```

Expected: success.

- [ ] **Step 3: Run full test suite to confirm no regressions**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```
git add engine/src/mixer.rs
git commit -m "feat(engine): mixer wires bass_hint into mix_crossfade_eq"
```

---

### Task 14: cargo fmt + clippy

**Files:** none (tooling)

- [ ] **Step 1: Format**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" fmt --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml
```

- [ ] **Step 2: Clippy**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" clippy --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml --all-targets -- -D warnings
```

Fix every warning. Likely categories:
- `needless_borrow`, `redundant_clone`, `useless_conversion` — one-line fixes
- Unused imports (the `sweep_log` consolidation may make some old imports redundant)

- [ ] **Step 3: Full test sweep**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" test --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml
```

Expected: all tests pass.

Run analyzer tests too:
```
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" -m pytest C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\tests -v
```

Expected: all pass.

Run lint on analyzer:
```
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" -m ruff check C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\src C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\tests
```

Fix any issues. Then:
```
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" -m mypy C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\src
```

Fix any new mypy errors (likely on scipy imports — add `# type: ignore[import-untyped]` as needed; scipy stubs aren't always present).

- [ ] **Step 4: Commit (if anything changed)**

```
git add -A
git commit -m "chore: satisfy lint/clippy for analyzer + engine smarter-transitions changes"
```

---

### Task 15: End-to-end smoke test

**Files:** none (manual procedure)

- [ ] **Step 1: Stop the running engine**

```powershell
Get-Process dj-engine -ErrorAction SilentlyContinue | Stop-Process -Force
```

- [ ] **Step 2: Re-analyze the real library**

The analyzer version bumped from 0.1.0 → 0.2.0, which invalidates all existing sidecars. Re-run on the user's library:

```powershell
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\dj-analyze.exe" `
    C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\data `
    C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzed-real `
    --workers 2
```

Expected: log shows `processed: 24, skipped: 0, errors: 0` (or close to it). Takes ~3-5 minutes.

- [ ] **Step 3: Inspect 3 sidecars**

```powershell
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" -c @"
import json
for i in (1, 7, 14):
    p = rf'C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzed-real\{i:03d}-song.mp3.json'
    sc = json.load(open(p))
    print(f'{i:03d}: schema_v={sc[\"schema_version\"]} is_beatmatchable={sc[\"is_beatmatchable\"]} is_phrase_aligned={sc[\"is_phrase_aligned\"]} bass_mean={sc[\"band_energy_means\"][\"bass\"]:.2f} mid_mean={sc[\"band_energy_means\"][\"mid\"]:.2f} high_mean={sc[\"band_energy_means\"][\"high\"]:.2f}')
"@
```

Expected: all three lines show `schema_v=2`, `is_beatmatchable=True`, band means populated. `is_phrase_aligned` may be True or False depending on each track's structural clarity.

- [ ] **Step 4: Count how many tracks are beatmatchable and phrase-aligned**

```powershell
& "C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe" -c @"
import json, glob
files = sorted(glob.glob(r'C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzed-real\*.json'))
beat = sum(1 for f in files if json.load(open(f)).get('is_beatmatchable') is True)
phrase = sum(1 for f in files if json.load(open(f)).get('is_phrase_aligned') is True)
print(f'beatmatchable: {beat}/{len(files)}')
print(f'phrase_aligned: {phrase}/{len(files)}')
"@
```

Expected: `beatmatchable: 24/24` (or very close). `phrase_aligned` should be most of them, but not necessarily all — depends on the music.

- [ ] **Step 5: Rebuild engine release binary**

```
& "C:\Users\f.nefzi\.cargo\bin\cargo.exe" build --release --manifest-path C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\Cargo.toml
```

- [ ] **Step 6: Ensure Icecast is running**

```powershell
if (-not (Get-Process icecast -ErrorAction SilentlyContinue)) {
    Start-Process -FilePath 'C:\Program Files (x86)\Icecast\bin\icecast.exe' `
        -ArgumentList '-c', 'C:\Users\f.nefzi\AppData\Local\Icecast-run\icecast.xml' `
        -WindowStyle Hidden `
        -RedirectStandardOutput 'C:\Users\f.nefzi\AppData\Local\Icecast-run\stdout.log' `
        -RedirectStandardError 'C:\Users\f.nefzi\AppData\Local\Icecast-run\stderr.log'
    Start-Sleep -Seconds 2
}
```

- [ ] **Step 7: Launch the engine**

```powershell
$env:ICECAST_URL = 'icecast://localhost:8000/dj.mp3'
$env:ICECAST_USER = 'source'
$env:ICECAST_PASSWORD = 'hackme'
$env:ICECAST_ADMIN_USER = 'admin'
$env:ICECAST_ADMIN_PASSWORD = 'hackme'
$env:ICECAST_FORMAT = 'mp3'
$env:ICECAST_BITRATE = '192k'
$env:RUST_LOG = 'info'

$logBase = 'C:\Users\f.nefzi\AppData\Local\Icecast-run'
Clear-Content "$logBase\engine-stdout.log","$logBase\engine-stderr.log" -ErrorAction SilentlyContinue

Start-Process `
    -FilePath 'C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine\target\release\dj-engine.exe' `
    -ArgumentList '--sidecars','C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzed-real',
                  '--music-root','C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\data' `
    -RedirectStandardOutput "$logBase\engine-stdout.log" `
    -RedirectStandardError "$logBase\engine-stderr.log" `
    -WindowStyle Hidden -PassThru
```

- [ ] **Step 8: Verify the stream is live**

```
timeout 10 curl -sS -o C:\Users\f.nefzi\AppData\Local\Icecast-run\smoke.mp3 http://localhost:8000/dj.mp3 --max-time 10
ffprobe -v error -show_entries format=duration,bit_rate -of default=noprint_wrappers=1 C:\Users\f.nefzi\AppData\Local\Icecast-run\smoke.mp3
```

Expected: duration ~10-13 s, bit_rate ~192000.

- [ ] **Step 9: Listen for ~15 minutes via VLC**

Open `http://localhost:8000/dj.mp3` in VLC. Tail the engine log:

```
Get-Content C:\Users\f.nefzi\AppData\Local\Icecast-run\engine-stdout.log -Tail 30 -Wait
```

Listening / log checklist:
- `length_bars` values vary across transitions (mix of 16, hopefully some 32, possibly some 8).
- `hard cut complete` appears rarely or never.
- `use_eq_swap=true` for most phrase-aligned transitions; `false` for non-phrase-aligned ones (still smooth, just no filter sweep).
- Two basslines do not fight during overlaps.

No commit for this task — it's verification.

- [ ] **Step 10: Document observations**

Note in chat (or as a follow-up plan if needed):
- Distribution of `length_bars` across the 10-20 transitions heard (count of 32 / 16 / 8 / 0).
- Whether any threshold feels off (transitions blending too long, or cutting too sharp).
- Any audible glitches at swap boundaries.

These observations will tune the constants in `transition.rs` and the selector weight in a follow-up pass.

---

## Self-review

### Spec coverage

| Spec section | Task(s) |
|---|---|
| Schema v2 — new fields + version bump | 1 |
| Schema v2 — backwards compatible | 1, 7, 8 |
| Analyzer 1.1 — split mixability | 5 |
| Analyzer 1.2 — librosa phrase detection | 4 |
| Analyzer 1.3 — per-band energy | 3 |
| Analyzer 1.4 — sidecar additions | 6 |
| Analyzer 1.5 — version bump | 2 |
| Engine 3.1 — sidecar fields + helpers | 8 |
| Engine 3.2 — phrase prefer-sidecar | 9 |
| Engine 3.3 — selector transition-aware | 11 |
| Engine 3.4 — relaxed hard cut, gated EQ | 10 |
| Engine 3.5 — band-aware EQ swap | 12 |
| Engine — mixer wires bass hint | 13 |
| Testing — analyzer | 3, 4, 5, 6, 7 |
| Testing — engine | 8, 9, 10, 11, 12 |
| End-to-end smoke | 15 |
| Lint + format | 14 |

**Gaps:** none. All spec sections map to tasks.

### Placeholder scan

Searched the plan for "TBD", "TODO", "implement later", "appropriate error handling", "similar to task N". No placeholders found. The `sweep_log` helper in Task 12 is a concrete consolidation; the schema validation tests in Task 7 reference an existing schema file as the "source of truth," which is concrete.

### Type consistency

- `BassMixHint { outgoing_bass_mean, incoming_bass_mean }` defined in Task 12, used by Task 13. Match.
- `BandValues { bass, mid, high }` defined in Task 8 (in `sidecar.rs`), referenced by Task 13's `band_energy_means.as_ref()`. Match.
- `Sidecar` v2 fields defined in Task 8 used in `transition.rs` Task 10 via `beatmatchable()` / `phrase_aligned()` methods. Match.
- `PhraseResult { boundaries, confidence }` defined in Task 4, used implicitly in Task 6 (`detect_phrases(...)`). Match.
- `classify(...)` returns a dict shape in Task 5; consumed in Task 6 via `mix["is_beatmatchable"]`, `mix["is_phrase_aligned"]`, `mix["is_mixable"]`, `mix["reasons"]`. Match.

### Scope

One focused enhancement, 15 bite-sized tasks, ~30 new tests, no new external deps. Executable in a single session.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-27-analyzer-selector-enhancement.md`. Two execution options:

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks, fast iteration
2. **Inline Execution** — executing-plans, batch with checkpoints

Which approach?
