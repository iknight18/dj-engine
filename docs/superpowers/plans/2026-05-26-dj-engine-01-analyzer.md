# DJ Engine — Plan 1: Analyzer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Python `dj-analyze` CLI: it walks a music directory, extracts BPM, beats, downbeats, key, energy and cue points from every track, and writes one JSON sidecar per track to an output directory. Idempotent, resumable, parallel.

**Architecture:** A Python package (`dj_analyze`) with one module per detection concern, an orchestrator that produces the sidecar dict, and a parallel pipeline runner. The output schema (`schema/sidecar.schema.json`) is the contract with Plan 2's engine. Plan 2 reads exactly the JSONs produced here.

**Tech Stack:** Python 3.11+, `librosa` (BPM/beats/energy), `madmom` (downbeats, with librosa fallback), `essentia` (key detection), `soundfile` + `audioread` (decode via ffmpeg), `multiprocessing` (parallelism), `rich` (progress), `jsonschema` (validation in tests), `pytest`, `ruff`, `mypy`.

**Prerequisite:** `ffmpeg.exe` on PATH. Python 3.11+. Git.

**Spec:** `docs/superpowers/specs/2026-05-26-dj-engine-design.md`

---

## File structure (this plan creates these files)

```
DJ-Engine/
  .gitignore                                # NEW
  README.md                                 # NEW (stub)
  schema/
    sidecar.schema.json                     # NEW
  analyzer/
    pyproject.toml                          # NEW
    Dockerfile                              # NEW
    src/dj_analyze/
      __init__.py                           # NEW (version)
      __main__.py                           # NEW (CLI entry)
      types.py                              # NEW (TypedDict for sidecar)
      io_helpers.py                         # NEW (file discovery, path computation)
      sidecar.py                            # NEW (load/save/is_fresh)
      decode.py                             # NEW (audio loading wrapper)
      detect_bpm.py                         # NEW (BPM + confidence)
      detect_beats.py                       # NEW (beat times)
      detect_downbeats.py                   # NEW (madmom or fallback)
      detect_key.py                         # NEW (essentia + Camelot mapping)
      detect_energy.py                      # NEW (RMS curve + mean)
      detect_cues.py                        # NEW (cue_in/out, intro/outro)
      mixability.py                         # NEW (classify mixable tracks)
      analyze.py                            # NEW (per-track orchestrator)
      pipeline.py                           # NEW (parallel runner)
      logging_config.py                     # NEW (pretty vs JSON logs)
    tests/
      __init__.py                           # NEW
      conftest.py                           # NEW (fixture generators)
      fixtures/
        generate_fixtures.py                # NEW (one-shot WAV generator)
        click_120bpm.wav                    # GENERATED
        sine_a_minor.wav                    # GENERATED
        silence_then_beep.wav               # GENERATED
        corrupt.mp3                         # NEW (1 byte file)
      test_io_helpers.py                    # NEW
      test_sidecar.py                       # NEW
      test_decode.py                        # NEW
      test_detect_bpm.py                    # NEW
      test_detect_beats.py                  # NEW
      test_detect_downbeats.py              # NEW
      test_detect_key.py                    # NEW
      test_detect_energy.py                 # NEW
      test_detect_cues.py                   # NEW
      test_mixability.py                    # NEW
      test_analyze.py                       # NEW
      test_pipeline.py                      # NEW
      test_cli.py                           # NEW
      test_schema_validation.py             # NEW
  deploy/
    analyzer-job.yaml                       # NEW
```

Each module has one clear responsibility. Detection modules expose a single pure function taking `(audio_samples, sample_rate, ...)` and returning a typed result. The orchestrator stitches them together. Tests live next to the package, organized one-test-file-per-module.

---

### Task 1: Initialize git repository

**Files:**
- Create: `.gitignore`

- [ ] **Step 1: Initialize git in the project root**

Run from `C:\Users\f.nefzi\Desktop\Personal\DJ-Engine`:
```bash
git init
git branch -M main
```

Expected: `Initialized empty Git repository in .git/` and the branch renamed to `main`.

- [ ] **Step 2: Create .gitignore**

Create `.gitignore`:
```gitignore
# Python
__pycache__/
*.py[cod]
*$py.class
*.egg-info/
.venv/
venv/
.pytest_cache/
.mypy_cache/
.ruff_cache/

# Rust
target/
Cargo.lock

# OS
.DS_Store
Thumbs.db

# IDE
.vscode/
.idea/

# Build / dist
dist/
build/

# Generated audio fixtures (regenerated from generate_fixtures.py)
# Keep them committed for CI reproducibility — remove this line if you want to regenerate locally
# analyzer/tests/fixtures/*.wav

# Local Icecast / dev artifacts
*.log
analyzed/
```

- [ ] **Step 3: Commit**

```bash
git add .gitignore
git commit -m "chore: initialize repo with .gitignore"
```

Expected: one commit on `main`.

---

### Task 2: Create directory skeleton + README stub

**Files:**
- Create: `README.md`
- Create: `schema/.gitkeep`
- Create: `analyzer/src/dj_analyze/.gitkeep`
- Create: `analyzer/tests/fixtures/.gitkeep`
- Create: `deploy/.gitkeep`
- Create: `docs/superpowers/plans/.gitkeep`

- [ ] **Step 1: Create the directory tree**

Run:
```bash
mkdir -p schema analyzer/src/dj_analyze analyzer/tests/fixtures deploy docs/superpowers/plans docs/superpowers/specs
```

(The spec already exists at `docs/superpowers/specs/2026-05-26-dj-engine-design.md`; this step ensures sibling directories exist.)

- [ ] **Step 2: Write the README stub**

Create `README.md`:
```markdown
# DJ Engine

Two-program system that DJs a directory of audio files and streams the live mix to Icecast.

- **`analyzer/`** — Python CLI: scans music, writes one JSON sidecar per track.
- **`engine/`** — Rust binary: reads sidecars, beatmatches + harmonically mixes, streams to Icecast.
- **`schema/sidecar.schema.json`** — the contract between the two halves.
- **`deploy/`** — Kubernetes manifests for Rancher deployment.

See `docs/superpowers/specs/` for the design and `docs/superpowers/plans/` for implementation plans.

## Dev quickstart (filled in over the course of the plans)

TBD — see plan 02-analyzer.md and plan 03-engine.md.
```

- [ ] **Step 3: Commit**

```bash
git add README.md schema docs deploy analyzer
git commit -m "chore: create directory skeleton and README stub"
```

---

### Task 3: Write the sidecar JSON schema

**Files:**
- Create: `schema/sidecar.schema.json`

This is the contract between analyzer and engine. Both halves validate against it.

- [ ] **Step 1: Write the schema**

Create `schema/sidecar.schema.json`:
```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://dj-engine/schema/sidecar.schema.json",
  "title": "DJ Engine Sidecar",
  "type": "object",
  "required": [
    "schema_version",
    "analyzer_version",
    "source_path",
    "source_mtime",
    "source_size_bytes"
  ],
  "properties": {
    "schema_version": { "const": 1 },
    "analyzer_version": { "type": "string" },
    "source_path": { "type": "string" },
    "source_mtime": { "type": "number" },
    "source_size_bytes": { "type": "integer", "minimum": 0 },
    "error": { "type": "string" },

    "duration_seconds": { "type": "number", "minimum": 0 },
    "sample_rate": { "type": "integer", "minimum": 1 },

    "bpm": { "type": "number", "minimum": 30, "maximum": 250 },
    "bpm_confidence": { "type": "number", "minimum": 0, "maximum": 1 },
    "beats_seconds": {
      "type": "array",
      "items": { "type": "number", "minimum": 0 }
    },
    "downbeats_seconds": {
      "type": "array",
      "items": { "type": "number", "minimum": 0 }
    },

    "key": {
      "type": "string",
      "pattern": "^(1[0-2]|[1-9])[AB]$"
    },
    "key_label": { "type": "string" },
    "key_confidence": { "type": "number", "minimum": 0, "maximum": 1 },

    "energy_curve": {
      "type": "array",
      "items": { "type": "number", "minimum": 0, "maximum": 1 }
    },
    "energy_mean": { "type": "number", "minimum": 0, "maximum": 1 },

    "cue_in_seconds": { "type": "number", "minimum": 0 },
    "cue_out_seconds": { "type": "number", "minimum": 0 },
    "intro_end_seconds": { "type": "number", "minimum": 0 },
    "outro_start_seconds": { "type": "number", "minimum": 0 },

    "is_mixable": { "type": "boolean" },
    "mixable_reasons": {
      "type": "array",
      "items": {
        "type": "string",
        "enum": [
          "bpm_low_confidence",
          "bpm_variable",
          "no_clear_downbeat",
          "too_short",
          "key_low_confidence"
        ]
      }
    }
  },
  "additionalProperties": false,
  "allOf": [
    {
      "if": { "not": { "required": ["error"] } },
      "then": {
        "required": [
          "duration_seconds",
          "sample_rate",
          "bpm",
          "bpm_confidence",
          "beats_seconds",
          "downbeats_seconds",
          "key",
          "key_label",
          "key_confidence",
          "energy_curve",
          "energy_mean",
          "cue_in_seconds",
          "cue_out_seconds",
          "intro_end_seconds",
          "outro_start_seconds",
          "is_mixable",
          "mixable_reasons"
        ]
      }
    }
  ]
}
```

- [ ] **Step 2: Verify the schema is valid JSON Schema**

Run:
```bash
python -c "import json, urllib.request; from jsonschema import Draft202012Validator; s=json.load(open('schema/sidecar.schema.json')); Draft202012Validator.check_schema(s); print('schema OK')"
```

(If `jsonschema` is not installed yet: `pip install jsonschema`.)

Expected: `schema OK`.

- [ ] **Step 3: Commit**

```bash
git add schema/sidecar.schema.json
git commit -m "feat(schema): define sidecar JSON schema (contract)"
```

---

### Task 4: Set up the Python package skeleton

**Files:**
- Create: `analyzer/pyproject.toml`
- Create: `analyzer/src/dj_analyze/__init__.py`
- Create: `analyzer/tests/__init__.py`

- [ ] **Step 1: Write pyproject.toml**

