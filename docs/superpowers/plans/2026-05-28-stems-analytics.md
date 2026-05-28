# Stems Analytics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Set up Python 3.10 + Demucs in the analyzer venv, add a `detect_stems.py` module that separates stems and derives per-bar drum/bass/vocal density + vocal segments, persist to sidecar, and have the engine load + log the new fields. **Phase D (stem-aware recipes) is a follow-up sub-project, not in this plan.**

**Architecture:** Recreate `analyzer/.venv` at Python 3.10. Add `detect_stems.py` that calls Demucs (htdemucs model), writes stems to `analyzed-real-stems/<track>/{drums,bass,vocals,other}.mp3`, computes per-bar density (RMS-per-bar normalized to [0,1]) and vocal segments (contiguous bars above a vocal-density threshold). Wire into `pipeline.py`. Add `stems` block to sidecar JSON + Rust struct. Engine logs vocal_segments count + drum density mean in `selector picked`. No selector / planner / recipe behavior changes.

**Tech Stack:** Python 3.10, librosa, demucs, torch, soundfile, numpy. Rust 2021, serde.

---

## File Structure

| File | New / Modify | Responsibility |
|---|---|---|
| `analyzer/.venv/` | Recreate | Python 3.10 venv with deps + demucs |
| `.gitignore` | Modify | Add `analyzed-*-stems/` |
| `analyzer/src/dj_analyze/detect_stems.py` | **New** | Demucs separation + per-bar density + vocal segments + write stems to disk |
| `analyzer/src/dj_analyze/sidecar.py` | Modify | Add `stems` dataclass + optional field on `Sidecar` |
| `analyzer/src/dj_analyze/pipeline.py` | Modify | Call `detect_stems`, attach to sidecar |
| `analyzer/src/dj_analyze/__init__.py` | Modify | Bump `__version__` (cache invalidation) |
| `analyzer/pyproject.toml` | Modify | Add `demucs`, `soundfile` to dependencies |
| `analyzer/tests/test_detect_stems.py` | **New** | Unit tests for density math + vocal segments; smoke test guarded with `@pytest.mark.slow` |
| `schema/sidecar.schema.json` | Modify | Add `stems` property block |
| `engine/src/sidecar.rs` | Modify | Add `Stems` struct + `Option<Stems>` field |
| `engine/src/main.rs` | Modify | Log vocal_segments count + drum_density_mean in `selector picked` |
| `engine/tests/sidecar_test.rs` | Modify | One test deserializing with `stems`, one without |

---

### Task 1: Install Python 3.10 and recreate the analyzer venv

**Files:**
- Recreate: `analyzer/.venv/`

- [ ] **Step 1: Verify Python 3.10 availability**

```powershell
py -3.10 --version
```

If it errors with "no installation found", install Python 3.10 via winget:

```powershell
winget install --id Python.Python.3.10 --accept-source-agreements --accept-package-agreements
```

After install, restart PowerShell so PATH refreshes. Then run `py -3.10 --version` again. Expected: `Python 3.10.x`.

If winget install fails (e.g., requires UAC, network blocked): STOP and report BLOCKED with the exact error — the user will install manually.

- [ ] **Step 2: Remove old venv and recreate at 3.10**

```powershell
Remove-Item -Recurse -Force C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv
py -3.10 -m venv C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv
C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe --version
```

Expected: `Python 3.10.x`.

- [ ] **Step 3: Upgrade pip and install existing analyzer deps**

```powershell
C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe -m pip install --upgrade pip
C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe -m pip install -e C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer[dev]
```

Expected: successful install. The `[dev]` extra brings pytest, etc.

- [ ] **Step 4: Run the existing analyzer test suite to confirm 3.10 works**

```powershell
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer
.venv\Scripts\python.exe -m pytest -q
```

Expected: all tests pass. If any fail with a Python-version-specific error (e.g., deprecated typing syntax, `match` statement issue), report which and fix per error.

