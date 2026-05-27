"""Downbeat detection.

Tries madmom (gold standard) first; falls back to a librosa-based phrase detector
that finds repetition periods via self-similarity. Both produce sorted timestamps.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

import numpy as np


@dataclass(frozen=True, slots=True)
class DownbeatResult:
    downbeats: list[float]
    method: Literal["madmom", "librosa_phrase", "fallback_4_4"]


@dataclass(frozen=True, slots=True)
class PhraseResult:
    boundaries: list[float]
    confidence: float  # 0..1; ratio of best to second-best period score


def detect_downbeats(
    audio: np.ndarray | None,
    sr: int,
    beats: list[float],
    *,
    force_fallback: bool = False,
) -> DownbeatResult:
    """Return downbeat times. Prefers madmom, then librosa phrase, then 4/4 fallback."""
    if not force_fallback and audio is not None:
        try:
            return _madmom_downbeats(audio, sr)
        except Exception:
            pass
    if not force_fallback and audio is not None and beats:
        phrase = detect_phrases(audio, sr, beats=beats, phrase_bars=4)
        if phrase.confidence >= 0.3 and phrase.boundaries:
            return DownbeatResult(downbeats=phrase.boundaries, method="librosa_phrase")
    # Last-resort fallback: every 4th beat. Used only when librosa phrase fails too.
    return DownbeatResult(downbeats=beats[::4], method="fallback_4_4")


def detect_phrases(
    audio: np.ndarray,
    sr: int,
    beats: list[float],
    *,
    phrase_bars: int = 4,
) -> PhraseResult:
    """Find repeating phrase structure in `audio` at the beat scale.

    Returns boundaries (in seconds) and a confidence score in [0, 1] derived from
    the strongest periodicity in the beat-synchronous self-similarity matrix.
    Confidence is low (~0) when no repeating structure is found.
    """
    if len(beats) < 4 or len(audio) < sr:
        return PhraseResult(boundaries=[], confidence=0.0)

    import librosa  # local import keeps module import cheap

    beat_frames = librosa.time_to_frames(np.asarray(beats), sr=sr)
    chroma = librosa.feature.chroma_cqt(y=audio, sr=sr)
    # Beat-synchronous chroma, one column per beat.
    chroma_sync = librosa.util.sync(chroma, beat_frames, aggregate=np.median)
    if chroma_sync.shape[1] < 2 * phrase_bars + 1:
        return PhraseResult(boundaries=[], confidence=0.0)

    # Self-similarity matrix on cosine distance. Prefer cross_similarity (newer librosa);
    # fall back to recurrence_matrix on older versions.
    if hasattr(librosa.segment, "cross_similarity"):
        sim = librosa.segment.cross_similarity(chroma_sync, chroma_sync, mode="affinity")
    else:
        sim = librosa.segment.recurrence_matrix(chroma_sync, mode="affinity")
    # For each candidate period P in {4, 8, 16} beats, score the diagonal at offset P.
    # We use max-of-diagonal (not mean) so the score reflects the STRONGEST aligned pair,
    # rather than being washed out by far-apart pairs whose affinity is weak.
    candidate_periods = [p for p in (4, 8, 16) if p < sim.shape[0]]
    if not candidate_periods:
        return PhraseResult(boundaries=[], confidence=0.0)
    scores: dict[int, float] = {}
    for p in candidate_periods:
        diag = [sim[i, i + p] for i in range(sim.shape[0] - p)]
        scores[p] = float(np.max(diag)) if diag else 0.0

    best_period = max(scores, key=lambda k: scores[k])
    best_score = scores[best_period]
    # Confidence is the strongest affinity at the best phrase offset, clipped to [0, 1].
    # Random / structureless audio yields sparse affinity → low max → low confidence.
    confidence = float(max(0.0, min(1.0, best_score)))

    # Anchor: first beat where similarity to (beat + best_period) is above the matrix median.
    median = float(np.median(sim))
    anchor = 0
    for i in range(sim.shape[0] - best_period):
        if sim[i, i + best_period] > median:
            anchor = i
            break

    boundary_indices = list(range(anchor, len(beats), best_period))
    boundaries = [beats[i] for i in boundary_indices]
    return PhraseResult(boundaries=boundaries, confidence=confidence)


def _madmom_downbeats(audio: np.ndarray, sr: int) -> DownbeatResult:
    # Imported lazily so madmom absence doesn't break this module.
    from madmom.features.beats import DBNDownBeatTrackingProcessor  # type: ignore[import-not-found]
    from madmom.features.downbeats import RNNDownBeatProcessor  # type: ignore[import-not-found]

    rnn = RNNDownBeatProcessor()
    dbn = DBNDownBeatTrackingProcessor(beats_per_bar=[3, 4], fps=100)
    activations = rnn(audio.astype(np.float32))
    beats_and_positions = dbn(activations)
    downbeats = [float(t) for t, pos in beats_and_positions if int(round(pos)) == 1]
    return DownbeatResult(downbeats=downbeats, method="madmom")