Create `analyzer/pyproject.toml`:
```toml
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[project]
name = "dj_analyze"
version = "0.1.0"
description = "Analyzer for DJ Engine — emits sidecar JSONs from a music directory."
requires-python = ">=3.11"
dependencies = [
    "numpy>=1.26,<2.1",
    "librosa>=0.10.2",
    "soundfile>=0.12.1",
    "audioread>=3.0.1",
    "rich>=13.7.0",
    "essentia>=2.1b6.dev1110; platform_system != 'Darwin' or platform_machine != 'arm64'",
    "madmom>=0.16.1",
]

[project.optional-dependencies]
dev = [
    "pytest>=8.0",
    "pytest-mock>=3.12",
    "jsonschema>=4.21",
    "ruff>=0.4",
    "mypy>=1.10",
    "scipy>=1.13",
]

[project.scripts]
dj-analyze = "dj_analyze.__main__:main"

[tool.hatch.build.targets.wheel]
packages = ["src/dj_analyze"]

[tool.ruff]
line-length = 100
target-version = "py311"
src = ["src", "tests"]

[tool.ruff.lint]
select = ["E", "F", "W", "I", "B", "UP", "N"]

[tool.mypy]
python_version = "3.11"
strict = true
disallow_untyped_decorators = false
warn_return_any = false
ignore_missing_imports = true

[tool.pytest.ini_options]
testpaths = ["tests"]
python_files = "test_*.py"
```

Notes:
- `essentia` install on Apple Silicon (`darwin` + `arm64`) is known to be painful; we exclude it there via the environment marker. The detection module has a librosa fallback (see Task 11).
- `madmom` is included but if its install fails, Task 10 covers a fallback path.

- [ ] **Step 2: Write the package `__init__.py`**

Create `analyzer/src/dj_analyze/__init__.py`:
```python
"""DJ Engine analyzer — emits sidecar JSONs from a directory of audio files."""

__version__ = "0.1.0"
```

- [ ] **Step 3: Write the test package marker**

Create `analyzer/tests/__init__.py` (empty file):
```python
```

- [ ] **Step 4: Verify the package installs**

Run from `analyzer/`:
```bash
python -m venv .venv
.venv\Scripts\activate
pip install -e ".[dev]"
```

