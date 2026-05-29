"""Downbeat detection (madmom) + phrase-structure detection (librosa SSM).

After the madmom-required switch, downbeats come exclusively from
detect_beat_grid. The librosa-based phrase / chroma-SSM helpers below are
retained — they are used by analyze.py to derive phrase boundaries and to
feed novelty detection, independent of the downbeat detector.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

import numpy as np

from .detect_beat_grid import detect_beat_grid


@dataclass(frozen=True, slots=True)
class DownbeatResult:
    downbeats: list[float]
    method: Literal["madmom"]


@dataclass(frozen=True, slots=True)
class PhraseResult:
    boundaries: list[float]
    confidence: float


def detect_downbeats(audio: np.ndarray, sr: int) -> DownbeatResult:
    """Return downbeat times from the madmom beat-grid pass."""
    grid = detect_beat_grid(audio, sr)
    return DownbeatResult(downbeats=grid.downbeats, method="madmom")


# Candidate beat periods we look for in the self-similarity matrix. These are SEARCH
# periods used only for anchoring on a musically meaningful repeat — the OUTPUT
# stride is independent (set by `output_stride_bars`).
_SEARCH_PERIODS_BEATS: tuple[int, ...] = (4, 8, 16)


def compute_chroma_ssm(
    audio: np.ndarray, sr: int, beats: list[float]
) -> np.ndarray | None:
    """Beat-synchronous chroma self-similarity matrix.

    Returns an (N x N) float64 affinity matrix where N is the number of beats,
    or None when the track is too short to build a meaningful SSM (fewer than 4
    beats, sub-second audio, or fewer chroma frames than the minimum search period).
    """
    if len(beats) < 4 or len(audio) < sr:
        return None
    import librosa

    beat_frames = librosa.time_to_frames(np.asarray(beats), sr=sr)
    chroma = librosa.feature.chroma_cqt(y=audio, sr=sr)
    chroma_sync = librosa.util.sync(chroma, beat_frames, aggregate=np.median)
    if chroma_sync.shape[1] < min(_SEARCH_PERIODS_BEATS) + 1:
        return None
    if hasattr(librosa.segment, "cross_similarity"):
        sim = librosa.segment.cross_similarity(
            chroma_sync, chroma_sync, mode="affinity"
        )
    else:
        sim = librosa.segment.recurrence_matrix(chroma_sync, mode="affinity")
    return np.asarray(sim, dtype=np.float64)


def detect_phrases(
    audio: np.ndarray,
    sr: int,
    beats: list[float],
    *,
    output_stride_bars: int = 16,
    phrase_bars: int | None = None,  # legacy alias for output_stride_bars
    chroma_ssm: np.ndarray | None = None,  # reuse a precomputed SSM if available
) -> PhraseResult:
    """Find repeating phrase structure and return boundaries at a fixed bar stride.

    The detector searches for the strongest periodicity in {4, 8, 16} beats to find
    where structural repetition begins (the "anchor"). It then emits boundaries every
    ``output_stride_bars`` BARS from that anchor (default 16 bars = standard DJ phrase
    length, assuming 4/4 time with 4 beats per bar). The search period and the output
    stride are independent — short detected periods give us reliable anchoring, and
    the configurable stride controls the granularity of the output for the caller.

    Returns boundaries (seconds) and a confidence in [0, 1] reflecting how strongly
    the audio's self-similarity matrix exhibits the detected period.

    Pass ``chroma_ssm`` to skip recomputing the SSM when the caller (analyze.py) has
    already built it for novelty detection. The ``phrase_bars`` keyword is accepted
    as a legacy alias so existing callers that meant "stride in bars" don't silently
    break.
    """
    if phrase_bars is not None:
        output_stride_bars = phrase_bars
    if len(beats) < 4 or len(audio) < sr:
        return PhraseResult(boundaries=[], confidence=0.0)

    sim = chroma_ssm if chroma_ssm is not None else compute_chroma_ssm(audio, sr, beats)
    if sim is None:
        return PhraseResult(boundaries=[], confidence=0.0)

    candidate_periods = [p for p in _SEARCH_PERIODS_BEATS if p < sim.shape[0]]
    if not candidate_periods:
        return PhraseResult(boundaries=[], confidence=0.0)
    scores: dict[int, float] = {}
    for p in candidate_periods:
        diag = [sim[i, i + p] for i in range(sim.shape[0] - p)]
        scores[p] = float(np.max(diag)) if diag else 0.0

    best_period = max(scores, key=lambda k: scores[k])
    best_score = scores[best_period]
    confidence = float(max(0.0, min(1.0, best_score)))

    median = float(np.median(sim))
    anchor = 0
    for i in range(sim.shape[0] - best_period):
        if sim[i, i + best_period] > median:
            anchor = i
            break

    stride_beats = max(1, output_stride_bars) * 4
    boundary_indices = list(range(anchor, len(beats), stride_beats))
    boundaries = [beats[i] for i in boundary_indices]
    return PhraseResult(boundaries=boundaries, confidence=confidence)
