"""BPM detection with a confidence score based on tempo stability across windows."""

from __future__ import annotations

import librosa
import numpy as np


PERCEPTUAL_LOW = 70.0
PERCEPTUAL_HIGH = 180.0
PERCEPTUAL_DEFAULT = 120.0
AGREEMENT_TOLERANCE = 0.06  # ±6%, matches _VOTE_TOLERANCE below.


def bpm_from_beats(beat_frames: np.ndarray, sr: int) -> float | None:
    """Derive a BPM estimate from beat-frame positions by taking 60 / median IBI.

    Returns ``None`` when the input has fewer than 2 beats or the median inter-beat
    interval is non-positive. Used as a second, independent BPM estimate to cross-check
    the global ``librosa.beat.tempo`` output for octave errors.
    """
    if beat_frames is None or len(beat_frames) < 2:
        return None
    beat_times = librosa.frames_to_time(beat_frames, sr=sr)
    intervals = np.diff(beat_times)
    if len(intervals) == 0:
        return None
    median_ibi = float(np.median(intervals))
    if median_ibi <= 0:
        return None
    return 60.0 / median_ibi


def _snap_to_perceptual(bpm: float) -> float:
    """Octave-shift ``bpm`` into the perceptual range [70, 180] when possible.

    Multiplies by 2 if too low, divides by 2 if too high, up to a safety bound to
    avoid infinite loops on degenerate inputs. Returns the input unchanged when it's
    zero, negative, or already in range.
    """
    if bpm <= 0:
        return bpm
    out = bpm
    safety = 10
    while out < PERCEPTUAL_LOW and safety > 0:
        out *= 2.0
        safety -= 1
    while out > PERCEPTUAL_HIGH and safety > 0:
        out /= 2.0
        safety -= 1
    return float(out)


def resolve_octave(primary: float, secondary: float | None) -> float:
    """Pick the best BPM given two estimates and the perceptual-range prior.

    When ``secondary`` is ``None``, returns ``_snap_to_perceptual(primary)``.

    Otherwise, generates ``{÷2, ×1, ×2}`` octave candidates for both estimates,
    filters to those inside ``[PERCEPTUAL_LOW, PERCEPTUAL_HIGH]``, and looks for an
    agreeing pair within ``AGREEMENT_TOLERANCE``. On agreement, returns the average.
    On disagreement, returns the in-range candidate closest to ``PERCEPTUAL_DEFAULT``.
    If nothing is in range, falls back to ``_snap_to_perceptual(primary)``.
    """
    if primary <= 0:
        # Trust the secondary if it exists and is positive, else give up.
        if secondary is not None and secondary > 0:
            return _snap_to_perceptual(secondary)
        return primary

    if secondary is None:
        return _snap_to_perceptual(primary)

    primary_candidates = (primary / 2.0, primary, primary * 2.0)
    secondary_candidates = (secondary / 2.0, secondary, secondary * 2.0)
    primary_in_range = [c for c in primary_candidates if PERCEPTUAL_LOW <= c <= PERCEPTUAL_HIGH]
    secondary_in_range = [c for c in secondary_candidates if PERCEPTUAL_LOW <= c <= PERCEPTUAL_HIGH]

    best_pair: tuple[float, float] | None = None
    best_rel_diff = float("inf")
    for p in primary_in_range:
        for s in secondary_in_range:
            rel = abs(p - s) / max(p, s, 1e-9)
            if rel < best_rel_diff:
                best_rel_diff = rel
                best_pair = (p, s)

    if best_pair is not None and best_rel_diff <= AGREEMENT_TOLERANCE:
        return float((best_pair[0] + best_pair[1]) / 2.0)

    all_in_range = primary_in_range + secondary_in_range
    if all_in_range:
        return float(min(all_in_range, key=lambda c: abs(c - PERCEPTUAL_DEFAULT)))

    return _snap_to_perceptual(primary)


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


def _octave_normalize(window_bpm: float, target_bpm: float) -> float:
    """Return whichever of {window_bpm/4, /2, *1, *2, *4} is closest to ``target_bpm``.

    ``librosa.beat.tempo`` on short windows is octave-ambiguous — on a steady 152 BPM
    track a single window may confidently report 76 (half-time interpretation) or 304
    (double-time), especially during breakdowns. Without this correction, one flipped
    window destroys the standard-deviation estimate and the disagreement multiplier,
    zeroing the confidence on tracks whose tempo is actually rock-solid.
    """
    if window_bpm <= 0 or target_bpm <= 0:
        return window_bpm
    candidates = (
        window_bpm / 4.0,
        window_bpm / 2.0,
        window_bpm,
        window_bpm * 2.0,
        window_bpm * 4.0,
    )
    return float(min(candidates, key=lambda c: abs(c - target_bpm)))


_VOTE_TOLERANCE = 0.06  # ±6% — matches the engine's BPM-compatibility window.


def _tempo_stability(audio: np.ndarray, sr: int, global_bpm: float) -> float:
    """Vote-based tempo confidence: fraction of windows agreeing with the global BPM.

    Each 10-second window's BPM is octave-normalized against the global BPM, then
    "votes" if it's within ``_VOTE_TOLERANCE`` of global. Confidence = votes / windows.

    This replaces the prior std-dev + disagreement-multiplier metric, which was
    outlier-sensitive — a few windows locked onto a non-octave fraction (e.g., 2/3
    of the real tempo, from syncopated onset patterns) pushed the rel-std past the
    cutoff and zeroed the confidence on tracks whose tempo is actually rock-solid.
    Voting is robust to such outliers: they simply don't contribute to the score.
    """
    window_seconds = 10.0
    window = int(window_seconds * sr)
    if len(audio) < window * 2:
        # Not enough data for windowing — fall back to a flat 0.75 (we got *a* number).
        return 0.75

    bpms: list[float] = []
    for start in range(0, len(audio) - window, window):
        seg = audio[start : start + window]
        env = librosa.onset.onset_strength(y=seg, sr=sr)
        t = librosa.beat.tempo(onset_envelope=env, sr=sr, aggregate=np.median)
        bpms.append(float(t[0]))

    if not bpms or global_bpm <= 0:
        return 0.0

    votes = 0
    for b in bpms:
        normed = _octave_normalize(b, global_bpm)
        if abs(normed - global_bpm) / global_bpm <= _VOTE_TOLERANCE:
            votes += 1
    return float(votes / len(bpms))