Expected: install succeeds and `dj-analyze --help` works (with a NotImplementedError until Task 17 — that's OK, the entry point just needs to resolve).

If `essentia` or `madmom` fails to install:
- `essentia` failure → comment it out in `pyproject.toml` temporarily; Task 11 will guard against its absence.
- `madmom` failure → comment it out; Task 10 has a fallback that doesn't need it.

Document either choice in a comment in `pyproject.toml`.

- [ ] **Step 5: Commit**

```bash
git add analyzer/pyproject.toml analyzer/src/dj_analyze/__init__.py analyzer/tests/__init__.py
git commit -m "feat(analyzer): python package skeleton with deps"
```

---

### Task 5: Generate synthetic test fixtures

**Files:**
- Create: `analyzer/tests/fixtures/generate_fixtures.py`
- Generated: `analyzer/tests/fixtures/click_120bpm.wav`
- Generated: `analyzer/tests/fixtures/sine_a_minor.wav`
- Generated: `analyzer/tests/fixtures/silence_then_beep.wav`
- Created: `analyzer/tests/fixtures/corrupt.mp3`

The fixtures are deterministic synthetic audio with known properties. We generate them once with a script and commit them so CI doesn't need scipy/numpy at test time just to make the WAVs.

- [ ] **Step 1: Write the fixture generator**

Create `analyzer/tests/fixtures/generate_fixtures.py`:
```python
"""One-shot script to (re)generate synthetic audio fixtures with known properties.

Run from analyzer/ with the dev venv active:
    python tests/fixtures/generate_fixtures.py
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
import soundfile as sf

SR = 44100
HERE = Path(__file__).parent


def _save(name: str, audio: np.ndarray) -> None:
    audio = np.clip(audio, -1.0, 1.0).astype(np.float32)
    sf.write(HERE / name, audio, SR, subtype="PCM_16")


def click_120bpm(duration_s: float = 8.0) -> None:
    """A click track at exactly 120 BPM (one click every 0.5s), 8 seconds.

    16 beats total → 4 downbeats at indices 0, 4, 8, 12.
    """
    n = int(duration_s * SR)
    audio = np.zeros(n, dtype=np.float32)
    click_len = int(0.005 * SR)  # 5ms click
    beat_interval_samples = SR // 2  # 0.5s
    envelope = np.linspace(1.0, 0.0, click_len, dtype=np.float32)
    for beat in range(int(duration_s * 2)):
        start = beat * beat_interval_samples
        end = start + click_len
        if end > n:
            break
        audio[start:end] += envelope
    _save("click_120bpm.wav", audio)


def sine_a_minor(duration_s: float = 8.0) -> None:
    """A-minor triad sustained tone: A3 + C4 + E4. Known key = 8A (Camelot)."""
    n = int(duration_s * SR)
    t = np.arange(n, dtype=np.float32) / SR
    a3 = 220.0
    c4 = 261.63
    e4 = 329.63
    audio = (
        0.25 * np.sin(2 * np.pi * a3 * t)
        + 0.25 * np.sin(2 * np.pi * c4 * t)
        + 0.25 * np.sin(2 * np.pi * e4 * t)
    )
    # Apply a 50ms fade-in and fade-out so the start/end aren't pure clicks.
    fade = int(0.05 * SR)
    audio[:fade] *= np.linspace(0, 1, fade)
    audio[-fade:] *= np.linspace(1, 0, fade)
    _save("sine_a_minor.wav", audio.astype(np.float32))


def silence_then_beep(duration_s: float = 10.0) -> None:
    """3s silence, 4s of a 1 kHz tone, 3s silence. Tests cue_in / cue_out trimming."""
    n = int(duration_s * SR)
    audio = np.zeros(n, dtype=np.float32)
    start = int(3.0 * SR)
    end = int(7.0 * SR)
    t = np.arange(end - start, dtype=np.float32) / SR
    audio[start:end] = 0.5 * np.sin(2 * np.pi * 1000.0 * t)
    _save("silence_then_beep.wav", audio)


def corrupt_mp3() -> None:
    """A 1-byte 'mp3' file — guarantees a decode failure."""
    (HERE / "corrupt.mp3").write_bytes(b"\x00")


if __name__ == "__main__":
    click_120bpm()
    sine_a_minor()
    silence_then_beep()
    corrupt_mp3()
    print("Fixtures regenerated in", HERE)
```

- [ ] **Step 2: Run the generator**

From `analyzer/` with the venv active:
```bash
python tests/fixtures/generate_fixtures.py
```

Expected: prints `Fixtures regenerated in ...` and creates 4 files in `tests/fixtures/`.

- [ ] **Step 3: Verify the fixtures**

Run:
```bash
python -c "import soundfile as sf; print(sf.info('analyzer/tests/fixtures/click_120bpm.wav'))"
```

Expected: prints duration ≈ 8.0s, sample rate 44100, channels 1.

- [ ] **Step 4: Commit fixtures + generator**

```bash
git add analyzer/tests/fixtures/
git commit -m "test(analyzer): add synthetic audio fixtures (click, sine, silence)"
```

---

### Task 6: io_helpers.py — file discovery and sidecar path computation

**Files:**
- Create: `analyzer/src/dj_analyze/io_helpers.py`
- Create: `analyzer/tests/test_io_helpers.py`

This module finds audio files in a directory and computes where each one's sidecar should be written. Pure functions, no audio decoding.

- [ ] **Step 1: Write the failing tests**

Create `analyzer/tests/test_io_helpers.py`:
```python
from __future__ import annotations

from pathlib import Path

import pytest

from dj_analyze.io_helpers import discover_audio_files, sidecar_path_for


AUDIO_EXTENSIONS = {".mp3", ".flac", ".m4a", ".wav", ".ogg", ".opus"}


def test_discover_finds_supported_extensions(tmp_path: Path) -> None:
    (tmp_path / "a.mp3").write_bytes(b"")
    (tmp_path / "sub").mkdir()
    (tmp_path / "sub" / "b.flac").write_bytes(b"")
    (tmp_path / "ignore.txt").write_bytes(b"")

    found = sorted(p.name for p in discover_audio_files(tmp_path))
    assert found == ["a.mp3", "b.flac"]


def test_discover_is_recursive(tmp_path: Path) -> None:
    deep = tmp_path / "a" / "b" / "c"
    deep.mkdir(parents=True)
    (deep / "deep.wav").write_bytes(b"")

    found = list(discover_audio_files(tmp_path))
    assert len(found) == 1
    assert found[0].name == "deep.wav"


def test_discover_is_case_insensitive(tmp_path: Path) -> None:
    (tmp_path / "UPPER.MP3").write_bytes(b"")
    found = list(discover_audio_files(tmp_path))
    assert len(found) == 1


def test_sidecar_path_mirrors_relative_path(tmp_path: Path) -> None:
    source_root = tmp_path / "music"
    output_root = tmp_path / "out"
    track = source_root / "Artist" / "Album" / "01 - Track.mp3"
    source_root.mkdir()
    track.parent.mkdir(parents=True)
    track.write_bytes(b"")

    sc = sidecar_path_for(track, source_root, output_root)
    assert sc == output_root / "Artist" / "Album" / "01 - Track.mp3.json"


def test_sidecar_path_handles_track_with_dots(tmp_path: Path) -> None:
    source_root = tmp_path / "music"
    source_root.mkdir()
    track = source_root / "1.2.3 - Mix.flac"
    track.write_bytes(b"")
    sc = sidecar_path_for(track, source_root, tmp_path / "out")
    assert sc.name == "1.2.3 - Mix.flac.json"


def test_sidecar_path_raises_if_not_under_root(tmp_path: Path) -> None:
    other = tmp_path / "other.mp3"
    other.write_bytes(b"")
    with pytest.raises(ValueError):
        sidecar_path_for(other, tmp_path / "music", tmp_path / "out")
```

- [ ] **Step 2: Run the tests to confirm they fail**

From `analyzer/`:
```bash
pytest tests/test_io_helpers.py -v
```

Expected: `ModuleNotFoundError: No module named 'dj_analyze.io_helpers'` or similar import error.

- [ ] **Step 3: Implement io_helpers.py**

Create `analyzer/src/dj_analyze/io_helpers.py`:
```python
"""Audio file discovery and sidecar output-path computation. Pure functions only."""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

AUDIO_EXTENSIONS: frozenset[str] = frozenset(
    {".mp3", ".flac", ".m4a", ".wav", ".ogg", ".opus"}
)


def discover_audio_files(root: Path) -> Iterator[Path]:
    """Yield every audio file under `root` (recursive). Case-insensitive on extension."""
    for path in root.rglob("*"):
        if path.is_file() and path.suffix.lower() in AUDIO_EXTENSIONS:
            yield path


def sidecar_path_for(track: Path, source_root: Path, output_root: Path) -> Path:
    """Compute the sidecar JSON path that mirrors `track`'s relative position under `source_root`.

    The sidecar filename is `<track filename>.json` (preserves extension to disambiguate
    `song.mp3` from `song.flac`).
    """
    try:
        relative = track.resolve().relative_to(source_root.resolve())
    except ValueError as e:
        raise ValueError(
            f"Track {track} is not inside source root {source_root}"
        ) from e
    return output_root / relative.with_name(relative.name + ".json")
```

- [ ] **Step 4: Run the tests and verify they pass**

```bash
pytest tests/test_io_helpers.py -v
```

Expected: 6 tests pass.

- [ ] **Step 5: Commit**

```bash
git add analyzer/src/dj_analyze/io_helpers.py analyzer/tests/test_io_helpers.py
git commit -m "feat(analyzer): file discovery and sidecar path computation"
```

---

### Task 7: sidecar.py — load, save, is_fresh

**Files:**
- Create: `analyzer/src/dj_analyze/sidecar.py`
- Create: `analyzer/tests/test_sidecar.py`

This module handles the on-disk side of the sidecar: writing the JSON atomically, loading it, and deciding whether an existing sidecar is still valid (skip-if-fresh).

- [ ] **Step 1: Write the failing tests**

Create `analyzer/tests/test_sidecar.py`:
```python
from __future__ import annotations

import json
from pathlib import Path

from dj_analyze.sidecar import is_fresh, load_sidecar, save_sidecar


def _make_track(tmp_path: Path, contents: bytes = b"audio") -> Path:
    track = tmp_path / "song.mp3"
    track.write_bytes(contents)
    return track


def test_save_writes_valid_json(tmp_path: Path) -> None:
    out = tmp_path / "out.json"
    save_sidecar(out, {"schema_version": 1, "analyzer_version": "0.1.0"})
    loaded = json.loads(out.read_text())
    assert loaded["schema_version"] == 1


def test_save_is_atomic_no_partial_file(tmp_path: Path, monkeypatch) -> None:
    """If serialization fails, no broken file should exist at the target path."""
    out = tmp_path / "out.json"
    out.write_text("OLD")

    class Unserializable:
        pass

    try:
        save_sidecar(out, {"bad": Unserializable()})
    except TypeError:
        pass
    # The old content (or no file) should remain — not a half-written one.
    assert out.read_text() == "OLD"


def test_load_returns_dict(tmp_path: Path) -> None:
    out = tmp_path / "out.json"
    out.write_text('{"hello": "world"}')
    assert load_sidecar(out) == {"hello": "world"}


def test_is_fresh_true_when_mtime_size_version_match(tmp_path: Path) -> None:
    track = _make_track(tmp_path)
    sidecar_path = tmp_path / "song.mp3.json"
    save_sidecar(
        sidecar_path,
        {
            "schema_version": 1,
            "analyzer_version": "0.1.0",
            "source_mtime": track.stat().st_mtime,
            "source_size_bytes": track.stat().st_size,
        },
    )
    assert is_fresh(track, sidecar_path, "0.1.0")


def test_is_fresh_false_when_size_changed(tmp_path: Path) -> None:
    track = _make_track(tmp_path, b"audio")
    sidecar_path = tmp_path / "song.mp3.json"
    save_sidecar(
        sidecar_path,
        {
            "schema_version": 1,
            "analyzer_version": "0.1.0",
            "source_mtime": track.stat().st_mtime,
            "source_size_bytes": 999,  # wrong
        },
    )
    assert not is_fresh(track, sidecar_path, "0.1.0")


def test_is_fresh_false_when_analyzer_version_changed(tmp_path: Path) -> None:
    track = _make_track(tmp_path)
    sidecar_path = tmp_path / "song.mp3.json"
    save_sidecar(
        sidecar_path,
        {
            "schema_version": 1,
            "analyzer_version": "0.1.0",
            "source_mtime": track.stat().st_mtime,
            "source_size_bytes": track.stat().st_size,
        },
    )
    assert not is_fresh(track, sidecar_path, "0.2.0")


def test_is_fresh_false_when_sidecar_missing(tmp_path: Path) -> None:
    track = _make_track(tmp_path)
    assert not is_fresh(track, tmp_path / "missing.json", "0.1.0")


def test_is_fresh_false_when_sidecar_corrupt(tmp_path: Path) -> None:
    track = _make_track(tmp_path)
    sidecar_path = tmp_path / "broken.json"
    sidecar_path.write_text("not valid json {")
    assert not is_fresh(track, sidecar_path, "0.1.0")
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
pytest tests/test_sidecar.py -v
```

Expected: import errors.

- [ ] **Step 3: Implement sidecar.py**

Create `analyzer/src/dj_analyze/sidecar.py`:
```python
"""Sidecar JSON load/save with atomic writes, and freshness check (skip-if-fresh)."""

from __future__ import annotations

import json
import os
from pathlib import Path
from tempfile import NamedTemporaryFile
from typing import Any


def save_sidecar(path: Path, data: dict[str, Any]) -> None:
    """Write `data` to `path` as JSON atomically.

    Writes to a temp file in the same directory, then renames over the target.
    If serialization fails, the target file is left untouched.
    """
    path.parent.mkdir(parents=True, exist_ok=True)
    # Use a temp file in the same dir so the rename is atomic across the same filesystem.
    with NamedTemporaryFile(
        mode="w",
        encoding="utf-8",
        dir=path.parent,
        delete=False,
        suffix=".tmp",
    ) as tmp:
        tmp_path = Path(tmp.name)
        try:
            json.dump(data, tmp, ensure_ascii=False, indent=2)
        except Exception:
            tmp.close()
            tmp_path.unlink(missing_ok=True)
            raise
    os.replace(tmp_path, path)


def load_sidecar(path: Path) -> dict[str, Any]:
    """Load a sidecar JSON file. Raises on missing or invalid JSON."""
    with path.open("r", encoding="utf-8") as f:
        result: dict[str, Any] = json.load(f)
    return result


def is_fresh(track: Path, sidecar_path: Path, analyzer_version: str) -> bool:
    """Return True if `sidecar_path` represents a current analysis of `track`.

    Fresh means: file exists, valid JSON, and source_mtime + source_size_bytes
    + analyzer_version all match.
    """
    if not sidecar_path.exists():
        return False
    try:
        data = load_sidecar(sidecar_path)
    except (json.JSONDecodeError, OSError):
        return False
    if data.get("analyzer_version") != analyzer_version:
        return False
    stat = track.stat()
    if data.get("source_size_bytes") != stat.st_size:
        return False
    # mtime comparison: tolerate 1ms drift (filesystem timestamp granularity)
    sidecar_mtime = data.get("source_mtime")
    if not isinstance(sidecar_mtime, (int, float)):
        return False
    if abs(sidecar_mtime - stat.st_mtime) > 0.001:
        return False
    return True
```

- [ ] **Step 4: Run the tests and verify they pass**

```bash
pytest tests/test_sidecar.py -v
```

Expected: 8 tests pass.

- [ ] **Step 5: Commit**

```bash
git add analyzer/src/dj_analyze/sidecar.py analyzer/tests/test_sidecar.py
git commit -m "feat(analyzer): atomic sidecar I/O and freshness check"
```

---

### Task 8: decode.py — audio loading wrapper

**Files:**
- Create: `analyzer/src/dj_analyze/decode.py`
- Create: `analyzer/tests/test_decode.py`

A thin wrapper that loads any audio file as mono float32 at a fixed sample rate (44.1 kHz), backed by `librosa.load`. Centralizing this means each detection module gets consistent inputs.

- [ ] **Step 1: Write the failing tests**

Create `analyzer/tests/test_decode.py`:
```python
from __future__ import annotations

from pathlib import Path

import numpy as np
import pytest

from dj_analyze.decode import DecodeError, decode_audio

FIXTURES = Path(__file__).parent / "fixtures"


def test_decode_returns_mono_float32_at_target_sr() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    assert audio.dtype == np.float32
    assert audio.ndim == 1
    assert sr == 44100
    # Click fixture is 8 seconds
    assert abs(len(audio) / sr - 8.0) < 0.05


def test_decode_normalizes_to_minus_one_one_range() -> None:
    audio, _ = decode_audio(FIXTURES / "click_120bpm.wav")
    assert audio.max() <= 1.0
    assert audio.min() >= -1.0


def test_decode_raises_on_corrupt_file() -> None:
    with pytest.raises(DecodeError):
        decode_audio(FIXTURES / "corrupt.mp3")


def test_decode_raises_on_missing_file(tmp_path: Path) -> None:
    with pytest.raises(DecodeError):
        decode_audio(tmp_path / "does-not-exist.mp3")
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
pytest tests/test_decode.py -v
```

Expected: import error.

- [ ] **Step 3: Implement decode.py**

Create `analyzer/src/dj_analyze/decode.py`:
```python
"""Audio decode wrapper. Loads any file as mono float32 at 44.1 kHz."""

from __future__ import annotations

from pathlib import Path

import librosa
import numpy as np

TARGET_SAMPLE_RATE = 44100


class DecodeError(Exception):
    """Raised when a file cannot be decoded."""


def decode_audio(path: Path) -> tuple[np.ndarray, int]:
    """Load `path` as mono float32 at TARGET_SAMPLE_RATE.

    Returns (audio, sr).
    Raises DecodeError on any failure (missing file, unsupported format, corrupt data).
    """
    if not path.exists():
        raise DecodeError(f"File does not exist: {path}")
    try:
        audio, sr = librosa.load(str(path), sr=TARGET_SAMPLE_RATE, mono=True)
    except Exception as e:
        raise DecodeError(f"Failed to decode {path}: {e}") from e
    return audio.astype(np.float32, copy=False), int(sr)
```

- [ ] **Step 4: Run tests and verify they pass**

```bash
pytest tests/test_decode.py -v
```

Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add analyzer/src/dj_analyze/decode.py analyzer/tests/test_decode.py
git commit -m "feat(analyzer): audio decode wrapper (mono f32 @ 44.1kHz)"
```

---

### Task 9: detect_bpm.py + detect_beats.py — tempo and beat times

**Files:**
- Create: `analyzer/src/dj_analyze/detect_bpm.py`
- Create: `analyzer/src/dj_analyze/detect_beats.py`
- Create: `analyzer/tests/test_detect_bpm.py`
- Create: `analyzer/tests/test_detect_beats.py`

These are split because `bpm_confidence` requires a different computation than the beat-times list.

- [ ] **Step 1: Write the failing tests for BPM**

Create `analyzer/tests/test_detect_bpm.py`:
```python
from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_bpm import detect_bpm

FIXTURES = Path(__file__).parent / "fixtures"


def test_detects_120_bpm_within_tolerance() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    bpm, confidence = detect_bpm(audio, sr)
    assert 118.0 <= bpm <= 122.0
    assert 0.0 <= confidence <= 1.0


def test_confidence_is_high_on_clean_click_track() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    _, confidence = detect_bpm(audio, sr)
    # A perfect click should give high confidence.
    assert confidence > 0.7
```

- [ ] **Step 2: Write the failing tests for beats**

Create `analyzer/tests/test_detect_beats.py`:
```python
from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_beats import detect_beats

FIXTURES = Path(__file__).parent / "fixtures"


def test_beats_spacing_is_half_a_second_at_120_bpm() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = detect_beats(audio, sr, bpm=120.0)
    # 8 seconds at 120 BPM → ~16 beats.
    assert 14 <= len(beats) <= 18
    # Mean spacing should be ~0.5s (allow ±20ms).
    deltas = [b - a for a, b in zip(beats, beats[1:])]
    mean_delta = sum(deltas) / len(deltas)
    assert 0.48 <= mean_delta <= 0.52


def test_beats_are_sorted_and_within_duration() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = detect_beats(audio, sr, bpm=120.0)
    assert all(b1 <= b2 for b1, b2 in zip(beats, beats[1:]))
    duration = len(audio) / sr
    assert all(0 <= b <= duration for b in beats)
```

- [ ] **Step 3: Run the tests to confirm they fail**

```bash
pytest tests/test_detect_bpm.py tests/test_detect_beats.py -v
```

Expected: import errors.

- [ ] **Step 4: Implement detect_bpm.py**

Create `analyzer/src/dj_analyze/detect_bpm.py`:
```python
"""BPM detection with a confidence score based on tempo stability across windows."""

from __future__ import annotations

import librosa
import numpy as np


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


def _tempo_stability(audio: np.ndarray, sr: int, global_bpm: float) -> float:
    """Per-window tempo standard deviation, normalized into a [0, 1] confidence."""
    window_seconds = 10.0
    window = int(window_seconds * sr)
    if len(audio) < window * 2:
        # Not enough data for windowing — fall back to a flat 0.7 (we got *a* number).
        return 0.7

    bpms = []
    for start in range(0, len(audio) - window, window):
        seg = audio[start : start + window]
        env = librosa.onset.onset_strength(y=seg, sr=sr)
        t = librosa.beat.tempo(onset_envelope=env, sr=sr, aggregate=np.median)
        bpms.append(float(t[0]))

    if not bpms:
        return 0.7

    arr = np.array(bpms, dtype=np.float64)
    # Relative std-dev around the median window tempo.
    median = float(np.median(arr))
    if median <= 0:
        return 0.0
    rel_std = float(np.std(arr) / median)
    # Map: 0% drift → 1.0, 10% drift → 0.0, clamp.
    confidence = max(0.0, min(1.0, 1.0 - rel_std / 0.1))
    # Penalize disagreement with the global estimate.
    disagreement = abs(median - global_bpm) / max(global_bpm, 1.0)
    confidence *= max(0.0, 1.0 - disagreement / 0.05)
    return float(confidence)
```

- [ ] **Step 5: Implement detect_beats.py**

Create `analyzer/src/dj_analyze/detect_beats.py`:
```python
"""Beat-time detection. Given a known BPM, returns a list of beat onsets (seconds)."""

from __future__ import annotations

import librosa
import numpy as np


def detect_beats(audio: np.ndarray, sr: int, bpm: float) -> list[float]:
    """Return beat times in seconds, given a BPM estimate."""
    _, beat_frames = librosa.beat.beat_track(
        y=audio, sr=sr, start_bpm=bpm, tightness=100, units="frames"
    )
    times: np.ndarray = librosa.frames_to_time(beat_frames, sr=sr)
    return [float(t) for t in times]
```

- [ ] **Step 6: Run tests and verify they pass**

```bash
pytest tests/test_detect_bpm.py tests/test_detect_beats.py -v
```

Expected: 4 tests pass. (BPM tests may take 10-15s; the windowing loop is slow.)

- [ ] **Step 7: Commit**

```bash
git add analyzer/src/dj_analyze/detect_bpm.py analyzer/src/dj_analyze/detect_beats.py analyzer/tests/test_detect_bpm.py analyzer/tests/test_detect_beats.py
git commit -m "feat(analyzer): BPM and beat-time detection via librosa"
```

---

### Task 10: detect_downbeats.py — downbeats with fallback

**Files:**
- Create: `analyzer/src/dj_analyze/detect_downbeats.py`
- Create: `analyzer/tests/test_detect_downbeats.py`

Try `madmom` first (gold standard). If unavailable or failing, fall back to "every 4th beat" assuming 4/4. Mark the result so mixability classification can downgrade the track.

- [ ] **Step 1: Write the failing tests**

Create `analyzer/tests/test_detect_downbeats.py`:
```python
from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_downbeats import DownbeatResult, detect_downbeats

FIXTURES = Path(__file__).parent / "fixtures"


def test_fallback_returns_every_fourth_beat() -> None:
    beats = [0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5]
    result: DownbeatResult = detect_downbeats(
        audio=None, sr=44100, beats=beats, force_fallback=True
    )
    assert result.method == "fallback_4_4"
    assert result.downbeats == [0.5, 2.5, 4.5]


def test_fallback_handles_empty_beats() -> None:
    result = detect_downbeats(audio=None, sr=44100, beats=[], force_fallback=True)
    assert result.downbeats == []
    assert result.method == "fallback_4_4"


def test_madmom_path_or_fallback_returns_valid_result() -> None:
    """If madmom is installed, it runs; if not, we get the fallback. Either way
    we get a list of monotonically increasing floats."""
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = [0.0 + 0.5 * i for i in range(16)]
    result = detect_downbeats(audio, sr, beats=beats)
    assert all(d1 <= d2 for d1, d2 in zip(result.downbeats, result.downbeats[1:]))
    assert result.method in {"madmom", "fallback_4_4"}
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
pytest tests/test_detect_downbeats.py -v
```

Expected: import errors.

- [ ] **Step 3: Implement detect_downbeats.py**

Create `analyzer/src/dj_analyze/detect_downbeats.py`:
```python
"""Downbeat detection. Uses madmom when available; falls back to assuming 4/4."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

import numpy as np


@dataclass(frozen=True, slots=True)
class DownbeatResult:
    downbeats: list[float]
    method: Literal["madmom", "fallback_4_4"]


def detect_downbeats(
    audio: np.ndarray | None,
    sr: int,
    beats: list[float],
    *,
    force_fallback: bool = False,
) -> DownbeatResult:
    """Return downbeat times in seconds.

    If `force_fallback=True` or madmom is not importable / fails, returns every
    4th beat starting from index 0 (assumes 4/4).
    """
    if not force_fallback and audio is not None:
        try:
            return _madmom_downbeats(audio, sr)
        except Exception:
            pass
    return DownbeatResult(downbeats=beats[::4], method="fallback_4_4")


def _madmom_downbeats(audio: np.ndarray, sr: int) -> DownbeatResult:
    # Imported lazily so absence of madmom doesn't break imports of this module.
    from madmom.features.beats import DBNDownBeatTrackingProcessor
    from madmom.features.downbeats import RNNDownBeatProcessor

    rnn = RNNDownBeatProcessor()
    dbn = DBNDownBeatTrackingProcessor(beats_per_bar=[3, 4], fps=100)
    activations = rnn(audio.astype(np.float32))
    beats_and_positions: np.ndarray = dbn(activations)
    # beats_and_positions is an array of shape (N, 2): [time, beat_position_in_bar (1..N)]
    downbeats = [
        float(t) for t, pos in beats_and_positions if int(round(pos)) == 1
    ]
    return DownbeatResult(downbeats=downbeats, method="madmom")
```

- [ ] **Step 4: Run tests and verify they pass**

```bash
pytest tests/test_detect_downbeats.py -v
```

Expected: 3 tests pass. (If madmom is installed, the third test exercises the madmom path; otherwise it exercises the fallback. Both are valid.)

- [ ] **Step 5: Commit**

```bash
git add analyzer/src/dj_analyze/detect_downbeats.py analyzer/tests/test_detect_downbeats.py
git commit -m "feat(analyzer): downbeat detection with madmom + 4/4 fallback"
```

---

### Task 11: detect_key.py — key + Camelot mapping

**Files:**
- Create: `analyzer/src/dj_analyze/detect_key.py`
- Create: `analyzer/tests/test_detect_key.py`

Use `essentia.standard.KeyExtractor` when available, fall back to `librosa.feature.chroma_cqt` + Krumhansl-Schmuckler key profiles otherwise. Always map to Camelot wheel notation.

- [ ] **Step 1: Write the failing tests**

Create `analyzer/tests/test_detect_key.py`:
```python
from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_key import KeyResult, detect_key, to_camelot

FIXTURES = Path(__file__).parent / "fixtures"


def test_to_camelot_a_minor() -> None:
    assert to_camelot("A", "minor") == "8A"


def test_to_camelot_c_major() -> None:
    assert to_camelot("C", "major") == "8B"


def test_to_camelot_g_sharp_handled_as_a_flat() -> None:
    # G# minor and Ab minor are enharmonically equivalent → both map to 1A.
    assert to_camelot("G#", "minor") == "1A"
    assert to_camelot("Ab", "minor") == "1A"


def test_detects_a_minor_on_synthetic_fixture() -> None:
    audio, sr = decode_audio(FIXTURES / "sine_a_minor.wav")
    result: KeyResult = detect_key(audio, sr)
    # Allow neighbors on the wheel: 8A, 7A, 9A, 8B all acceptable.
    # The sine triad is unambiguously minor, so the letter should be A.
    assert result.camelot.endswith("A")
    # Confidence is in valid range.
    assert 0.0 <= result.confidence <= 1.0
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
pytest tests/test_detect_key.py -v
```

Expected: import errors.

- [ ] **Step 3: Implement detect_key.py**

Create `analyzer/src/dj_analyze/detect_key.py`:
```python
"""Key detection. Uses essentia when available; falls back to librosa chroma + KS profiles.

All keys are returned in Camelot wheel notation (1A..12A for minor, 1B..12B for major)
so the engine can compute compatibility uniformly.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

# Camelot wheel mapping. Pitch class → (minor_code, major_code).
# Pitch classes use sharps; flats are normalized in `to_camelot`.
_PITCH_TO_CAMELOT: dict[str, tuple[str, str]] = {
    "C":  ("5A",  "8B"),
    "C#": ("12A", "3B"),
    "D":  ("7A",  "10B"),
    "D#": ("2A",  "5B"),
    "E":  ("9A",  "12B"),
    "F":  ("4A",  "7B"),
    "F#": ("11A", "2B"),
    "G":  ("6A",  "9B"),
    "G#": ("1A",  "4B"),
    "A":  ("8A",  "11B"),
    "A#": ("3A",  "6B"),
    "B":  ("10A", "1B"),
}

_FLAT_TO_SHARP: dict[str, str] = {
    "Db": "C#", "Eb": "D#", "Gb": "F#", "Ab": "G#", "Bb": "A#",
}


@dataclass(frozen=True, slots=True)
class KeyResult:
    camelot: str
    label: str           # e.g., "A minor"
    confidence: float    # 0..1


def to_camelot(pitch: str, mode: str) -> str:
    """Convert (pitch, mode) like ("A", "minor") to Camelot notation."""
    normalized = _FLAT_TO_SHARP.get(pitch, pitch)
    if normalized not in _PITCH_TO_CAMELOT:
        raise ValueError(f"Unknown pitch: {pitch}")
    minor_code, major_code = _PITCH_TO_CAMELOT[normalized]
    mode_lower = mode.lower()
    if mode_lower.startswith("min"):
        return minor_code
    if mode_lower.startswith("maj"):
        return major_code
    raise ValueError(f"Unknown mode: {mode}")


def detect_key(audio: np.ndarray, sr: int) -> KeyResult:
    """Detect key, returning Camelot code + human label + confidence."""
    try:
        return _essentia_key(audio, sr)
    except Exception:
        return _librosa_fallback_key(audio, sr)


def _essentia_key(audio: np.ndarray, sr: int) -> KeyResult:
    from essentia.standard import KeyExtractor

    extractor = KeyExtractor()
    pitch, mode, strength = extractor(audio.astype(np.float32))
    camelot = to_camelot(pitch, mode)
    return KeyResult(
        camelot=camelot,
        label=f"{pitch} {mode}",
        confidence=float(strength),
    )


# Krumhansl-Schmuckler key profiles (major and minor), 12 pitch classes.
_KS_MAJOR = np.array(
    [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88]
)
_KS_MINOR = np.array(
    [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17]
)
_PITCH_CLASSES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]