- [ ] **Step 5: Commit (the venv is gitignored so nothing to add — verify clean tree)**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine status
```

Expected: no changes (venv is gitignored). Skip commit if so. If somehow `.venv` shows up, it's NOT supposed to be tracked — add `analyzer/.venv/` to `.gitignore` if missing.

---

### Task 2: Install Demucs + soundfile, declare in `pyproject.toml`

**Files:**
- Modify: `analyzer/pyproject.toml`

- [ ] **Step 1: Install demucs**

```powershell
C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe -m pip install demucs
```

Expected: pulls down demucs (~1 MB) + torch (~2 GB). Takes 1-3 minutes.

If install fails with "Could not find a version that satisfies torch": fall back to the CPU-only wheel:

```powershell
C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe -m pip install torch --index-url https://download.pytorch.org/whl/cpu
C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe -m pip install demucs
```

- [ ] **Step 2: Install soundfile (for MP3 export)**

```powershell
C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe -m pip install soundfile
```

(soundfile is often already pulled in by librosa, but installing explicitly makes it visible.)

- [ ] **Step 3: Smoke-test demucs imports**

```powershell
C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe -c "import demucs; import torch; import soundfile as sf; print('demucs', demucs.__version__, '/ torch', torch.__version__, '/ sf', sf.__version__)"
```

Expected output similar to: `demucs 4.0.1 / torch 2.x.x / sf 0.13.x`. If any import errors, STOP and report.

- [ ] **Step 4: Add to `pyproject.toml`**

Find the `[project]` `dependencies` list in `analyzer/pyproject.toml`. Append:

```toml
    "demucs>=4.0",
    "soundfile>=0.12",
```

Re-run `pip install -e .` to ensure the lock is updated:

```powershell
C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe -m pip install -e C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer
```

- [ ] **Step 5: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add analyzer/pyproject.toml
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "chore(analyzer): add demucs and soundfile dependencies"
```

---

### Task 3: Extend `.gitignore` and create stems output directory

**Files:**
- Modify: `.gitignore`

- [ ] **Step 1: Add the stems directory pattern**

Read `.gitignore`. Find the existing `analyzed/` and `analyzed-*/` patterns. Add a sibling pattern for stems:

```
analyzed-*-stems/
analyzed-stems/
```

Place near the existing `analyzed/` entries for consistency.

- [ ] **Step 2: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add .gitignore
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "chore: gitignore stems output directories"
```

---

### Task 4: Write `detect_stems.py` (TDD — tests first)

**Files:**
- Create: `analyzer/tests/test_detect_stems.py`

- [ ] **Step 1: Write the unit tests (functions that don't need Demucs)**

Create `analyzer/tests/test_detect_stems.py`:

```python
from __future__ import annotations

import numpy as np
import pytest

# These imports will fail until detect_stems.py exists — that's the TDD RED state.
from dj_analyze.detect_stems import (
    _per_bar_density,
    _vocal_segments_from_density,
    _VOCAL_PRESENT_THRESHOLD,
)


def test_per_bar_density_normalizes_to_unit_max() -> None:
    # 4 stems x 2 channels x 4*2 seconds @ sr=4
    sr = 4
    bars = [0.0, 2.0, 4.0, 6.0, 8.0]  # 4 bars of 2 seconds each
    # Build a fake sources array: stem 0 ("drums") is silent in bars 0,2; loud in bar 1; medium in bar 3.
    samples_per_bar = 8
    drums_mono = np.zeros(32, dtype=np.float32)
    drums_mono[8:16] = 1.0          # bar 1: amplitude 1
    drums_mono[24:32] = 0.5         # bar 3: amplitude 0.5
    drums = np.stack([drums_mono, drums_mono])  # stereo
    silence = np.zeros_like(drums)
    sources = np.stack([drums, silence, silence, silence])  # (4, 2, 32)

    density = _per_bar_density(sources, ["drums", "bass", "other", "vocals"], "drums", bars, sr=sr)
    # Loudest bar (1) normalized to 1.0; bar 3 should be ~0.5.
    assert len(density) == 4
    assert density[0] == pytest.approx(0.0, abs=1e-6)
    assert density[1] == pytest.approx(1.0, abs=1e-6)
    assert density[2] == pytest.approx(0.0, abs=1e-6)
    assert density[3] == pytest.approx(0.5, abs=1e-6)


