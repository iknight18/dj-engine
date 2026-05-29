from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_beats import detect_beats

FIXTURES = Path(__file__).parent / "fixtures"


def test_detect_beats_returns_sorted_floats():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = detect_beats(audio, sr)
    assert isinstance(beats, list)
    assert all(isinstance(b, float) for b in beats)
    assert all(a < b for a, b in zip(beats, beats[1:]))


def test_detect_beats_finds_at_least_8_beats_on_click_fixture():
    """A 120 BPM click fixture of reasonable length should yield ≥8 beats."""
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = detect_beats(audio, sr)
    assert len(beats) >= 8, f"expected ≥8 beats on click track, got {len(beats)}"
