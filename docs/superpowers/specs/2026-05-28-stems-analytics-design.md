# Stems Analytics (Phases A+B+C)

**Date:** 2026-05-28
**Status:** Approved for planning
**Branch:** `research/two-deck-mixer`
**Sub-project:** analyzer + sidecar + engine plumbing (Phase D — stem-aware recipes — is a follow-up)

## Summary

Add per-track stem separation (drums, bass, vocals, other) via Demucs to the analyzer, derive per-bar density metrics + vocal-presence segments from the stems, store them in the sidecar JSON, and have the engine load them into its `Sidecar` struct (visible in logs at transition time, no behavior change yet). The separated stem audio is preserved on disk under `analyzed-real-stems/<track>/{drums,bass,vocals,other}.mp3` for future use by stem-aware recipes (Phase D). The analyzer Python environment is downgraded from 3.13 → 3.10 so Demucs installs cleanly into the existing `analyzer/.venv` — no side-venv subprocess complexity.

## Motivation

The user's listening feedback: current transitions feel "like a radio, songs just passing" rather than DJ-like. Root cause analysis showed every recipe today is a *variation on crossfade with EQ choreography* — no genuine event recipes (drops, breakdowns, vocal-over-instrumental layering). The blocker to event recipes is **per-bar structural awareness** the engine doesn't have: when do vocals enter/leave, where are the drum-density valleys (breakdowns), where do bass kicks return after a build. Aggregate `energy_mean` and `band_energy_means.bass` aren't enough — they're whole-track averages, not per-bar timelines.