def test_per_bar_density_returns_empty_for_unknown_stem() -> None:
    sources = np.zeros((4, 2, 16), dtype=np.float32)
    density = _per_bar_density(sources, ["drums", "bass", "other", "vocals"], "doesnotexist", [0.0, 1.0, 2.0], sr=4)
    assert density == []


def test_per_bar_density_all_silent() -> None:
    sources = np.zeros((4, 2, 16), dtype=np.float32)
    density = _per_bar_density(sources, ["drums", "bass", "other", "vocals"], "drums", [0.0, 1.0, 2.0, 3.0, 4.0], sr=4)
    assert density == [0.0, 0.0, 0.0, 0.0]


def test_vocal_segments_merges_contiguous_above_threshold() -> None:
    # 7 bars at 2-second boundaries → bar_edges = [0, 2, 4, 6, 8, 10, 12, 14]
    bar_edges = [0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0]
    # Densities: silent, present, present, silent, silent, present, silent
    densities = [0.0, 0.5, 0.5, 0.0, 0.0, 0.4, 0.0]
    segments = _vocal_segments_from_density(densities, bar_edges)
    # Expect (2.0, 6.0) for bars 1-2, and (10.0, 12.0) for bar 5.
    assert segments == [(2.0, 6.0), (10.0, 12.0)]


def test_vocal_segments_returns_empty_for_silence() -> None:
    bar_edges = [0.0, 2.0, 4.0]
    densities = [0.0, 0.0]
    assert _vocal_segments_from_density(densities, bar_edges) == []


def test_vocal_segments_handles_track_ending_with_vocals() -> None:
    bar_edges = [0.0, 2.0, 4.0, 6.0]
    densities = [0.0, 0.5, 0.5]
    segments = _vocal_segments_from_density(densities, bar_edges)
    assert segments == [(2.0, 6.0)]


def test_vocal_segments_handles_track_starting_with_vocals() -> None:
    bar_edges = [0.0, 2.0, 4.0]
    densities = [0.5, 0.0]
    segments = _vocal_segments_from_density(densities, bar_edges)
    assert segments == [(0.0, 2.0)]


def test_vocal_segments_below_threshold_doesnt_count() -> None:
    bar_edges = [0.0, 2.0, 4.0]
    # Densities just below threshold → no segments.
    densities = [_VOCAL_PRESENT_THRESHOLD - 0.001, _VOCAL_PRESENT_THRESHOLD - 0.001]
    assert _vocal_segments_from_density(densities, bar_edges) == []
```

- [ ] **Step 2: Run the tests to confirm RED**

```powershell
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer
.venv\Scripts\python.exe -m pytest tests/test_detect_stems.py -v
```

Expected: ImportError on `dj_analyze.detect_stems` — the module doesn't exist yet.

- [ ] **Step 3: Commit failing tests**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add analyzer/tests/test_detect_stems.py
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "test(analyzer): failing tests for stem density + vocal segments (TDD)"
```

---

### Task 5: Implement `detect_stems.py`

**Files:**
- Create: `analyzer/src/dj_analyze/detect_stems.py`

- [ ] **Step 1: Write the module**

Create `analyzer/src/dj_analyze/detect_stems.py`:

