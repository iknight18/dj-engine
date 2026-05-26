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
