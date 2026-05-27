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
