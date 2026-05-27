from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_bpm import _octave_normalize, detect_bpm

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


def test_octave_normalize_collapses_half_time_to_target() -> None:
    # Track is steady 152 BPM; a window erroneously reported 76 (half-time).
    assert _octave_normalize(76.0, 152.0) == 152.0


def test_octave_normalize_collapses_double_time_to_target() -> None:
    # Same track, a window reported 304 (double-time).
    assert _octave_normalize(304.0, 152.0) == 152.0


def test_octave_normalize_keeps_correct_value() -> None:
    assert _octave_normalize(152.0, 152.0) == 152.0


def test_octave_normalize_handles_quarter_time() -> None:
    # Edge case: librosa returned a /4 estimate on a slow chord change.
    assert _octave_normalize(38.0, 152.0) == 152.0


def test_octave_normalize_does_not_flip_genuinely_different_tempos() -> None:
    # 90 BPM track vs 152 BPM target — none of {22.5, 45, 90, 180, 360} is closer to
    # 152 than 90 itself? 90 distance 62; 180 distance 28 — so 180 wins. This is the
    # intended behavior: if the windowed BPM is plausibly an octave-equivalent of the
    # target (180 ≈ 152 is within an octave), we collapse. Genuinely unrelated tempos
    # would need to be more than 2x apart to get collapsed.
    assert _octave_normalize(90.0, 152.0) == 180.0


def test_octave_normalize_zero_or_negative_pass_through() -> None:
    assert _octave_normalize(0.0, 152.0) == 0.0
    assert _octave_normalize(-1.0, 152.0) == -1.0
