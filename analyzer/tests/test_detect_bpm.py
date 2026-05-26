from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_bpm import detect_bpm

FIXTURES = Path(__file__).parent / "fixtures"


def test_detects_120_bpm_within_tolerance() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    bpm, confidence = detect_bpm(audio, sr)
    assert 118.0 <= bpm <= 122.0
    assert 0.0 <= confidence <= 1.0


def test_confidence_is_high_on_clean_click_track() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    _, confidence = detect_bpm(audio, sr)
    # A perfect click should give high confidence.
    assert confidence > 0.7