```python
"""Stem separation via Demucs + per-bar density analytics.

Outputs:
  - 4 stem MP3s on disk under <stems_root>/<track_basename>/{drums,bass,vocals,other}.mp3
  - StemAnalytics dataclass with per-bar drum/bass/vocal densities,
    vocal_segments, energy_per_stem.

The heavy demucs import is deferred so the helper functions can be unit-tested
without loading torch.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

import numpy as np


@dataclass
class StemAnalytics:
    drum_density: list[float] = field(default_factory=list)
    bass_density: list[float] = field(default_factory=list)
    vocal_density: list[float] = field(default_factory=list)
    vocal_segments: list[tuple[float, float]] = field(default_factory=list)
    energy_per_stem: dict[str, float] = field(default_factory=dict)
    demucs_version: str = "unknown"
    model: str = "htdemucs"


_VOCAL_PRESENT_THRESHOLD = 0.15
_MODEL_NAME = "htdemucs"
_MODEL_CACHE: dict[str, object] = {}


def detect_stems(
    audio: np.ndarray,
    sr: int,
    downbeats_seconds: list[float],
    track_basename: str,
    stems_root: Path,
) -> StemAnalytics:
    """Separate stems with Demucs, write to disk, derive analytics."""
    import librosa
    import soundfile as sf
    import torch
    from demucs.apply import apply_model
    from demucs.pretrained import get_model

    model = _MODEL_CACHE.get(_MODEL_NAME)
    if model is None:
        model = get_model(_MODEL_NAME)
        _MODEL_CACHE[_MODEL_NAME] = model

    # Demucs wants stereo @ 44100.
    if sr != 44100:
        audio_44k = librosa.resample(audio, orig_sr=sr, target_sr=44100)
    else:
        audio_44k = audio
    if audio_44k.ndim == 1:
        audio_44k = np.stack([audio_44k, audio_44k])
    elif audio_44k.ndim == 2 and audio_44k.shape[0] != 2:
        # If (samples, 2), transpose to (2, samples).
        if audio_44k.shape[1] == 2:
            audio_44k = audio_44k.T

    waveform = torch.from_numpy(audio_44k).float().unsqueeze(0)
    with torch.no_grad():
        sources = apply_model(model, waveform, progress=False)
    sources_np = sources.squeeze(0).cpu().numpy()  # (4, channels, samples)
    stem_names = list(model.sources)

    # Write stems to disk (mono mix for compactness).
    track_dir = stems_root / track_basename
    track_dir.mkdir(parents=True, exist_ok=True)
    for name, stem in zip(stem_names, sources_np):
        stem_mono = stem.mean(axis=0).astype(np.float32)
        out_path = track_dir / f"{name}.mp3"
        try:
            sf.write(str(out_path), stem_mono, 44100, format="MP3")
        except Exception:
            # Fall back to WAV if MP3 encoding isn't available on this platform.
            wav_path = track_dir / f"{name}.wav"
            sf.write(str(wav_path), stem_mono, 44100)

    # Per-bar density (use 44100 SR since that's the stem audio's SR).
    if len(downbeats_seconds) < 2:
        return StemAnalytics(
            energy_per_stem={n: float(np.sqrt(np.mean(sources_np[i].mean(axis=0) ** 2))) for i, n in enumerate(stem_names)},
            demucs_version=_demucs_version(),
        )

    bar_edges = list(downbeats_seconds)
    drum_density = _per_bar_density(sources_np, stem_names, "drums", bar_edges, sr=44100)
    bass_density = _per_bar_density(sources_np, stem_names, "bass", bar_edges, sr=44100)
    vocal_density = _per_bar_density(sources_np, stem_names, "vocals", bar_edges, sr=44100)
    vocal_segments = _vocal_segments_from_density(vocal_density, bar_edges)

    energy_per_stem = {
        n: float(np.sqrt(np.mean(sources_np[i].mean(axis=0) ** 2)))
        for i, n in enumerate(stem_names)
    }

    return StemAnalytics(
        drum_density=drum_density,
        bass_density=bass_density,
        vocal_density=vocal_density,
        vocal_segments=vocal_segments,
        energy_per_stem=energy_per_stem,
        demucs_version=_demucs_version(),
    )


def _per_bar_density(
    sources: np.ndarray,
    stem_names: list[str],
    target_stem: str,
    bar_edges_seconds: list[float],
    sr: int,
) -> list[float]:
    if target_stem not in stem_names:
        return []
    idx = stem_names.index(target_stem)
    stem = sources[idx].mean(axis=0) if sources[idx].ndim == 2 else sources[idx]
    rms_per_bar: list[float] = []
    for start_s, end_s in zip(bar_edges_seconds[:-1], bar_edges_seconds[1:]):
        i0 = int(start_s * sr)
        i1 = int(end_s * sr)
        if i1 <= i0 or i0 < 0 or i0 >= len(stem):
            rms_per_bar.append(0.0)
            continue
        i1 = min(i1, len(stem))
        chunk = stem[i0:i1]
        if chunk.size == 0:
            rms_per_bar.append(0.0)
        else:
            rms_per_bar.append(float(np.sqrt(np.mean(chunk.astype(np.float64) ** 2))))
    if not rms_per_bar:
        return []
    max_rms = max(rms_per_bar)
    if max_rms <= 0:
        return [0.0] * len(rms_per_bar)
    return [r / max_rms for r in rms_per_bar]


def _vocal_segments_from_density(
    vocal_density: list[float],
    bar_edges_seconds: list[float],
) -> list[tuple[float, float]]:
    if not vocal_density or len(bar_edges_seconds) < 2:
        return []
    segments: list[tuple[float, float]] = []
    in_segment = False
    seg_start = 0.0
    seg_end = 0.0
    for i, d in enumerate(vocal_density):
        bar_start = bar_edges_seconds[i] if i < len(bar_edges_seconds) else bar_edges_seconds[-1]
        bar_end = bar_edges_seconds[i + 1] if i + 1 < len(bar_edges_seconds) else bar_edges_seconds[-1]
        if d >= _VOCAL_PRESENT_THRESHOLD:
            if not in_segment:
                seg_start = bar_start
                in_segment = True
            seg_end = bar_end
        else:
            if in_segment:
                segments.append((seg_start, seg_end))
                in_segment = False
    if in_segment:
        segments.append((seg_start, seg_end))
    return segments


def _demucs_version() -> str:
    try:
        import demucs
        return getattr(demucs, "__version__", "unknown")
    except Exception:
        return "unknown"
```