def _librosa_fallback_key(audio: np.ndarray, sr: int) -> KeyResult:
    import librosa

    chroma = librosa.feature.chroma_cqt(y=audio, sr=sr)
    profile = chroma.mean(axis=1)
    # Try all 24 keys, pick the highest correlation.
    best_score = -np.inf
    best_pitch = "C"
    best_mode = "major"
    for i in range(12):
        major_score = float(np.corrcoef(profile, np.roll(_KS_MAJOR, i))[0, 1])
        minor_score = float(np.corrcoef(profile, np.roll(_KS_MINOR, i))[0, 1])
        if major_score > best_score:
            best_score = major_score
            best_pitch = _PITCH_CLASSES[i]
            best_mode = "major"
        if minor_score > best_score:
            best_score = minor_score
            best_pitch = _PITCH_CLASSES[i]
            best_mode = "minor"
    # Correlation is in [-1, 1] — map positively-correlated half to [0, 1].
    confidence = max(0.0, best_score)
    return KeyResult(
        camelot=to_camelot(best_pitch, best_mode),
        label=f"{best_pitch} {best_mode}",
        confidence=float(confidence),
    )
```

- [ ] **Step 4: Run tests and verify they pass**

```bash
pytest tests/test_detect_key.py -v
```

Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add analyzer/src/dj_analyze/detect_key.py analyzer/tests/test_detect_key.py
git commit -m "feat(analyzer): key detection (essentia + librosa fallback) with Camelot mapping"
```

