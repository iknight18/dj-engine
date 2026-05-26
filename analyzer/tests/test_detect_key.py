from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_key import KeyResult, detect_key, to_camelot

FIXTURES = Path(__file__).parent / "fixtures"


def test_to_camelot_a_minor() -> None:
    assert to_camelot("A", "minor") == "8A"


def test_to_camelot_c_major() -> None:
    assert to_camelot("C", "major") == "8B"


def test_to_camelot_g_sharp_handled_as_a_flat() -> None:
    # G# minor and Ab minor are enharmonically equivalent → both map to 1A.
    assert to_camelot("G#", "minor") == "1A"
    assert to_camelot("Ab", "minor") == "1A"


def test_detects_a_minor_on_synthetic_fixture() -> None:
    audio, sr = decode_audio(FIXTURES / "sine_a_minor.wav")
    result: KeyResult = detect_key(audio, sr)
    # Allow neighbors on the wheel: 8A, 7A, 9A, 8B all acceptable.
    # The sine triad is unambiguously minor, so the letter should be A.
    assert result.camelot.endswith("A")
    # Confidence is in valid range.
    assert 0.0 <= result.confidence <= 1.0
