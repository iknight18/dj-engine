"""Single madmom RNN+DBN pass producing beats, downbeats, and per-event confidences.

This module is the foundation for detect_beats and detect_downbeats; both call into
detect_beat_grid and select fields out of the shared result. Confidences are the
RNN activation values (per beat / per downbeat axis) sampled at each detected time.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

# Frame rate of the madmom RNN output. Used to convert detected times to indices
# into the activation array when sampling confidences.
_RNN_FPS = 100


@dataclass(frozen=True, slots=True)
class BeatGrid:
    beats: list[float]
    beat_confidence: list[float]
    downbeats: list[float]
    downbeat_confidence: list[float]


def detect_beat_grid(audio: np.ndarray, sr: int) -> BeatGrid:
    """Detect beats and downbeats with confidences via madmom RNN+DBN.

    Returns a BeatGrid where:
      - beats: all detected beat times (non-uniform; snapped to onsets)
      - downbeats: subset of beats marked as position 1 by the DBN
      - beat_confidence: RNN beat-axis activation sampled at each beat time, [0, 1]
      - downbeat_confidence: RNN downbeat-axis activation sampled at each downbeat, [0, 1]
    """
    from madmom.features.downbeats import (
        DBNDownBeatTrackingProcessor,
        RNNDownBeatProcessor,
    )

    rnn = RNNDownBeatProcessor()
    dbn = DBNDownBeatTrackingProcessor(beats_per_bar=[3, 4], fps=_RNN_FPS)
    activations = rnn(audio.astype(np.float32))

    if activations.size == 0:
        return BeatGrid([], [], [], [])

    beats_and_positions = dbn(activations)
    if len(beats_and_positions) == 0:
        return BeatGrid([], [], [], [])

    beat_act = activations[:, 0]
    downbeat_act = activations[:, 1]

    def sample_at(arr: np.ndarray, t: float) -> float:
        idx = int(round(t * _RNN_FPS))
        idx = max(0, min(idx, len(arr) - 1))
        return float(arr[idx])

    beats: list[float] = []
    beat_confidence: list[float] = []
    downbeats: list[float] = []
    downbeat_confidence: list[float] = []

    for t, pos in beats_and_positions:
        t_f = float(t)
        beats.append(t_f)
        beat_confidence.append(sample_at(beat_act, t_f))
        if int(round(pos)) == 1:
            downbeats.append(t_f)
            downbeat_confidence.append(sample_at(downbeat_act, t_f))

    return BeatGrid(
        beats=beats,
        beat_confidence=beat_confidence,
        downbeats=downbeats,
        downbeat_confidence=downbeat_confidence,
    )
