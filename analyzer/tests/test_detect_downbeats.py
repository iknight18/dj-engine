from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_downbeats import DownbeatResult, detect_downbeats

FIXTURES = Path(__file__).parent / "fixtures"


def test_fallback_returns_every_fourth_beat() -> None:
    beats = [0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5]
    result: DownbeatResult = detect_downbeats(
        audio=None, sr=44100, beats=beats, force_fallback=True
    )
    assert result.method == "fallback_4_4"
    assert result.downbeats == [0.5, 2.5, 4.5]


def test_fallback_handles_empty_beats() -> None:
    result = detect_downbeats(audio=None, sr=44100, beats=[], force_fallback=True)
    assert result.downbeats == []
    assert result.method == "fallback_4_4"


def test_madmom_path_or_fallback_returns_valid_result() -> None:
    """If madmom is installed, it runs; if not, we get the fallback. Either way
    we get a list of monotonically increasing floats."""
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = [0.0 + 0.5 * i for i in range(16)]
    result = detect_downbeats(audio, sr, beats=beats)
    assert all(d1 <= d2 for d1, d2 in zip(result.downbeats, result.downbeats[1:]))
    assert result.method in {"madmom", "fallback_4_4"}