- [ ] **Step 2: Run the unit tests — should pass now**

```powershell
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer
.venv\Scripts\python.exe -m pytest tests/test_detect_stems.py -v
```

Expected: all 7 tests pass.

- [ ] **Step 3: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add analyzer/src/dj_analyze/detect_stems.py
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(analyzer): detect_stems with Demucs separation + per-bar density + vocal segments"
```

---

### Task 6: Add `stems` field to `Sidecar` dataclass + JSON schema

**Files:**
- Modify: `analyzer/src/dj_analyze/sidecar.py`
- Modify: `schema/sidecar.schema.json`

- [ ] **Step 1: Extend the Sidecar dataclass**

Read `analyzer/src/dj_analyze/sidecar.py`. Find the `Sidecar` dataclass. Add:

```python
# Near the top, alongside any other nested dataclasses:
@dataclass
class StemsBlock:
    drum_density: list[float] = field(default_factory=list)
    bass_density: list[float] = field(default_factory=list)
    vocal_density: list[float] = field(default_factory=list)
    vocal_segments: list[list[float]] = field(default_factory=list)  # JSON-friendly
    energy_per_stem: dict[str, float] = field(default_factory=dict)
    demucs_version: str = "unknown"
    model: str = "htdemucs"
```

In the `Sidecar` dataclass, add:

```python
    stems: StemsBlock | None = None
```

If the existing sidecar serialization uses a manual `to_dict` / `from_dict`, extend those too. If it uses `dataclasses_json` or `pydantic`, the new field is auto-handled.

- [ ] **Step 2: Extend the JSON schema**

Read `schema/sidecar.schema.json`. Find the `properties` block. Add:

```json
    "stems": {
      "type": ["object", "null"],
      "properties": {
        "drum_density": { "type": "array", "items": { "type": "number" } },
        "bass_density": { "type": "array", "items": { "type": "number" } },
        "vocal_density": { "type": "array", "items": { "type": "number" } },
        "vocal_segments": {
          "type": "array",
          "items": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 }
        },
        "energy_per_stem": { "type": "object", "additionalProperties": { "type": "number" } },
        "demucs_version": { "type": "string" },
        "model": { "type": "string" }
      }
    }