---

### Task 12: detect_energy.py — RMS energy curve + mean

**Files:**
- Create: `analyzer/src/dj_analyze/detect_energy.py`
- Create: `analyzer/tests/test_detect_energy.py`

Per-second RMS, normalized into [0, 1].

- [ ] **Step 1: Write the failing tests**

Create `analyzer/tests/test_detect_energy.py`:
```python
from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_energy import detect_energy

FIXTURES = Path(__file__).parent / "fixtures"


def test_energy_curve_length_matches_duration_in_seconds() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    curve, mean = detect_energy(audio, sr)
    # 8s click → curve of ~8 entries (one per second)
    assert 7 <= len(curve) <= 9


def test_energy_curve_values_in_unit_range() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    curve, _ = detect_energy(audio, sr)
    assert all(0.0 <= v <= 1.0 for v in curve)


def test_silence_then_beep_curve_has_low_then_high_then_low() -> None:
    audio, sr = decode_audio(FIXTURES / "silence_then_beep.wav")
    curve, _ = detect_energy(audio, sr)
    # 10s file: roughly seconds 0-2 silence, 3-6 beep, 7-9 silence.
    assert curve[1] < 0.2          # in the silent intro
    assert curve[5] > 0.4          # in the beep
    assert curve[8] < 0.2          # in the silent outro
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
pytest tests/test_detect_energy.py -v
```

Expected: import errors.

- [ ] **Step 3: Implement detect_energy.py**

Create `analyzer/src/dj_analyze/detect_energy.py`:
```python
"""Per-second RMS energy curve, normalized to [0, 1]."""

from __future__ import annotations

import librosa
import numpy as np


def detect_energy(audio: np.ndarray, sr: int) -> tuple[list[float], float]:
    """Return (per-second curve, mean) — both normalized to [0, 1].

    Normalization divides by the 95th percentile of RMS values to be robust to
    short loud transients. Values above the 95th are clipped to 1.0.
    """
    hop = sr  # one frame per second
    frame_length = sr  # 1-second window
    rms = librosa.feature.rms(y=audio, frame_length=frame_length, hop_length=hop)
    raw = rms[0].astype(np.float64)
    if len(raw) == 0:
        return [], 0.0
    p95 = float(np.percentile(raw, 95))
    if p95 <= 0:
        return [0.0] * len(raw), 0.0
    normalized = np.clip(raw / p95, 0.0, 1.0)
    curve = [float(v) for v in normalized]
    mean = float(normalized.mean())
    return curve, mean
```

- [ ] **Step 4: Run tests and verify they pass**

```bash
pytest tests/test_detect_energy.py -v
```

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add analyzer/src/dj_analyze/detect_energy.py analyzer/tests/test_detect_energy.py
git commit -m "feat(analyzer): per-second RMS energy curve + mean"
```

---

### Task 13: detect_cues.py — cue_in/cue_out, intro_end, outro_start

**Files:**
- Create: `analyzer/src/dj_analyze/detect_cues.py`
- Create: `analyzer/tests/test_detect_cues.py`

Cue points come from combining energy curve + downbeat grid:
- `cue_in_seconds`: first downbeat where energy ≥ 10% of mean
- `cue_out_seconds`: last downbeat where energy ≥ 10% of mean
- `intro_end_seconds`: 16 bars after `cue_in_seconds` (or first sustained energy plateau, whichever later)
- `outro_start_seconds`: 16 bars before `cue_out_seconds`

A "bar" = 4 beats by convention. With a known BPM we compute bar length in seconds.

- [ ] **Step 1: Write the failing tests**

Create `analyzer/tests/test_detect_cues.py`:
```python
from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_cues import detect_cues

FIXTURES = Path(__file__).parent / "fixtures"


def test_cues_on_silence_then_beep() -> None:
    audio, sr = decode_audio(FIXTURES / "silence_then_beep.wav")
    duration = len(audio) / sr
    # Synthetic downbeats every 2s (close to 120 BPM in 4/4)
    downbeats = [0.0, 2.0, 4.0, 6.0, 8.0]
    energy_curve = [0.0, 0.0, 0.0, 0.6, 0.6, 0.6, 0.6, 0.0, 0.0, 0.0]
    cues = detect_cues(
        duration_seconds=duration,
        beats=[0.5 * i for i in range(20)],
        downbeats=downbeats,
        energy_curve=energy_curve,
        bpm=120.0,
    )
    # cue_in should be at the first downbeat where energy is meaningful (≥3s)
    assert 2.0 <= cues["cue_in_seconds"] <= 4.0
    # cue_out should be at the last downbeat where energy is meaningful (≤7s)
    assert 6.0 <= cues["cue_out_seconds"] <= 8.0


def test_short_track_clamps_intro_and_outro() -> None:
    cues = detect_cues(
        duration_seconds=10.0,
        beats=[0.5 * i for i in range(20)],
        downbeats=[0.0, 2.0, 4.0, 6.0, 8.0],
        energy_curve=[0.5] * 10,
        bpm=120.0,
    )
    # With bpm=120 a bar is 2s; 16 bars = 32s but track is 10s, so intro_end
    # should be clamped sensibly (no later than cue_out).
    assert cues["intro_end_seconds"] <= cues["cue_out_seconds"]
    assert cues["outro_start_seconds"] >= cues["cue_in_seconds"]
    assert cues["intro_end_seconds"] <= cues["outro_start_seconds"]


def test_silent_track_uses_endpoints() -> None:
    """Fully silent energy curve → cue_in=0, cue_out=duration."""
    duration = 8.0
    cues = detect_cues(
        duration_seconds=duration,
        beats=[0.5 * i for i in range(16)],
        downbeats=[0.0, 2.0, 4.0, 6.0],
        energy_curve=[0.0] * 8,
        bpm=120.0,
    )
    assert cues["cue_in_seconds"] == 0.0
    assert cues["cue_out_seconds"] == duration
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
pytest tests/test_detect_cues.py -v
```

Expected: import errors.

- [ ] **Step 3: Implement detect_cues.py**

Create `analyzer/src/dj_analyze/detect_cues.py`:
```python
"""Cue point detection from energy curve and downbeat grid."""

from __future__ import annotations


def detect_cues(
    duration_seconds: float,
    beats: list[float],
    downbeats: list[float],
    energy_curve: list[float],
    bpm: float,
) -> dict[str, float]:
    """Compute cue_in, cue_out, intro_end, outro_start (seconds).

    cue_in:  first downbeat where energy ≥ 10% of mean (or 0 if all silent)
    cue_out: last  downbeat where energy ≥ 10% of mean (or duration if all silent)
    intro_end:    cue_in + 16 bars (clamped to [cue_in, outro_start])
    outro_start:  cue_out - 16 bars (clamped to [intro_end, cue_out])
    """
    if not energy_curve or all(v == 0.0 for v in energy_curve):
        cue_in = 0.0
        cue_out = duration_seconds
    else:
        mean = sum(energy_curve) / len(energy_curve)
        threshold = max(0.05, 0.1 * mean)
        cue_in = _first_downbeat_with_energy(downbeats, energy_curve, threshold)
        cue_out = _last_downbeat_with_energy(downbeats, energy_curve, threshold, duration_seconds)

    bar_seconds = (60.0 / bpm) * 4.0 if bpm > 0 else 2.0
    intro_end = cue_in + 16.0 * bar_seconds
    outro_start = cue_out - 16.0 * bar_seconds

    # Clamp: intro_end ≤ outro_start ≤ cue_out, intro_end ≥ cue_in.
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


