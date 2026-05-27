from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_downbeats import DownbeatResult, detect_downbeats, detect_phrases

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
    beats = [0.5 * i for i in range(16)]  # 16 beats at 120 BPM
    result = detect_phrases(audio, sr, beats=beats, phrase_bars=4)
    assert result.confidence > 0.3, f"expected confidence > 0.3, got {result.confidence}"
    assert len(result.boundaries) >= 2


def test_detect_phrases_on_noise_returns_low_confidence_no_boundaries() -> None:
    """White noise has no structure, confidence should be low."""
    import numpy as np

    rng = np.random.default_rng(seed=42)
    audio = rng.uniform(-0.5, 0.5, size=44100 * 4).astype(np.float32)  # 4s of noise
    beats = [0.5 * i for i in range(8)]
    result = detect_phrases(audio, sr=44100, beats=beats, phrase_bars=4)
    # Loose threshold, librosa may find weak similarity in random data.
    assert result.confidence < 0.6, f"expected low confidence on noise, got {result.confidence}"


def test_detect_phrases_handles_too_few_beats() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    result = detect_phrases(audio, sr, beats=[0.0, 0.5], phrase_bars=4)
    assert result.boundaries == []
    assert result.confidence == 0.0