```

- [ ] **Step 3: Verify schema is still valid JSON**

```powershell
C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer\.venv\Scripts\python.exe -c "import json; json.load(open(r'C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\schema\sidecar.schema.json')); print('ok')"
```

Expected: `ok`.

- [ ] **Step 4: Run the analyzer tests (schema-validation test must still pass)**

```powershell
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer
.venv\Scripts\python.exe -m pytest -q
```

Expected: all pass. If the schema_validation test fails because of the new field, debug — it may need the field to be marked optional.

- [ ] **Step 5: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add analyzer/src/dj_analyze/sidecar.py schema/sidecar.schema.json
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(analyzer): add stems block to Sidecar dataclass + JSON schema"
```

---

### Task 7: Wire `detect_stems` into the analyzer pipeline + bump version

**Files:**
- Modify: `analyzer/src/dj_analyze/pipeline.py`
- Modify: `analyzer/src/dj_analyze/__init__.py`

- [ ] **Step 1: Add stems extraction to the pipeline**

Read `analyzer/src/dj_analyze/pipeline.py`. Find the place where the Sidecar is being built (after bpm + downbeats + bands are computed). Insert a call to `detect_stems`:

```python
from dj_analyze.detect_stems import detect_stems, StemAnalytics
from dj_analyze.sidecar import StemsBlock
# ...

# After downbeats_seconds is known and the sidecar is being constructed:
stems_root = sidecars_dir.parent / f"{sidecars_dir.name}-stems"
try:
    track_basename = Path(source_path).stem  # e.g., "001-song"
    sa = detect_stems(audio, sr, downbeats_seconds, track_basename, stems_root)
    stems_block = StemsBlock(
        drum_density=sa.drum_density,
        bass_density=sa.bass_density,
        vocal_density=sa.vocal_density,
        vocal_segments=[[s, e] for s, e in sa.vocal_segments],
        energy_per_stem=sa.energy_per_stem,
        demucs_version=sa.demucs_version,
        model=sa.model,
    )
except Exception as e:
    logger.warning("detect_stems failed for %s: %s", source_path, e)
    stems_block = None

# Then on the sidecar object:
sidecar.stems = stems_block
```

Match the existing pipeline's style (logger name, error handling, where `audio` and `sr` are scoped). Read the surrounding code to fit.

`sidecars_dir.parent / f"{sidecars_dir.name}-stems"` produces e.g., `analyzed-real-stems/` if sidecars are at `analyzed-real/`. Adjust based on the actual `sidecars_dir` variable name in `pipeline.py`.

- [ ] **Step 2: Bump `__version__` for cache invalidation**

In `analyzer/src/dj_analyze/__init__.py`, bump:

```python
__version__ = "0.4.0"
```

(was 0.3.1 — bump to 0.4.0 since this is a non-trivial schema change.)

- [ ] **Step 3: Run the analyzer pipeline test on one real track to sanity-check**

```powershell
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\analyzer
mkdir C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\.test-stems-out -ErrorAction SilentlyContinue
.venv\Scripts\python.exe -m dj_analyze C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\data\001-song.mp3 C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\.test-stems-out
```

Wait — the analyzer CLI takes a directory, not a single file. Check `analyzer/src/dj_analyze/__main__.py` for the CLI signature. If it accepts a single file, use the command above. If it requires a directory, create a temp dir with just `001-song.mp3`:

```powershell
mkdir C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\.test-one-song
Copy-Item C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\data\001-song.mp3 C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\.test-one-song\
.venv\Scripts\python.exe -m dj_analyze C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\.test-one-song C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\.test-stems-out
```

