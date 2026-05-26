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
        # Not enough data for windowing — fall back to a flat 0.75 (we got *a* number).
        return 0.75

    bpms = []
    for start in range(0, len(audio) - window, window):
        seg = audio[start : start + window]
        env = librosa.onset.onset_strength(y=seg, sr=sr)
        t = librosa.beat.tempo(onset_envelope=env, sr=sr, aggregate=np.median)
        bpms.append(float(t[0]))

    if not bpms:
        return 0.75

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
