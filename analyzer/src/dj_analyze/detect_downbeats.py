"""Downbeat detection. Uses madmom when available; falls back to assuming 4/4."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

import numpy as np


@dataclass(frozen=True, slots=True)
class DownbeatResult:
    downbeats: list[float]
    method: Literal["madmom", "fallback_4_4"]


def detect_downbeats(
    audio: np.ndarray | None,
    sr: int,
    beats: list[float],
    *,
    force_fallback: bool = False,
) -> DownbeatResult:
    """Return downbeat times in seconds.

    If `force_fallback=True` or madmom is not importable / fails, returns every
    4th beat starting from index 0 (assumes 4/4).
    """
    if not force_fallback and audio is not None:
        try:
            return _madmom_downbeats(audio, sr)
        except Exception:
            pass
    return DownbeatResult(downbeats=beats[::4], method="fallback_4_4")


def _madmom_downbeats(audio: np.ndarray, sr: int) -> DownbeatResult:
    # Imported lazily so absence of madmom doesn't break imports of this module.
    from madmom.features.beats import DBNDownBeatTrackingProcessor  # type: ignore[import-not-found]
    from madmom.features.downbeats import RNNDownBeatProcessor  # type: ignore[import-not-found]

    rnn = RNNDownBeatProcessor()
    dbn = DBNDownBeatTrackingProcessor(beats_per_bar=[3, 4], fps=100)
    activations = rnn(audio.astype(np.float32))
    beats_and_positions: np.ndarray = dbn(activations)
    # beats_and_positions is an array of shape (N, 2): [time, beat_position_in_bar (1..N)]
    downbeats = [
        float(t) for t, pos in beats_and_positions if int(round(pos)) == 1
    ]
    return DownbeatResult(downbeats=downbeats, method="madmom")
