"""Stem separation via Demucs + per-bar density analytics.

Outputs:
  - 4 stem MP3s on disk under <stems_root>/<track_basename>/{drums,bass,vocals,other}.mp3
    (falls back to WAV if the libmp3lame backend isn't available).
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
            energy_per_stem={
                n: float(np.sqrt(np.mean(sources_np[i].mean(axis=0) ** 2)))
                for i, n in enumerate(stem_names)
            },
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
        bar_end = (
            bar_edges_seconds[i + 1] if i + 1 < len(bar_edges_seconds) else bar_edges_seconds[-1]
        )
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
