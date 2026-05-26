from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_beats import detect_beats

FIXTURES = Path(__file__).parent / "fixtures"


def test_beats_spacing_is_half_a_second_at_120_bpm() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = detect_beats(audio, sr, bpm=120.0)
    # 8 seconds at 120 BPM → ~16 beats.
    assert 14 <= len(beats) <= 18
    # Mean spacing should be ~0.5s (allow ±20ms).
    deltas = [b - a for a, b in zip(beats, beats[1:])]
    mean_delta = sum(deltas) / len(deltas)
    assert 0.48 <= mean_delta <= 0.52


def test_beats_are_sorted_and_within_duration() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = detect_beats(audio, sr, bpm=120.0)
    assert all(b1 <= b2 for b1, b2 in zip(beats, beats[1:]))
    duration = len(audio) / sr
    assert all(0 <= b <= duration for b in beats)