Stems give us per-bar timelines. Once the analyzer extracts them, even simple decisions get smarter — e.g., the planner can prefer outro positions where vocals are silent, avoiding the radio-DJ trope of cutting off a singer mid-phrase. The Phase D recipes that the user mentioned (kill outgoing's bass, let incoming play over it, sync bass on the swap) become natural once we have per-bar drum/bass timelines to anchor them to.

## Goals

- Downgrade `analyzer/.venv` from Python 3.13 → 3.10. Reinstall the existing analyzer deps + `demucs`.
- Add `analyzer/src/dj_analyze/detect_stems.py` that runs Demucs on a decoded audio buffer, writes the 4 separated stems to `analyzed-real-stems/<track_basename>/{drums,bass,vocals,other}.mp3`, and returns derived analytics.
- Derived analytics, computed per-bar (using the existing beat detection's downbeat positions to define bar boundaries):
  - `drum_density[]` — RMS-of-onsets per bar, normalized to [0, 1] across the track.
  - `bass_density[]` — same for the bass stem.
  - `vocal_density[]` — same for the vocals stem (used to detect breakdowns).
  - `vocal_segments` — list of `(start_seconds, end_seconds)` where vocal energy is above a threshold for at least one continuous bar. Detected via simple energy-thresholding on the vocals stem.
  - `energy_per_stem` — single mean energy per stem (drums, bass, vocals, other) — useful for selector classification later.
- Add a `stems` block to the sidecar schema:
  ```json
  "stems": {
    "drum_density": [0.12, 0.45, 0.88, ...],
    "bass_density": [...],
    "vocal_density": [...],
    "vocal_segments": [[12.5, 45.0], [78.0, 110.2], ...],
    "energy_per_stem": {"drums": 0.71, "bass": 0.64, "vocals": 0.42, "other": 0.55},
    "demucs_version": "4.0.1",
    "model": "htdemucs"
  }
  ```
  Optional field — when absent the engine continues to work using the existing aggregate metrics.
- Engine loads the new field into its `Sidecar` struct in Rust (`engine/src/sidecar.rs`). At transition time, the existing `selector picked` log line gains `outgoing_vocal_segments=2 outgoing_drum_density_mean=0.62 outgoing_bass_density_mean=0.71` (compact summaries; the full per-bar arrays are too verbose to log every transition).
- **No engine behavioral changes** — the field is loaded and logged but not yet used in any selector / planner / recipe decision. That's Phase D.

## Non-Goals (this sub-project)

- **No stem-aware recipes.** No `VocalOverlay`, no `BreakdownIntoDrop`, no per-stem fader / EQ / filter manipulation in transitions. Phase D.
- **No deck-level stem decoding.** The engine deck still decodes the mixed source MP3. Loading 4 stems into a single deck so a recipe can mix them independently is Phase D infrastructure.
- **No selector or planner changes.** Decisions still use `energy_mean`, `band_energy_means`, etc. The new fields are exposed in logs but not in scoring.
- **No automatic re-run on existing sidecars.** Re-analyzing the 18-track library is a one-time manual step the user runs after the engineering lands. The analyzer's incremental cache (`analyzer_version` bump) handles the cache invalidation.
- **No Python 3.13 fallback.** We commit to Python 3.10 for the analyzer. The engine (Rust) is unaffected.
- **No GPU detection / acceleration.** Demucs runs on CPU. If the user later wants GPU acceleration (~10× faster), that's a `pip install torch --index-url https://download.pytorch.org/whl/cu121` + `device='cuda'` change — out of scope.

## Architecture

```
analyzer/
  .venv/                      ← recreated with Python 3.10 (was 3.13)
  src/dj_analyze/
    detect_stems.py           ← NEW: runs demucs, writes stems, computes analytics
    pipeline.py               ← MODIFIED: call detect_stems, attach result to sidecar
    sidecar.py                ← MODIFIED: dataclass field for stems block
    __init__.py               ← MODIFIED: __version__ bump (cache invalidation)
  tests/
    test_detect_stems.py      ← NEW: synthesize a 10s mixed signal, separate, check shapes

analyzed-real-stems/          ← NEW directory, gitignored (extend .gitignore)
  001-song/
    drums.mp3
    bass.mp3
    vocals.mp3
    other.mp3
  002-song/
    ...

schema/
  sidecar.schema.json         ← MODIFIED: add "stems" object schema

engine/
  src/sidecar.rs              ← MODIFIED: add Option<Stems> field with serde
  src/main.rs                 ← MODIFIED: include vocal_segments count + density means in "selector picked" log
```

## Components

### 1. Python 3.10 environment

Install Python 3.10 via winget if not present:

```powershell
winget install --id Python.Python.3.10 --accept-source-agreements --accept-package-agreements
```

Recreate `analyzer/.venv`:

```powershell
Remove-Item -Recurse -Force analyzer\.venv
py -3.10 -m venv analyzer\.venv
analyzer\.venv\Scripts\python.exe -m pip install --upgrade pip
analyzer\.venv\Scripts\python.exe -m pip install -e analyzer[dev]
analyzer\.venv\Scripts\python.exe -m pip install demucs
```

Demucs pulls in PyTorch (~2 GB CPU-only wheels). One-time install, then it's cached.

### 2. `detect_stems.py`

```python
"""Stem separation via Demucs + per-bar density analytics.

Outputs:
  - 4 stem MP3s on disk under <stems_root>/<track_basename>/{drums,bass,vocals,other}.mp3
  - StemAnalytics dataclass with per-bar drum/bass/vocal densities,
    vocal_segments, energy_per_stem.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

import numpy as np
import torch
from demucs.apply import apply_model
from demucs.pretrained import get_model

import librosa


@dataclass
class StemAnalytics:
    drum_density: list[float]
    bass_density: list[float]
    vocal_density: list[float]
    vocal_segments: list[tuple[float, float]]
    energy_per_stem: dict[str, float]
    demucs_version: str
    model: str


_MODEL_NAME = "htdemucs"
_MODEL_CACHE: dict[str, torch.nn.Module] = {}


def _get_model() -> torch.nn.Module:
    if _MODEL_NAME not in _MODEL_CACHE:
        _MODEL_CACHE[_MODEL_NAME] = get_model(_MODEL_NAME)
    return _MODEL_CACHE[_MODEL_NAME]


def detect_stems(
    audio: np.ndarray,
    sr: int,
    downbeats_seconds: list[float],
    track_basename: str,
    stems_root: Path,
) -> StemAnalytics:
    """Separate stems with Demucs, write to disk, derive analytics.

    Demucs expects audio shaped (channels, samples) at 44100 Hz. The analyzer
    typically passes mono audio at 22050 Hz; we convert to stereo @ 44100 here.
    """
    model = _get_model()

    # Demucs wants stereo @ 44100.
    if sr != 44100:
        audio_44k = librosa.resample(audio, orig_sr=sr, target_sr=44100)
    else:
        audio_44k = audio
    if audio_44k.ndim == 1:
        audio_44k = np.stack([audio_44k, audio_44k])

    waveform = torch.from_numpy(audio_44k).float().unsqueeze(0)
    with torch.no_grad():
        sources = apply_model(model, waveform, progress=False)
    # sources shape: (1, 4, channels, samples) — sources order from model.sources.
    sources_np = sources.squeeze(0).cpu().numpy()  # (4, channels, samples)
    stem_names = model.sources  # e.g. ["drums", "bass", "other", "vocals"]

    # Write stems to disk.
    track_dir = stems_root / track_basename
    track_dir.mkdir(parents=True, exist_ok=True)
    import soundfile as sf
    for name, stem in zip(stem_names, sources_np):
        # Mono-mix for storage; recipes that need stereo can refer to the original.
        stem_mono = stem.mean(axis=0)
        sf.write(track_dir / f"{name}.mp3", stem_mono, 44100, format="MP3", bitrate_mode="VARIABLE")

    # Per-bar density.
    bar_edges = list(downbeats_seconds)
    if len(bar_edges) < 2:
        # No downbeats detected — return empty analytics with a warning logged upstream.
        return StemAnalytics(
            drum_density=[],
            bass_density=[],
            vocal_density=[],
            vocal_segments=[],
            energy_per_stem={name: 0.0 for name in stem_names},
            demucs_version=_demucs_version(),
            model=_MODEL_NAME,
        )

    drum_density = _per_bar_density(sources_np, stem_names, "drums", bar_edges, sr=44100)
    bass_density = _per_bar_density(sources_np, stem_names, "bass", bar_edges, sr=44100)
    vocal_density = _per_bar_density(sources_np, stem_names, "vocals", bar_edges, sr=44100)

    vocal_segments = _vocal_segments_from_density(vocal_density, bar_edges)

    energy_per_stem = {
        name: float(np.sqrt(np.mean(stem.mean(axis=0) ** 2)))
        for name, stem in zip(stem_names, sources_np)
    }

    return StemAnalytics(
        drum_density=drum_density,
        bass_density=bass_density,
        vocal_density=vocal_density,
        vocal_segments=vocal_segments,
        energy_per_stem=energy_per_stem,
        demucs_version=_demucs_version(),
        model=_MODEL_NAME,
    )


def _per_bar_density(
    sources: np.ndarray,
    stem_names: list[str],
    target_stem: str,
    bar_edges_seconds: list[float],
    sr: int,
) -> list[float]:
    """RMS per bar of `target_stem`, normalized to max RMS across the track."""
    if target_stem not in stem_names:
        return []
    idx = stem_names.index(target_stem)
    stem = sources[idx].mean(axis=0)  # mono
    rms_per_bar = []
    for start_s, end_s in zip(bar_edges_seconds[:-1], bar_edges_seconds[1:]):
        i0 = int(start_s * sr)
        i1 = int(end_s * sr)
        if i1 <= i0 or i1 > len(stem):
            rms_per_bar.append(0.0)
            continue
        chunk = stem[i0:i1]
        rms_per_bar.append(float(np.sqrt(np.mean(chunk ** 2))))
    if not rms_per_bar:
        return []
    max_rms = max(rms_per_bar)
    if max_rms <= 0:
        return [0.0] * len(rms_per_bar)
    return [r / max_rms for r in rms_per_bar]


_VOCAL_PRESENT_THRESHOLD = 0.15  # normalized density above which we say vocals are "present"


def _vocal_segments_from_density(
    vocal_density: list[float],
    bar_edges_seconds: list[float],
) -> list[tuple[float, float]]:
    """Merge contiguous bars where vocal_density >= threshold into segments."""
    if not vocal_density or len(bar_edges_seconds) < 2:
        return []
    segments: list[tuple[float, float]] = []
    in_segment = False
    seg_start = 0.0
    for i, d in enumerate(vocal_density):
        bar_start = bar_edges_seconds[i]
        bar_end = bar_edges_seconds[i + 1] if i + 1 < len(bar_edges_seconds) else bar_edges_seconds[-1]
        if d >= _VOCAL_PRESENT_THRESHOLD:
            if not in_segment:
                seg_start = bar_start
                in_segment = True
            # Always extend to current bar_end.
            seg_end = bar_end
        else:
            if in_segment:
                segments.append((seg_start, seg_end))
                in_segment = False
    if in_segment:
        segments.append((seg_start, bar_edges_seconds[-1]))
    return segments


def _demucs_version() -> str:
    import demucs
    return getattr(demucs, "__version__", "unknown")
```

### 3. Sidecar schema + dataclass

Extend `schema/sidecar.schema.json` with a `stems` object (optional):

```json
"stems": {
  "type": "object",
  "properties": {
    "drum_density":   { "type": "array", "items": { "type": "number" } },
    "bass_density":   { "type": "array", "items": { "type": "number" } },
    "vocal_density":  { "type": "array", "items": { "type": "number" } },
    "vocal_segments": {
      "type": "array",
      "items": {
        "type": "array",
        "items": { "type": "number" },
        "minItems": 2,
        "maxItems": 2
      }
    },
    "energy_per_stem": {
      "type": "object",
      "additionalProperties": { "type": "number" }
    },
    "demucs_version": { "type": "string" },
    "model": { "type": "string" }
  }
}
```

`analyzer/src/dj_analyze/sidecar.py` gains a corresponding dataclass field. `engine/src/sidecar.rs` gains an `Option<Stems>` field with `#[serde(default)]` so existing sidecars without the block deserialize without error.

### 4. Engine-side: read + log

`engine/src/sidecar.rs`:

```rust
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
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

pub struct Sidecar {
    // ... existing fields ...
    #[serde(default)]
    pub stems: Option<Stems>,
}
```

In `engine/src/main.rs`, the existing `selector picked` log line gets a few summary fields (no full arrays):

```rust
let outgoing_vocal_segs = current.sidecar.stems.as_ref()
    .map(|s| s.vocal_segments.len()).unwrap_or(0);
let outgoing_drum_density_mean = current.sidecar.stems.as_ref()
    .map(|s| mean_or_zero(&s.drum_density)).unwrap_or(0.0);
let incoming_vocal_segs = next_track.sidecar.stems.as_ref()
    .map(|s| s.vocal_segments.len()).unwrap_or(0);
let incoming_drum_density_mean = next_track.sidecar.stems.as_ref()
    .map(|s| mean_or_zero(&s.drum_density)).unwrap_or(0.0);

info!(
    // ... existing fields ...
    outgoing_vocal_segments = outgoing_vocal_segs,
    outgoing_drum_density_mean,
    incoming_vocal_segments = incoming_vocal_segs,
    incoming_drum_density_mean,
    "selector picked"
);
```

Helper `fn mean_or_zero(v: &[f64]) -> f64` defined in main.rs.

## Testing

### Analyzer tests

`analyzer/tests/test_detect_stems.py` (new):

1. **Smoke: separate a 10-second synthesized mixed signal.** Generate `np.random.randn(2, 44100 * 10) * 0.1` (random stereo noise), call `detect_stems` with fake `downbeats_seconds = [0, 2, 4, 6, 8, 10]`. Assert:
   - `StemAnalytics.drum_density` has length 5 (5 bars in 10 seconds at 2s/bar).
   - All densities are in `[0, 1]`.
   - 4 MP3 files are written to the stems dir.
   - `energy_per_stem` has all 4 stem names as keys.
   - `demucs_version` matches the installed version.
2. **`_per_bar_density` math.** Bypass demucs: feed a hand-constructed `sources` array (4 stems × 2 channels × N samples) where the target stem has known RMS per bar (e.g., bar 0 silence, bar 1 unit-amplitude sine, bar 2 silence). Assert `[0, 1, 0]` (normalized).
3. **`_vocal_segments_from_density` boundary detection.** Hand-constructed density list `[0.0, 0.5, 0.5, 0.0, 0.0, 0.4, 0.0]` with bar_edges `[0, 2, 4, 6, 8, 10, 12, 14]` → expects segments `[(2.0, 6.0), (10.0, 12.0)]`.
4. **No downbeats → empty analytics.** Call with `downbeats_seconds = []`. Returns empty arrays.
5. **Idempotency.** Call `detect_stems` twice on the same audio — second call writes the same MP3s (or skips if disk-cached; either is acceptable as long as analytics match).

Skip #1 in CI if Demucs is large to load (~150 MB model); mark it `@pytest.mark.slow` and run locally.

### Engine tests

`engine/tests/sidecar_test.rs` (extend):

- Deserialize a sidecar JSON with the `stems` block — assert the field is `Some(...)` with the right shape.
- Deserialize a sidecar JSON *without* the `stems` block — assert `stems == None` (serde default).

### Integration

After Phase A+B+C lands, the user manually re-runs the analyzer on the 18-track library. The runtime per track on CPU: ~1× realtime for Demucs separation + a few seconds for analytics → ~5 minutes per track + ~30 seconds analytics = ~95 minutes total for 18 tracks. Plan: one-time overnight or background run.

## Risks

- **Demucs install failure on Python 3.10.** Unlikely (it's officially supported), but PyTorch wheels can be finicky on Windows. Mitigation: the install command shows the version; if it fails, the subagent reports BLOCKED with the actual pip error. Manual fix may need `pip install torch --index-url https://download.pytorch.org/whl/cpu`.
- **First demucs call downloads ~150 MB model.** Happens on first separation call; cached after. First track is slow.
- **Stem files take disk space.** ~360 MB for 18 tracks at VBR MP3. Acceptable for a 24-track personal library; large libraries would need a re-think.
- **Demucs separation quality varies by genre.** Vocals stem on instrumental-only tracks may have noise; vocal_segments may falsely include the noise. Threshold of 0.15 is conservative — tune after listening.
- **Resampling overhead.** The analyzer's existing pipeline runs at 22050 Hz (memory-cheap); demucs needs 44100. The resample happens in `detect_stems.py` and adds ~10 % CPU per track. Acceptable.
- **`sf.write` MP3 encoding** may not be available on all platforms — `soundfile` typically delegates MP3 to libmp3lame; if the system lacks lame, the call fails. Mitigation: if MP3 fails, fall back to WAV (lossless, ~10× more disk).

## Open questions

- Should `_VOCAL_PRESENT_THRESHOLD` be relative-to-track-loudness instead of absolute? After listening to vocal_segments outputs on real tracks, we may need to tune.
- Should we store stems at 44100 (full-fidelity) or downsample to 22050 to save disk? Phase D recipes might want full quality; 44100 it is.
- Should the analyzer skip stem separation when it detects the track is too short, too quiet, or otherwise unsuitable? Out of scope for now — fail loudly if anything breaks.

## Roadmap (Phase D — stem-aware recipes, follow-up sub-project)

With stems analytics in place, future event recipes the user mentioned can be built:

1. **`VocalOverlay`**: Outgoing plays its instrumental stems (drums + bass + other) at full volume; incoming's vocals stem layers on top via a separate deck-side path. Outgoing's vocals stem is muted. Held for 8-16 bars before a clean hard-cut.
2. **`BreakdownIntoDrop`**: Locate outgoing's drum-density valley (a breakdown). Schedule the transition to fire at the next drum-density peak (the drop). Hard cut on the drop's downbeat.
3. **`BassSwap-Stem-Aware`**: The existing BassSwap, but the kill/release windows snap to the OUTGOING's bass_density valleys and INCOMING's bass_density peaks, not arbitrary fractions.
4. **`DoubleDrop`**: Both decks silent for 1 bar (fader to 0), then both come in at full volume on the same downbeat. Selector pairs tracks whose drops align harmonically.

All four need deck-level stem decoding (loading multiple audio sources per deck) — a non-trivial engine change. Defer to Phase D spec.
