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

    # 16-bar phrase boundaries: the engine snaps transitions to these so they land
    # on structural moments (verse/chorus/drop) rather than every bar.
    phrase_result = (
        detect_phrases(audio, sr, beats=beats, output_stride_bars=16) if beats else None
    )

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
