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
        # Bar-level downbeats: stride by 1 bar (4 beats) for downbeat semantics.
        phrase = detect_phrases(audio, sr, beats=beats, output_stride_bars=1)
        if phrase.confidence >= 0.3 and phrase.boundaries:
            return DownbeatResult(downbeats=phrase.boundaries, method="librosa_phrase")
    # Last-resort fallback: every 4th beat. Used only when librosa phrase fails too.
    return DownbeatResult(downbeats=beats[::4], method="fallback_4_4")


# Candidate beat periods we look for in the self-similarity matrix. These are SEARCH
# periods used only for anchoring on a musically meaningful repeat — the OUTPUT
# stride is independent (set by `output_stride_bars`).
_SEARCH_PERIODS_BEATS: tuple[int, ...] = (4, 8, 16)


def detect_phrases(
    audio: np.ndarray,
    sr: int,
    beats: list[float],
    *,
    output_stride_bars: int = 16,
    phrase_bars: int | None = None,  # legacy alias for output_stride_bars
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

    The ``phrase_bars`` keyword is accepted as a legacy alias so existing callers
    that meant "stride in bars" don't silently break.
    """
    if phrase_bars is not None:
        output_stride_bars = phrase_bars
    if len(beats) < 4 or len(audio) < sr:
        return PhraseResult(boundaries=[], confidence=0.0)

    import librosa  # local import keeps module import cheap

    beat_frames = librosa.time_to_frames(np.asarray(beats), sr=sr)
    chroma = librosa.feature.chroma_cqt(y=audio, sr=sr)
    # Beat-synchronous chroma, one column per beat.
    chroma_sync = librosa.util.sync(chroma, beat_frames, aggregate=np.median)
    if chroma_sync.shape[1] < min(_SEARCH_PERIODS_BEATS) + 1:
        return PhraseResult(boundaries=[], confidence=0.0)

    # Self-similarity matrix on cosine distance. Prefer cross_similarity (newer librosa);
    # fall back to recurrence_matrix on older versions.
    if hasattr(librosa.segment, "cross_similarity"):
        sim = librosa.segment.cross_similarity(chroma_sync, chroma_sync, mode="affinity")
    else:
        sim = librosa.segment.recurrence_matrix(chroma_sync, mode="affinity")
    # For each candidate period score the diagonal at offset P. We use max-of-diagonal
    # (not mean) so the score reflects the STRONGEST aligned pair, rather than being
    # washed out by far-apart pairs whose affinity is weak.
    candidate_periods = [p for p in _SEARCH_PERIODS_BEATS if p < sim.shape[0]]
    if not candidate_periods:
        return PhraseResult(boundaries=[], confidence=0.0)
    scores: dict[int, float] = {}
    for p in candidate_periods:
        diag = [sim[i, i + p] for i in range(sim.shape[0] - p)]
        scores[p] = float(np.max(diag)) if diag else 0.0

    best_period = max(scores, key=lambda k: scores[k])
    best_score = scores[best_period]
    # Confidence is the strongest affinity at the best phrase offset, clipped to [0, 1].
    confidence = float(max(0.0, min(1.0, best_score)))

    # Anchor: first beat where similarity to (beat + best_period) is above the matrix median.
    median = float(np.median(sim))
    anchor = 0
    for i in range(sim.shape[0] - best_period):
        if sim[i, i + best_period] > median:
            anchor = i
            break

    # Emit boundaries at the OUTPUT STRIDE, not the detected period. For 4/4 time,
    # 1 bar = 4 beats, so stride_beats = output_stride_bars * 4. Anchoring on the
    # detected repeat point still lines us up with where the music's structure starts.
    stride_beats = max(1, output_stride_bars) * 4
    boundary_indices = list(range(anchor, len(beats), stride_beats))
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
