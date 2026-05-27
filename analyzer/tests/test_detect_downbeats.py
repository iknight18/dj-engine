from __future__ import annotations

from pathlib import Path

import numpy as np

from dj_analyze.decode import decode_audio
from dj_analyze.detect_downbeats import (
    DownbeatResult,
    compute_chroma_ssm,
    detect_downbeats,
    detect_phrases,
)

FIXTURES = Path(__file__).parent / "fixtures"


def test_fallback_handles_empty_beats() -> None:
    result = detect_downbeats(audio=None, sr=44100, beats=[], force_fallback=True)
    assert result.downbeats == []
    assert result.method == "fallback_4_4"


def test_downbeats_path_or_phrase_fallback_returns_valid_result() -> None:
    """Whichever path runs, result is a valid DownbeatResult with sorted times."""
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = [0.0 + 0.5 * i for i in range(16)]
    result: DownbeatResult = detect_downbeats(audio, sr, beats=beats)
    assert all(
        d1 <= d2 for d1, d2 in zip(result.downbeats, result.downbeats[1:], strict=False)
    )
    assert result.method in {"madmom", "librosa_phrase", "fallback_4_4"}


def test_force_fallback_when_no_audio_returns_4_4() -> None:
    """With force_fallback=True and beats provided, we get every 4th beat."""
    beats = [0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5]
    result = detect_downbeats(audio=None, sr=44100, beats=beats, force_fallback=True)
    assert result.method == "fallback_4_4"
    assert result.downbeats == [0.5, 2.5, 4.5]


def test_detect_phrases_on_clean_click_returns_high_confidence_and_boundaries() -> None:
    """Click track has a perfect repeating 1-bar pattern, should pass confidence threshold."""
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = [0.5 * i for i in range(16)]  # 16 beats at 120 BPM = 4 bars
    # output_stride_bars=1: 4-beat stride, the click yields multiple boundaries.
    result = detect_phrases(audio, sr, beats=beats, output_stride_bars=1)
    assert result.confidence > 0.3, f"expected confidence > 0.3, got {result.confidence}"
    assert len(result.boundaries) >= 2


def test_detect_phrases_16_bar_stride_yields_fewer_boundaries() -> None:
    """The default stride emits boundaries every 16 bars (64 beats) — on a short
    click track that's at most one boundary."""
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = [0.5 * i for i in range(16)]  # only 16 beats total
    result = detect_phrases(audio, sr, beats=beats)  # default output_stride_bars=16
    # With stride 64 beats and only 16 beats of audio, we get at most one boundary
    # (the anchor itself).
    assert len(result.boundaries) <= 1


def test_detect_phrases_on_noise_returns_low_confidence_no_boundaries() -> None:
    """White noise has no structure, confidence should be low."""
    import numpy as np

    rng = np.random.default_rng(seed=42)
    audio = rng.uniform(-0.5, 0.5, size=44100 * 4).astype(np.float32)  # 4s of noise
    beats = [0.5 * i for i in range(8)]
    result = detect_phrases(audio, sr=44100, beats=beats, output_stride_bars=1)
    # Loose threshold, librosa may find weak similarity in random data.
    assert result.confidence < 0.6, f"expected low confidence on noise, got {result.confidence}"


def test_detect_phrases_handles_too_few_beats() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    result = detect_phrases(audio, sr, beats=[0.0, 0.5], output_stride_bars=1)
    assert result.boundaries == []
    assert result.confidence == 0.0


def test_detect_phrases_phrase_bars_legacy_alias_still_works() -> None:
    """`phrase_bars` is kept as a legacy alias for `output_stride_bars` so existing
    callers don't silently break."""
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = [0.5 * i for i in range(16)]
    via_legacy = detect_phrases(audio, sr, beats=beats, phrase_bars=1)
    via_new = detect_phrases(audio, sr, beats=beats, output_stride_bars=1)
    assert via_legacy.boundaries == via_new.boundaries
    assert via_legacy.confidence == via_new.confidence


def test_compute_chroma_ssm_returns_square_matrix():
    sr = 22050
    # 8 seconds of audio with two halves in different keys.
    seg = sr * 4
    a = np.sin(2 * np.pi * 220 * np.arange(seg) / sr).astype(np.float32) * 0.5  # A3
    b = np.sin(2 * np.pi * 330 * np.arange(seg) / sr).astype(np.float32) * 0.5  # E4
    audio = np.concatenate([a, b])
    # Beats every 0.5s (simulated 120 BPM)
    beats = [0.5 * i for i in range(16)]
    ssm = compute_chroma_ssm(audio, sr, beats)
    assert ssm is not None
    assert ssm.ndim == 2
    assert ssm.shape[0] == ssm.shape[1]
    # Affinity matrix in [0, 1].
    assert ssm.min() >= 0.0 - 1e-6
    assert ssm.max() <= 1.0 + 1e-6


def test_compute_chroma_ssm_too_short_returns_none():
    sr = 22050
    audio = np.zeros(100, dtype=np.float32)
    assert compute_chroma_ssm(audio, sr, beats=[]) is None


def test_detect_phrases_accepts_precomputed_ssm():
    sr = 22050
    seg = sr * 4
    a = np.sin(2 * np.pi * 220 * np.arange(seg) / sr).astype(np.float32) * 0.5
    b = np.sin(2 * np.pi * 330 * np.arange(seg) / sr).astype(np.float32) * 0.5
    audio = np.concatenate([a, b])
    beats = [0.5 * i for i in range(16)]
    ssm = compute_chroma_ssm(audio, sr, beats)
    # Passing the precomputed SSM should yield the same result as not passing it.
    r1 = detect_phrases(audio, sr, beats=beats, output_stride_bars=4)
    r2 = detect_phrases(audio, sr, beats=beats, output_stride_bars=4, chroma_ssm=ssm)
    assert r1.boundaries == r2.boundaries
    assert abs(r1.confidence - r2.confidence) < 1e-6
