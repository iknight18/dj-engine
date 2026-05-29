"""Per-track orchestration: decode + run all detectors + classify mixability.

Returns a dict matching schema/sidecar.schema.json v2.
"""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any

from . import __version__
from .decode import DecodeError, decode_audio
from .detect_bands import detect_band_energies
from .detect_beat_grid import detect_beat_grid
from .detect_bpm import detect_bpm
from .detect_cues import detect_cues
from .detect_downbeats import DownbeatResult, compute_chroma_ssm, detect_phrases
from .detect_energy import detect_energy
from .detect_key import detect_key
from .detect_novelty import EMPTY_NOVELTY, detect_novelty
from .detect_stems import detect_stems
from .mixability import classify

SCHEMA_VERSION = 2

_log = logging.getLogger("dj_analyze.analyze")


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


def analyze_track(path: Path, *, stems_root: Path | None = None) -> dict[str, Any]:
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

    # Single madmom RNN+DBN pass — used for beats, downbeats, and confidences.
    grid = detect_beat_grid(audio, sr)
    beats = grid.beats
    db_result = DownbeatResult(downbeats=grid.downbeats, method="madmom")

    bpm, bpm_conf = detect_bpm(audio, sr)

    local_bpm = compute_local_bpm(beats=beats, downbeats=db_result.downbeats)

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
        else EMPTY_NOVELTY
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
        novelty_peaks=novelty.peaks_seconds,
        novelty_strengths=novelty.peak_strengths,
        novelty_confidence=novelty.confidence,
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
        "local_bpm": local_bpm,
        "beat_confidence": grid.beat_confidence,
        "downbeat_confidence": grid.downbeat_confidence,
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

    if stems_root is not None:
        try:
            sa = detect_stems(
                audio=audio,
                sr=sr,
                downbeats_seconds=db_result.downbeats,
                track_basename=path.stem,
                stems_root=stems_root,
            )
            out["stems"] = {
                "drum_density": sa.drum_density,
                "bass_density": sa.bass_density,
                "vocal_density": sa.vocal_density,
                "vocal_segments": [[s, e] for s, e in sa.vocal_segments],
                "energy_per_stem": sa.energy_per_stem,
                "demucs_version": sa.demucs_version,
                "model": sa.model,
            }
        except Exception as e:
            _log.warning("detect_stems failed for %s: %s", path, e)

    return out
