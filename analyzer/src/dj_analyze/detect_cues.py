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
_SILENCE_GUARD_DB = -59.0  # treat as no signal below this; catches the -120 dB digital silence floor

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
    intro_end is the earliest novelty peak in [cue_in + MIN_INTRO, cue_in + MAX_INTRO]
    snapped to the nearest downbeat; falls back to cue_in + 16 bars when no usable
    peak or when novelty_confidence < NOVELTY_CONFIDENCE_THRESHOLD.
    outro_start is the latest peak in the symmetric window before cue_out, falling
    back to cue_out - 16 bars on the same conditions.
    (Long-track shortening is still Task 6.)
    """
    cue_in, cue_out = _loudness_anchor_cues(
        duration_seconds, downbeats, rms_db_curve, max_rms_db
    )

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


def _loudness_anchor_cues(
    duration: float,
    downbeats: list[float],
    rms_db_curve: list[float],
    max_rms_db: float,
) -> tuple[float, float]:
    """Return (cue_in, cue_out) anchored to -9 dB-of-max windows, snapped to downbeats."""
    if not rms_db_curve or max_rms_db <= _SILENCE_GUARD_DB:
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
    """First downbeat >= t. Assumes downbeats are sorted ascending. If none, return fallback."""
    for db in downbeats:
        if db >= t:
            return float(db)
    return float(fallback)


def _snap_down_to_downbeat(t: float, downbeats: Sequence[float], fallback: float) -> float:
    """Last downbeat <= t. Assumes downbeats are sorted ascending. If none, return fallback."""
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