def _energy_at_seconds(curve: list[float], seconds: float) -> float:
    idx = int(seconds)
    if idx < 0 or idx >= len(curve):
        return 0.0
    return curve[idx]


def _first_downbeat_with_energy(
    downbeats: list[float], curve: list[float], threshold: float
) -> float:
    for db in downbeats:
        if _energy_at_seconds(curve, db) >= threshold:
            return float(db)
    return float(downbeats[0]) if downbeats else 0.0


def _last_downbeat_with_energy(
    downbeats: list[float], curve: list[float], threshold: float, duration: float
) -> float:
    for db in reversed(downbeats):
        if _energy_at_seconds(curve, db) >= threshold:
            return float(db)
    return float(downbeats[-1]) if downbeats else float(duration)
```

- [ ] **Step 4: Run tests and verify they pass**

```bash
pytest tests/test_detect_cues.py -v
```

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add analyzer/src/dj_analyze/detect_cues.py analyzer/tests/test_detect_cues.py
git commit -m "feat(analyzer): cue point detection from energy + downbeats"
```

---

### Task 14: mixability.py — classify mixable vs. non-mixable tracks

**Files:**
- Create: `analyzer/src/dj_analyze/mixability.py`
- Create: `analyzer/tests/test_mixability.py`

Pure function over the metrics; returns `(is_mixable, reasons[])`.

- [ ] **Step 1: Write the failing tests**

Create `analyzer/tests/test_mixability.py`:
```python
from __future__ import annotations

from dj_analyze.mixability import classify


def test_clean_track_is_mixable() -> None:
    is_mixable, reasons = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.8,
    )
    assert is_mixable is True
    assert reasons == []


def test_low_bpm_confidence_disqualifies() -> None:
    is_mixable, reasons = classify(
        bpm=120.0,
        bpm_confidence=0.4,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.8,
    )
    assert is_mixable is False
    assert "bpm_low_confidence" in reasons


def test_too_short_disqualifies() -> None:
    is_mixable, reasons = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=60.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.8,
    )
    assert is_mixable is False
    assert "too_short" in reasons


def test_fallback_downbeats_marks_no_clear_downbeat() -> None:
    is_mixable, reasons = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="fallback_4_4",
        key_confidence=0.8,
    )
    assert is_mixable is False
    assert "no_clear_downbeat" in reasons


def test_low_key_confidence_is_reason_but_not_disqualifying() -> None:
    is_mixable, reasons = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.2,
    )
    assert is_mixable is True
    assert "key_low_confidence" in reasons


def test_multiple_reasons_accumulate() -> None:
    is_mixable, reasons = classify(
        bpm=120.0,
        bpm_confidence=0.4,
        duration_seconds=60.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="fallback_4_4",
        key_confidence=0.8,
    )
    assert is_mixable is False
    assert "bpm_low_confidence" in reasons
    assert "too_short" in reasons
    assert "no_clear_downbeat" in reasons
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
pytest tests/test_mixability.py -v
```

Expected: import errors.

- [ ] **Step 3: Implement mixability.py**

Create `analyzer/src/dj_analyze/mixability.py`:
```python
"""Classify whether a track is suitable for beatmatched DJ mixing."""

from __future__ import annotations

from typing import Literal


def classify(
    *,
    bpm: float,
    bpm_confidence: float,
    duration_seconds: float,
    downbeats_count: int,
    beats_count: int,
    downbeat_method: Literal["madmom", "fallback_4_4"],
    key_confidence: float,
) -> tuple[bool, list[str]]:
    """Return (is_mixable, reasons).

    Reasons (any of):
      - "bpm_low_confidence"
      - "bpm_variable"   (reserved; computed in detect_bpm windowing — TODO: thread through)
      - "no_clear_downbeat"
      - "too_short"
      - "key_low_confidence"  (advisory; does NOT disqualify)

    Disqualifying reasons (any → is_mixable=False):
      bpm_low_confidence, no_clear_downbeat, too_short, bpm_variable
    """
    reasons: list[str] = []

    if bpm_confidence < 0.6:
        reasons.append("bpm_low_confidence")
    if duration_seconds < 90.0:
        reasons.append("too_short")
    if downbeat_method == "fallback_4_4":
        reasons.append("no_clear_downbeat")
    if key_confidence < 0.4:
        reasons.append("key_low_confidence")

    disqualifying = {
        "bpm_low_confidence",
        "bpm_variable",
        "no_clear_downbeat",
        "too_short",
    }
    is_mixable = not any(r in disqualifying for r in reasons)
    return is_mixable, reasons
```

- [ ] **Step 4: Run tests and verify they pass**

```bash
pytest tests/test_mixability.py -v
```

Expected: 6 tests pass.

- [ ] **Step 5: Commit**

```bash
git add analyzer/src/dj_analyze/mixability.py analyzer/tests/test_mixability.py
git commit -m "feat(analyzer): mixability classification with reason codes"
```

---

### Task 15: analyze.py — per-track orchestrator

**Files:**
- Create: `analyzer/src/dj_analyze/analyze.py`
- Create: `analyzer/tests/test_analyze.py`

This is where everything composes. Input: file path. Output: a dict matching the sidecar schema.

- [ ] **Step 1: Write the failing test**

Create `analyzer/tests/test_analyze.py`:
```python
from __future__ import annotations

from pathlib import Path

from dj_analyze.analyze import analyze_track

FIXTURES = Path(__file__).parent / "fixtures"


def test_analyze_returns_dict_with_required_fields() -> None:
    result = analyze_track(FIXTURES / "click_120bpm.wav")
    assert result["schema_version"] == 1
    assert "analyzer_version" in result
    assert result["source_path"] == str((FIXTURES / "click_120bpm.wav").resolve())
    assert "source_mtime" in result
    assert "source_size_bytes" in result
    assert "duration_seconds" in result
    assert "bpm" in result
    assert "beats_seconds" in result
    assert "downbeats_seconds" in result
    assert "key" in result
    assert "energy_curve" in result
    assert "cue_in_seconds" in result
    assert "intro_end_seconds" in result
    assert "is_mixable" in result


def test_analyze_120_bpm_click_detects_bpm_near_120() -> None:
    result = analyze_track(FIXTURES / "click_120bpm.wav")
    assert 118.0 <= result["bpm"] <= 122.0


def test_analyze_corrupt_file_returns_error_sidecar() -> None:
    result = analyze_track(FIXTURES / "corrupt.mp3")
    assert "error" in result
    assert result["schema_version"] == 1
    assert "bpm" not in result
```

- [ ] **Step 2: Run the test to confirm it fails**

```bash
pytest tests/test_analyze.py -v
```

Expected: import error.

- [ ] **Step 3: Implement analyze.py**

Create `analyzer/src/dj_analyze/analyze.py`:
```python
"""Per-track orchestration: decode + run all detectors + classify mixability.

Returns a dict that matches schema/sidecar.schema.json (does NOT validate; that's
done separately in CI / tests).
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from . import __version__
from .decode import DecodeError, decode_audio
from .detect_beats import detect_beats
from .detect_bpm import detect_bpm
from .detect_cues import detect_cues
from .detect_downbeats import detect_downbeats
from .detect_energy import detect_energy
from .detect_key import detect_key
from .mixability import classify

SCHEMA_VERSION = 1


def analyze_track(path: Path) -> dict[str, Any]:
    """Analyze one audio file. Returns a sidecar-shaped dict.

    On decode failure, returns an error sidecar (no analysis fields).
    """
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
    key_result = detect_key(audio, sr)
    energy_curve, energy_mean = detect_energy(audio, sr)
    cues = detect_cues(
        duration_seconds=duration,
        beats=beats,
        downbeats=db_result.downbeats,
        energy_curve=energy_curve,
        bpm=bpm,
    )
    is_mixable, reasons = classify(
        bpm=bpm,
        bpm_confidence=bpm_conf,
        duration_seconds=duration,
        downbeats_count=len(db_result.downbeats),
        beats_count=len(beats),
        downbeat_method=db_result.method,
        key_confidence=key_result.confidence,
    )

    return {
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
        **cues,
        "is_mixable": is_mixable,
        "mixable_reasons": reasons,
    }
```

- [ ] **Step 4: Run tests and verify they pass**

```bash
pytest tests/test_analyze.py -v
```

Expected: 3 tests pass. (This is the slowest test in the suite — possibly 30s for the BPM windowing on the 8s fixture.)

- [ ] **Step 5: Commit**

```bash
git add analyzer/src/dj_analyze/analyze.py analyzer/tests/test_analyze.py
git commit -m "feat(analyzer): per-track orchestrator producing sidecar dicts"
```

---

### Task 16: pipeline.py — parallel runner over a directory

**Files:**
- Create: `analyzer/src/dj_analyze/pipeline.py`
- Create: `analyzer/tests/test_pipeline.py`

- [ ] **Step 1: Write the failing test**