Expected:
- A sidecar JSON written to `.test-stems-out/001-song.mp3.json` containing a `stems` block with non-empty arrays.
- A directory `.test-stems-out-stems/001-song/` containing 4 audio files (MP3 or WAV fallback).
- Runtime: ~3-5 minutes (first-time demucs model download + separation).

Verify:

```powershell
Get-Content C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\.test-stems-out\001-song.mp3.json | Select-String '"drum_density"|"vocal_segments"'
Get-ChildItem C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\.test-stems-out-stems\001-song\
```

If either fails (e.g., empty stems block, no audio files), inspect the error log from the analyzer run. Likely culprits: import paths, demucs model download failure, missing `soundfile` MP3 codec.

Cleanup:

```powershell
Remove-Item -Recurse C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\.test-one-song, C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\.test-stems-out, C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\.test-stems-out-stems -ErrorAction SilentlyContinue
```

- [ ] **Step 4: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add analyzer/src/dj_analyze/pipeline.py analyzer/src/dj_analyze/__init__.py
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(analyzer): wire detect_stems into pipeline; bump version to 0.4.0"
```

---

### Task 8: Engine — load + log stems data

**Files:**
- Modify: `engine/src/sidecar.rs`
- Modify: `engine/src/main.rs`
- Modify: `engine/tests/sidecar_test.rs`

- [ ] **Step 1: Add `Stems` struct to `sidecar.rs`**

In `engine/src/sidecar.rs`, alongside the existing `BandEnergyMeans` struct (or wherever nested sidecar types live), add:

```rust
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct Stems {
    #[serde(default)]
    pub drum_density: Vec<f64>,
    #[serde(default)]
    pub bass_density: Vec<f64>,
    #[serde(default)]
    pub vocal_density: Vec<f64>,
    #[serde(default)]
    pub vocal_segments: Vec<[f64; 2]>,
    #[serde(default)]
    pub energy_per_stem: std::collections::HashMap<String, f64>,
    #[serde(default)]
    pub demucs_version: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}
```

In the `Sidecar` struct, add (alphabetically near other fields):

```rust
    #[serde(default)]
    pub stems: Option<Stems>,
```

- [ ] **Step 2: Extend `selector picked` log in `main.rs`**

Find the `selector picked` `info!` call. Add helper above the loop:

```rust
fn mean_or_zero(v: &[f64]) -> f64 {
    if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 }
}
```

Modify the `info!` block. Just before it, compute summaries:

```rust
let outgoing_vocal_segs = current.sidecar.stems.as_ref().map(|s| s.vocal_segments.len()).unwrap_or(0);
let outgoing_drum_density_mean = current.sidecar.stems.as_ref().map(|s| mean_or_zero(&s.drum_density)).unwrap_or(0.0);
let incoming_vocal_segs = next_track.sidecar.stems.as_ref().map(|s| s.vocal_segments.len()).unwrap_or(0);
let incoming_drum_density_mean = next_track.sidecar.stems.as_ref().map(|s| mean_or_zero(&s.drum_density)).unwrap_or(0.0);
```

Then in the `info!` call, add the four fields:

```rust
    outgoing_vocal_segments = outgoing_vocal_segs,
    outgoing_drum_density_mean,
    incoming_vocal_segments = incoming_vocal_segs,
    incoming_drum_density_mean,
