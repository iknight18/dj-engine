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