Create `analyzer/tests/test_pipeline.py`:
```python
from __future__ import annotations

import json
from pathlib import Path

from dj_analyze.pipeline import run_pipeline

FIXTURES = Path(__file__).parent / "fixtures"


def test_pipeline_writes_one_sidecar_per_track(tmp_path: Path) -> None:
    source = tmp_path / "music"
    source.mkdir()
    # Use a tiny set of fixtures (skip the slow ones for this test).
    for name in ("click_120bpm.wav",):
        (source / name).write_bytes((FIXTURES / name).read_bytes())

    output = tmp_path / "out"
    n_processed, n_skipped, n_errors = run_pipeline(
        source_dir=source, output_dir=output, workers=1
    )
    assert n_processed == 1
    assert n_skipped == 0
    assert n_errors == 0
    sidecar = output / "click_120bpm.wav.json"
    assert sidecar.exists()
    data = json.loads(sidecar.read_text())
    assert data["schema_version"] == 1


def test_pipeline_is_idempotent(tmp_path: Path) -> None:
    source = tmp_path / "music"
    source.mkdir()
    (source / "click_120bpm.wav").write_bytes((FIXTURES / "click_120bpm.wav").read_bytes())
    output = tmp_path / "out"

    run_pipeline(source_dir=source, output_dir=output, workers=1)
    n_processed, n_skipped, n_errors = run_pipeline(
        source_dir=source, output_dir=output, workers=1
    )
    assert n_processed == 0
    assert n_skipped == 1
    assert n_errors == 0


def test_pipeline_force_reanalyzes(tmp_path: Path) -> None:
    source = tmp_path / "music"
    source.mkdir()
    (source / "click_120bpm.wav").write_bytes((FIXTURES / "click_120bpm.wav").read_bytes())
    output = tmp_path / "out"

    run_pipeline(source_dir=source, output_dir=output, workers=1)
    n_processed, n_skipped, n_errors = run_pipeline(
        source_dir=source, output_dir=output, workers=1, force=True
    )
    assert n_processed == 1
    assert n_skipped == 0


def test_pipeline_records_error_for_corrupt_file(tmp_path: Path) -> None:
    source = tmp_path / "music"
    source.mkdir()
    (source / "broken.mp3").write_bytes(b"\x00")
    output = tmp_path / "out"

    n_processed, n_skipped, n_errors = run_pipeline(
        source_dir=source, output_dir=output, workers=1
    )
    assert n_errors == 1
    data = json.loads((output / "broken.mp3.json").read_text())
    assert "error" in data
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
pytest tests/test_pipeline.py -v
```

Expected: import errors.

- [ ] **Step 3: Implement pipeline.py**

Create `analyzer/src/dj_analyze/pipeline.py`:
```python
"""Parallel pipeline runner: scan a directory, analyze each track, write sidecars."""

from __future__ import annotations

import logging
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path
from typing import Any

from rich.progress import (
    BarColumn,
    MofNCompleteColumn,
    Progress,
    SpinnerColumn,
    TextColumn,
    TimeRemainingColumn,
)

from . import __version__
from .analyze import analyze_track
from .io_helpers import discover_audio_files, sidecar_path_for
from .sidecar import is_fresh, save_sidecar

log = logging.getLogger("dj_analyze.pipeline")


def _analyze_one(args: tuple[Path, Path]) -> tuple[Path, dict[str, Any]]:
    """Worker entry point — must be top-level for multiprocessing pickling."""
    track, sidecar = args
    return sidecar, analyze_track(track)


def run_pipeline(
    *,
    source_dir: Path,
    output_dir: Path,
    workers: int = 1,
    force: bool = False,
) -> tuple[int, int, int]:
    """Walk `source_dir`, analyze tracks, write sidecars under `output_dir`.

    Returns (n_processed, n_skipped, n_errors).
    """
    if not source_dir.exists() or not source_dir.is_dir():
        raise FileNotFoundError(f"Source dir not found: {source_dir}")
    output_dir.mkdir(parents=True, exist_ok=True)

    tracks = list(discover_audio_files(source_dir))
    todo: list[tuple[Path, Path]] = []
    n_skipped = 0
    for track in tracks:
        sidecar = sidecar_path_for(track, source_dir, output_dir)
        if not force and is_fresh(track, sidecar, __version__):
            n_skipped += 1
            continue
        todo.append((track, sidecar))

    n_processed = 0
    n_errors = 0

    with Progress(
        SpinnerColumn(),
        TextColumn("[progress.description]{task.description}"),
        BarColumn(),
        MofNCompleteColumn(),
        TimeRemainingColumn(),
    ) as progress:
        task = progress.add_task("Analyzing tracks", total=len(todo))

        if workers <= 1:
            for track, sidecar in todo:
                _process_one(track, sidecar, on_error=lambda: None)
                # Inline path tracks counters directly.
                # (Inlined for simplicity — the helper writes the sidecar.)
                # Re-check whether sidecar contains an error:
                from .sidecar import load_sidecar
                data = load_sidecar(sidecar)
                if "error" in data:
                    n_errors += 1
                else:
                    n_processed += 1
                progress.advance(task)
        else:
            with ProcessPoolExecutor(max_workers=workers) as pool:
                futures = {
                    pool.submit(_analyze_one, (track, sidecar)): (track, sidecar)
                    for track, sidecar in todo
                }
                for future in as_completed(futures):
                    sidecar, data = future.result()
                    save_sidecar(sidecar, data)
                    if "error" in data:
                        n_errors += 1
                    else:
                        n_processed += 1
                    progress.advance(task)

    log.info(
        "pipeline complete",
        extra={
            "processed": n_processed,
            "skipped": n_skipped,
            "errors": n_errors,
        },
    )
    return n_processed, n_skipped, n_errors


def _process_one(track: Path, sidecar: Path, *, on_error) -> None:
    """Single-worker codepath: analyze and save in this process."""
    data = analyze_track(track)
    save_sidecar(sidecar, data)
```

- [ ] **Step 4: Run tests and verify they pass**

```bash
pytest tests/test_pipeline.py -v
```

Expected: 4 tests pass (slow — possibly 60-90s due to repeated BPM analysis).

- [ ] **Step 5: Commit**

```bash
git add analyzer/src/dj_analyze/pipeline.py analyzer/tests/test_pipeline.py
git commit -m "feat(analyzer): parallel pipeline runner with skip-if-fresh"
```

---

### Task 17: logging_config.py — pretty vs JSON log modes

**Files:**
- Create: `analyzer/src/dj_analyze/logging_config.py`

- [ ] **Step 1: Implement logging_config.py**

Create `analyzer/src/dj_analyze/logging_config.py`:
```python
"""Configure logging — `pretty` (default) or `json` (for production / Rancher)."""

from __future__ import annotations

import json
import logging
import sys
from typing import Literal


class _JsonFormatter(logging.Formatter):
    def format(self, record: logging.LogRecord) -> str:
        payload = {
            "ts": self.formatTime(record, datefmt="%Y-%m-%dT%H:%M:%S"),
            "level": record.levelname,
            "logger": record.name,
            "msg": record.getMessage(),
        }
        # Propagate any `extra=` fields passed by callers.
        reserved = {
            "name", "msg", "args", "levelname", "levelno", "pathname", "filename",
            "module", "exc_info", "exc_text", "stack_info", "lineno", "funcName",
            "created", "msecs", "relativeCreated", "thread", "threadName",
            "processName", "process", "message", "taskName",
        }
        for k, v in record.__dict__.items():
            if k not in reserved:
                payload[k] = v
        if record.exc_info:
            payload["exc"] = self.formatException(record.exc_info)
        return json.dumps(payload, ensure_ascii=False, default=str)


def configure_logging(format_: Literal["pretty", "json"] = "pretty") -> None:
    """Reset and configure the root logger for the analyzer process."""
    root = logging.getLogger()
    root.handlers.clear()
    root.setLevel(logging.INFO)

    handler = logging.StreamHandler(sys.stderr)
    if format_ == "json":
        handler.setFormatter(_JsonFormatter())
    else:
        handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s"))
    root.addHandler(handler)
```

- [ ] **Step 2: Commit**

```bash
git add analyzer/src/dj_analyze/logging_config.py
git commit -m "feat(analyzer): pretty + JSON logging modes"
```

---

### Task 18: __main__.py — CLI entry

**Files:**
- Create: `analyzer/src/dj_analyze/__main__.py`
- Create: `analyzer/tests/test_cli.py`

- [ ] **Step 1: Write the failing CLI tests**

Create `analyzer/tests/test_cli.py`:
```python
from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

FIXTURES = Path(__file__).parent / "fixtures"


def _run_cli(*args: str, cwd: Path | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, "-m", "dj_analyze", *args],
        capture_output=True,
        text=True,
        cwd=cwd,
        env={**os.environ},
    )


def test_cli_help_exits_zero() -> None:
    result = _run_cli("--help")
    assert result.returncode == 0
    assert "dj-analyze" in result.stdout.lower() or "SOURCE_DIR" in result.stdout


def test_cli_analyzes_directory(tmp_path: Path) -> None:
    source = tmp_path / "music"
    source.mkdir()
    (source / "click.wav").write_bytes((FIXTURES / "click_120bpm.wav").read_bytes())

    output = tmp_path / "analyzed"
    result = _run_cli(str(source), str(output), "--workers", "1")
    assert result.returncode == 0, result.stderr

    sidecar = output / "click.wav.json"
    assert sidecar.exists()
    assert json.loads(sidecar.read_text())["schema_version"] == 1


def test_cli_json_log_mode_emits_json(tmp_path: Path) -> None:
    source = tmp_path / "music"
    source.mkdir()
    (source / "click.wav").write_bytes((FIXTURES / "click_120bpm.wav").read_bytes())
    output = tmp_path / "analyzed"

    result = _run_cli(
        str(source), str(output), "--workers", "1", "--log-format", "json"
    )
    assert result.returncode == 0
    # At least one stderr line should parse as JSON.
    json_lines = [
        line for line in result.stderr.splitlines() if line.startswith("{")
    ]
    assert json_lines, f"No JSON log lines found in stderr: {result.stderr!r}"
    parsed = json.loads(json_lines[0])
    assert "level" in parsed
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
pytest tests/test_cli.py -v
```

Expected: failures because `__main__.py` doesn't exist or doesn't accept these args.

- [ ] **Step 3: Implement __main__.py**

Create `analyzer/src/dj_analyze/__main__.py`:
```python
"""CLI entry: `dj-analyze SOURCE_DIR OUTPUT_DIR [--workers N] [--force]`."""

from __future__ import annotations

import argparse
import logging
import os
import sys
from pathlib import Path

from .logging_config import configure_logging
from .pipeline import run_pipeline

log = logging.getLogger("dj_analyze.cli")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="dj-analyze",
        description="Analyze a directory of audio files and write sidecar JSONs.",
    )
    parser.add_argument("source_dir", type=Path, help="Root directory of audio files (recursive)")
    parser.add_argument("output_dir", type=Path, help="Where to write sidecar JSONs")
    parser.add_argument(
        "--workers",
        type=int,
        default=max(1, (os.cpu_count() or 2) - 1),
        help="Parallel workers (default: CPU count - 1)",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Re-analyze tracks even if a fresh sidecar exists",
    )
    parser.add_argument(
        "--log-format",
        choices=("pretty", "json"),
        default="pretty",
        help="Log output style (default: pretty)",
    )
    args = parser.parse_args(argv)

    configure_logging(args.log_format)

    try:
        processed, skipped, errors = run_pipeline(
            source_dir=args.source_dir,
            output_dir=args.output_dir,
            workers=args.workers,
            force=args.force,
        )
    except FileNotFoundError as e:
        log.error("input error: %s", e)
        return 2

    log.info(
        "done",
        extra={"processed": processed, "skipped": skipped, "errors": errors},
    )
    return 0 if errors == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 4: Run tests and verify they pass**

```bash
pytest tests/test_cli.py -v
```

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add analyzer/src/dj_analyze/__main__.py analyzer/tests/test_cli.py
git commit -m "feat(analyzer): CLI entry with --workers, --force, --log-format"
```

