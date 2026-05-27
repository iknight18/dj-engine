"""Foote-style novelty detection on a fused chroma + onset SSM.

Returns timestamped peaks where the music's structural character changes.
Used by detect_cues.py to anchor intro_end / outro_start at musical boundaries
rather than at regular-grid bar offsets.

Reference: J. Foote, "Visualizing Music and Audio Using Self-Similarity," 1999.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np
from scipy.signal import find_peaks

from .detect_downbeats import compute_chroma_ssm

NOVELTY_KERNEL_BEATS = 16
NOVELTY_MIN_PROMINENCE_MAD = 1.0
NOVELTY_MIN_PEAK_DISTANCE_BEATS = 8


@dataclass(frozen=True, slots=True)
class NoveltyResult:
    peaks_seconds: list[float]    # sorted; one entry per accepted peak
    peak_strengths: list[float]   # parallel to peaks_seconds
    confidence: float             # 0..1; how clearly the strongest peak rises above the noise floor


def detect_novelty(
    audio: np.ndarray,
    sr: int,
    beats: list[float],
    *,
    chroma_ssm: np.ndarray | None = None,
) -> NoveltyResult:
    """Find structural-boundary timestamps via Foote novelty on a fused SSM.

    Args:
        audio: float32 mono PCM.
        sr: sample rate.
        beats: beat timestamps in seconds (from detect_beats).
        chroma_ssm: optional precomputed chroma SSM (from compute_chroma_ssm)
            to avoid duplicate work when the caller has one in hand.

    Returns:
        NoveltyResult with peaks (in seconds), peak strengths, and confidence.
        Returns empty result for sub-second audio or fewer than 4 beats.
    """
    if len(beats) < 4 or len(audio) < sr:
        return NoveltyResult(peaks_seconds=[], peak_strengths=[], confidence=0.0)

    chroma = chroma_ssm if chroma_ssm is not None else compute_chroma_ssm(audio, sr, beats)
    if chroma is None or chroma.shape[0] < NOVELTY_KERNEL_BEATS:
        return NoveltyResult(peaks_seconds=[], peak_strengths=[], confidence=0.0)

    onset = _compute_onset_ssm(audio, sr, beats)
    if onset is None or onset.shape != chroma.shape:
        # Fall back to chroma-only if onset SSM can't be aligned (shouldn't happen).
        fused = chroma
    else:
        fused = chroma * onset

    novelty = _compute_novelty_curve(fused, NOVELTY_KERNEL_BEATS)
    if novelty.size == 0:
        return NoveltyResult(peaks_seconds=[], peak_strengths=[], confidence=0.0)

    peak_indices, strengths = _pick_peaks(novelty)
    if peak_indices.size == 0:
        return NoveltyResult(peaks_seconds=[], peak_strengths=[], confidence=0.0)

    # Each novelty index corresponds to a beat index (the kernel is centered at the beat).
    # peak_indices is into the novelty array which is indexed by beat. Convert to seconds.
    peaks_seconds = [float(beats[min(i, len(beats) - 1)]) for i in peak_indices]
    peak_strengths = [float(s) for s in strengths]

    # Confidence: how high the strongest peak rises above the median of the whole curve,
    # expressed in MADs, clipped to [0, 1].
    median = float(np.median(novelty))
    mad = float(np.median(np.abs(novelty - median)))
    if mad <= 1e-12:
        confidence = 0.0
    else:
        confidence = float(np.clip((max(peak_strengths) - median) / (mad * 4.0), 0.0, 1.0))

    return NoveltyResult(
        peaks_seconds=peaks_seconds,
        peak_strengths=peak_strengths,
        confidence=confidence,
    )


def _compute_onset_ssm(audio: np.ndarray, sr: int, beats: list[float]) -> np.ndarray | None:
    """Beat-synchronous onset-strength SSM (affinity in [0, 1])."""
    import librosa

    onset_env = librosa.onset.onset_strength(y=audio, sr=sr)
    if onset_env.size == 0:
        return None
    beat_frames = librosa.time_to_frames(np.asarray(beats), sr=sr)
    # librosa.util.sync over beats; aggregate=np.mean keeps onset density per beat.
    onset_sync = librosa.util.sync(onset_env, beat_frames, aggregate=np.mean)
    if onset_sync.ndim == 1:
        onset_sync = onset_sync[np.newaxis, :]
    if onset_sync.shape[1] < 4:
        return None
    # Cosine-similarity affinity matrix.
    if hasattr(librosa.segment, "cross_similarity"):
        sim = librosa.segment.cross_similarity(onset_sync, onset_sync, mode="affinity")
    else:
        sim = librosa.segment.recurrence_matrix(onset_sync, mode="affinity")
    return np.asarray(sim, dtype=np.float64)


def _foote_kernel(size_beats: int) -> np.ndarray:
    """Gaussian-tapered checkerboard kernel of size (size_beats x size_beats)."""
    if size_beats % 2 != 0:
        size_beats += 1
    half = size_beats // 2
    # Checkerboard: +1 in TL/BR quadrants, -1 in TR/BL.
    checker = np.ones((size_beats, size_beats), dtype=np.float64)
    checker[:half, half:] = -1.0
    checker[half:, :half] = -1.0
    # Gaussian taper centered at the middle.
    coords = np.arange(size_beats) - (size_beats - 1) / 2.0
    sigma = size_beats / 4.0
    g = np.exp(-(coords**2) / (2.0 * sigma**2))
    taper = np.outer(g, g)
    kernel = checker * taper
    return kernel


def _compute_novelty_curve(fused_ssm: np.ndarray, kernel_size: int) -> np.ndarray:
    """Slide the Foote kernel along the main diagonal; produce a 1-D novelty curve.

    The first and last ``half`` beat positions are left at zero (boundary guard)
    to prevent Foote kernel edge-effects from producing spurious high-confidence peaks
    at the very start or end of the track.
    """
    n = fused_ssm.shape[0]
    if n < kernel_size:
        return np.zeros(0, dtype=np.float64)
    kernel = _foote_kernel(kernel_size)
    half = kernel_size // 2
    curve = np.zeros(n, dtype=np.float64)
    # Leave an extra guard beat at each boundary to suppress edge artifacts.
    guard = half + 1
    for i in range(guard, n - guard):
        block = fused_ssm[i - half : i + half, i - half : i + half]
        curve[i] = float(np.sum(block * kernel))
    return curve


def _pick_peaks(novelty: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Return (peak_indices, peak_strengths) using MAD-based prominence threshold."""
    median = float(np.median(novelty))
    mad = float(np.median(np.abs(novelty - median)))
    if mad <= 1e-12:
        return np.empty(0, dtype=np.int64), np.empty(0, dtype=np.float64)
    prominence = NOVELTY_MIN_PROMINENCE_MAD * mad
    indices, props = find_peaks(
        novelty,
        prominence=prominence,
        distance=NOVELTY_MIN_PEAK_DISTANCE_BEATS,
    )
    if indices.size == 0:
        return np.empty(0, dtype=np.int64), np.empty(0, dtype=np.float64)
    strengths = novelty[indices]
    return indices, strengths
