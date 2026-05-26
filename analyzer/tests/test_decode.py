from __future__ import annotations

from pathlib import Path

import numpy as np
import pytest

from dj_analyze.decode import DecodeError, decode_audio

FIXTURES = Path(__file__).parent / "fixtures"


def test_decode_returns_mono_float32_at_target_sr() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    assert audio.dtype == np.float32
    assert audio.ndim == 1
    assert sr == 44100
    # Click fixture is 8 seconds
    assert abs(len(audio) / sr - 8.0) < 0.05


def test_decode_normalizes_to_minus_one_one_range() -> None:
    audio, _ = decode_audio(FIXTURES / "click_120bpm.wav")
    assert audio.max() <= 1.0
    assert audio.min() >= -1.0


def test_decode_raises_on_corrupt_file() -> None:
    with pytest.raises(DecodeError):
        decode_audio(FIXTURES / "corrupt.mp3")


def test_decode_raises_on_missing_file(tmp_path: Path) -> None:
    with pytest.raises(DecodeError):
        decode_audio(tmp_path / "does-not-exist.mp3")
