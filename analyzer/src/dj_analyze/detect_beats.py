"""Beat-time detection. Thin wrapper over detect_beat_grid (madmom RNN+DBN)."""

from __future__ import annotations

import numpy as np

from .detect_beat_grid import detect_beat_grid


def detect_beats(audio: np.ndarray, sr: int) -> list[float]:
    """Return madmom-detected beat times in seconds. Non-uniform: snapped to onsets."""
    return detect_beat_grid(audio, sr).beats