```

- [ ] **Step 3: Add sidecar deserialization tests**

Append to `engine/tests/sidecar_test.rs`:

```rust
#[test]
fn deserializes_sidecar_with_stems_block() {
    let json = r#"{
        "schema_version": 1,
        "analyzer_version": "0.4.0",
        "source_path": "test.mp3",
        "source_mtime": 0.0,
        "source_size_bytes": 0,
        "stems": {
            "drum_density": [0.1, 0.5, 0.9],
            "bass_density": [0.2, 0.4, 0.8],
            "vocal_density": [0.0, 0.3, 0.6],
            "vocal_segments": [[10.0, 20.0]],
            "energy_per_stem": { "drums": 0.7, "vocals": 0.4 },
            "demucs_version": "4.0.1",
            "model": "htdemucs"
        }
    }"#;
    let sc: dj_engine::sidecar::Sidecar = serde_json::from_str(json).expect("should parse");
    let stems = sc.stems.expect("stems should be present");
    assert_eq!(stems.drum_density.len(), 3);
    assert_eq!(stems.vocal_segments.len(), 1);
    assert!((stems.energy_per_stem["drums"] - 0.7).abs() < 1e-9);
    assert_eq!(stems.model.as_deref(), Some("htdemucs"));
}

#[test]
fn deserializes_sidecar_without_stems_block() {
    let json = r#"{
        "schema_version": 1,
        "analyzer_version": "0.3.1",
        "source_path": "test.mp3",
        "source_mtime": 0.0,
        "source_size_bytes": 0
    }"#;
    let sc: dj_engine::sidecar::Sidecar = serde_json::from_str(json).expect("should parse");
    assert!(sc.stems.is_none());
}
```

If the test file's existing tests use a custom helper to build sidecar JSON, mimic that helper for the two new tests.

- [ ] **Step 4: Build and test the engine**

```powershell
cd C:\Users\f.nefzi\Desktop\Personal\DJ-Engine\engine
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --release 2>&1 | Select-Object -Last 3
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --test sidecar_test 2>&1 | Select-Object -Last 5
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test 2>&1 | Select-Object -Last 5
```

Expected: clean build, all sidecar tests pass, full suite passes.

- [ ] **Step 5: cargo fmt + clippy**

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" fmt
& "$env:USERPROFILE\.cargo\bin\cargo.exe" clippy --all-targets -- -D warnings
```

Fix any warnings.

- [ ] **Step 6: Commit**

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add engine/src/sidecar.rs engine/src/main.rs engine/tests/sidecar_test.rs
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "feat(engine): load + log stems data from sidecar"
```

If `cargo fmt` made changes, commit them separately:

```
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine add -u
git -C C:/Users/f.nefzi/Desktop/Personal/DJ-Engine commit -m "chore(engine): fmt after stems integration"
```

---

### Task 9: Report — leave full library re-analysis to the user

Phase A+B+C engineering is complete after Task 8. The user runs the full library re-analysis manually (it takes ~90 minutes for 18 tracks). The implementer DOES NOT run the full analyzer here.

- [ ] **Step 1: Report to controller**

The controller will tell the user how to kick off the full re-analysis. Subagent's job ends here.

---

## Self-review notes

- **Spec coverage:**
  - Python 3.10 env → Task 1.
  - Demucs install → Task 2.
  - `detect_stems.py` + tests → Tasks 4 & 5.
  - Sidecar schema + dataclass → Task 6.
  - Pipeline integration + version bump → Task 7.
  - Engine read + log → Task 8.
- **Placeholders:** None. Every step has concrete commands or code.
- **Type consistency:** `StemAnalytics` (analyzer) ↔ `StemsBlock` (sidecar dataclass) ↔ `Stems` (Rust struct) all carry the same fields. `vocal_segments` is a list of 2-element tuples (Python) / 2-element arrays (JSON) / `[f64; 2]` arrays (Rust). Consistent.
- **Risk: demucs install time.** Task 2 takes 1-3 minutes. The first separation in Task 7 takes another 3-5 minutes (model download + run). Total Task 7 runtime: ~10 minutes. Set generous timeouts.
- **Risk: model download.** Demucs caches the model in `~/.cache/torch/hub/checkpoints/`. If the disk is full or network is down, separation fails. Report BLOCKED with the error.
- **Risk: MP3 codec missing.** `soundfile` delegates MP3 to libmp3lame. On Windows with default `pip install soundfile`, MP3 may not be available. The detect_stems code falls back to WAV automatically — but the disk usage triples. Acceptable; note in the report if MP3 wasn't available.