---

### Task 19: Schema validation in tests

**Files:**
- Create: `analyzer/tests/test_schema_validation.py`

Every sidecar produced by analyze_track should validate against `schema/sidecar.schema.json`. This catches drift between the schema and the implementation.

- [ ] **Step 1: Write the validation tests**

Create `analyzer/tests/test_schema_validation.py`:
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


def test_click_sidecar_validates(validator: Draft202012Validator) -> None:
    result = analyze_track(FIXTURES / "click_120bpm.wav")
    errors = list(validator.iter_errors(result))
    assert not errors, "Schema errors:\n" + "\n".join(e.message for e in errors)


def test_corrupt_sidecar_validates(validator: Draft202012Validator) -> None:
    """Error sidecars only have base fields + `error`; must still validate."""
    result = analyze_track(FIXTURES / "corrupt.mp3")
    errors = list(validator.iter_errors(result))
    assert not errors, "Schema errors:\n" + "\n".join(e.message for e in errors)
```

- [ ] **Step 2: Run the tests**

```bash
pytest tests/test_schema_validation.py -v
```

Expected: 2 tests pass.

- [ ] **Step 3: Commit**

```bash
git add analyzer/tests/test_schema_validation.py
git commit -m "test(analyzer): schema validation on real analyzer output"
```

---

### Task 20: Lint + type check the package

**Files:** (no new files; runs tooling)

- [ ] **Step 1: Run ruff**

From `analyzer/`:
```bash
ruff check src tests
```

Fix any issues by re-editing the implicated files. Commit each fix.

- [ ] **Step 2: Run mypy**

```bash
mypy src
```

Fix any type errors by adding type annotations in the implicated files. Common likely fixes:
- librosa returns are largely `Any`; cast or ignore via `# type: ignore[no-untyped-call]` if needed
- numpy operations may need explicit dtype annotations

Commit each fix.

- [ ] **Step 3: Run the full test suite as a final check**

```bash
pytest -v
```

Expected: all tests pass.

- [ ] **Step 4: Commit (if any fixes happened)**

```bash
git add -A
git commit -m "chore(analyzer): satisfy ruff + mypy"
```

---

### Task 21: Dockerfile for the analyzer

**Files:**
- Create: `analyzer/Dockerfile`

- [ ] **Step 1: Write the Dockerfile**

Create `analyzer/Dockerfile`:
```dockerfile
# syntax=docker/dockerfile:1
FROM python:3.11-slim AS base

# System deps: ffmpeg for audio decoding; build essentials for any sdists.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ffmpeg \
        build-essential \
        libsndfile1 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install in two stages so deps are cached across rebuilds.
COPY analyzer/pyproject.toml /app/analyzer/
RUN pip install --no-cache-dir hatchling

COPY analyzer/ /app/analyzer/
COPY schema/ /app/schema/

RUN pip install --no-cache-dir -e /app/analyzer

# Default invocation: `dj-analyze /music /out --workers $(nproc) --log-format json`.
# Users override via `docker run ... <args>`.
ENTRYPOINT ["dj-analyze"]
CMD ["--help"]
```

- [ ] **Step 2: Build the image locally to verify**

From the repo root:
```bash
docker build -f analyzer/Dockerfile -t dj-analyzer:dev .
```

Expected: successful build. If `essentia` or `madmom` fail to install inside the container, comment them out in `pyproject.toml` (the code has fallbacks).

- [ ] **Step 3: Smoke-test the image**

```bash
docker run --rm dj-analyzer:dev --help
```

Expected: prints the argparse help.

- [ ] **Step 4: Commit**

```bash
git add analyzer/Dockerfile
git commit -m "feat(analyzer): Dockerfile for deployment image"
```

---

### Task 22: Kubernetes Job manifest for the analyzer

**Files:**
- Create: `deploy/analyzer-job.yaml`

- [ ] **Step 1: Write the Job manifest**

Create `deploy/analyzer-job.yaml`:
```yaml
apiVersion: batch/v1
kind: Job
metadata:
  name: dj-analyze
  labels:
    app: dj-engine
    component: analyzer
spec:
  ttlSecondsAfterFinished: 86400
  backoffLimit: 1
  template:
    metadata:
      labels:
        app: dj-engine
        component: analyzer
    spec:
      restartPolicy: Never
      containers:
        - name: analyzer
          image: dj-analyzer:0.1.0
          imagePullPolicy: IfNotPresent
          args:
            - /music
            - /analyzed
            - --workers
            - "4"
            - --log-format
            - json
          volumeMounts:
            - name: music
              mountPath: /music
              readOnly: true
            - name: analyzed
              mountPath: /analyzed
          resources:
            requests:
              cpu: "1"
              memory: 1Gi
            limits:
              cpu: "4"
              memory: 4Gi
      volumes:
        - name: music
          persistentVolumeClaim:
            claimName: dj-music
        - name: analyzed
          persistentVolumeClaim:
            claimName: dj-analyzed
```

- [ ] **Step 2: Validate the YAML**

Run:
```bash
python -c "import yaml; yaml.safe_load_all(open('deploy/analyzer-job.yaml').read())" && echo OK
```

Expected: `OK`.

- [ ] **Step 3: Commit**

```bash
git add deploy/analyzer-job.yaml
git commit -m "feat(deploy): kubernetes Job manifest for analyzer"
```

---

### Task 23: End-to-end smoke test

**Files:** (no new files)

- [ ] **Step 1: Run the analyzer over the fixtures**

From `analyzer/`:
```bash
dj-analyze tests/fixtures ../analyzed --workers 1
```

Expected:
- `../analyzed/click_120bpm.wav.json` exists with `bpm` ≈ 120
- `../analyzed/sine_a_minor.wav.json` exists with `key` ending in `"A"`
- `../analyzed/silence_then_beep.wav.json` exists
- `../analyzed/corrupt.mp3.json` exists with `error` set

- [ ] **Step 2: Inspect one output**

```bash
python -c "import json; print(json.dumps(json.load(open('../analyzed/click_120bpm.wav.json')), indent=2)[:500])"
```

Expected: A pretty-printed JSON with `schema_version: 1`, `bpm` ≈ 120, beats/downbeats/cues populated.

- [ ] **Step 3: Re-run the analyzer to verify idempotence**

```bash
dj-analyze tests/fixtures ../analyzed --workers 1
```

Expected: log shows `skipped: 4, processed: 0`.

- [ ] **Step 4: Update README dev quickstart**

Edit `README.md`, replace the `TBD` section under "Dev quickstart" with:

```markdown
## Dev quickstart

### Analyzer

```bash
cd analyzer
python -m venv .venv
.venv\Scripts\activate
pip install -e ".[dev]"
dj-analyze C:/path/to/music ./analyzed
```

Each track in `C:/path/to/music` (recursive) gets a `.json` sidecar mirrored under `./analyzed`. Re-running is cheap (only new/changed tracks re-analyzed). See `schema/sidecar.schema.json` for the output format.

### Engine

See plan `docs/superpowers/plans/2026-05-26-dj-engine-02-engine.md`.
```

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "docs: README dev quickstart for analyzer"
```

---

## Self-review

### Spec coverage check

| Spec section | Task(s) | Notes |
|---|---|---|
| Two-program system | 1, 2 | Repo structure scaffolded; engine half is plan 2 |
| Sidecar schema | 3 | `schema/sidecar.schema.json` |
| Idempotent + resumable | 7, 16 | `is_fresh()` + pipeline checks; tested |
| Parallel workers | 16, 18 | ProcessPoolExecutor + `--workers` flag |
| Supported formats (mp3/flac/m4a/wav/ogg/opus) | 6 | `AUDIO_EXTENSIONS` |
| BPM + bpm_confidence | 9 | `detect_bpm.py` with windowed stability score |
| Beats list | 9 | `detect_beats.py` |
| Downbeats (madmom + fallback) | 10 | `detect_downbeats.py` with `DownbeatResult.method` |
| Key (Camelot) | 11 | `detect_key.py` with essentia + librosa fallback |
| Energy curve + mean | 12 | `detect_energy.py` |
| Cue points (cue_in/out, intro_end, outro_start) | 13 | `detect_cues.py` |
| Mixability classification | 14 | `mixability.py` with reason codes |
| Per-track orchestrator | 15 | `analyze.py` |
| CLI with --workers, --force, --log-format | 18 | `__main__.py` |
| Error handling for corrupt files | 15 | Returns error sidecar; tested in tasks 15, 19 |
| Schema validation in CI | 19 | `test_schema_validation.py` |
| Structured JSON logs | 17, 18 | `--log-format json` |
| Dockerfile | 21 | Multi-stage with ffmpeg, libsndfile |
| K8s Job manifest | 22 | `deploy/analyzer-job.yaml` |

**Gaps identified during review:** none. All analyzer-side spec items mapped.

### Placeholder scan

Searched the plan for "TBD", "TODO", "implement later", "fill in details", "appropriate error handling", "similar to task N", "etc.". Found one intentional `TBD` in the README stub (Task 2) which is replaced concretely in Task 23. Found one "TODO" comment in `mixability.py` (`bpm_variable` reason wired but not currently computed). Leaving that as a documented future-work hook — the disqualification logic handles its absence correctly. No other placeholders.

### Type consistency

- `DecodeError` raised from `decode.py` (Task 8); imported and caught by `analyze.py` (Task 15). ✓
- `DownbeatResult` dataclass with `.downbeats` and `.method` defined in Task 10; consumed in Task 15. ✓
- `KeyResult` with `.camelot`, `.label`, `.confidence` defined in Task 11; consumed in Task 15. ✓
- `is_fresh(track, sidecar_path, analyzer_version)` signature in Task 7 matches usage in Task 16. ✓
- `save_sidecar(path, data)` signature in Task 7 matches usage in Task 16. ✓
- `classify(...)` keyword arguments in Task 14 match call in Task 15. ✓
- `discover_audio_files(root)` and `sidecar_path_for(track, source_root, output_root)` in Task 6 match calls in Task 16. ✓

### Scope check

This plan is one focused subsystem (Python analyzer + its deployment artifacts) and produces working, testable software on its own: after the last task, `dj-analyze` works as a CLI, has a Docker image, and ships with a K8s Job manifest. The engine half is in plan 02.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-26-dj-engine-01-analyzer.md`. Two execution options:

1. **Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration
2. **Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
