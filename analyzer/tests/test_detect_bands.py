from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_bands import detect_band_energies

FIXTURES = Path(__file__).parent / "fixtures"


def test_bass_only_track_has_high_bass_low_mid_low_high() -> None:
    audio, sr = decode_audio(FIXTURES / "bass_only_80hz.wav")
    result = detect_band_energies(audio, sr)
    means = result["means"]
    assert means["bass"] > 0.5, f"expected bass mean > 0.5, got {means['bass']}"
    assert means["mid"] < 0.1, f"expected mid mean < 0.1, got {means['mid']}"
    assert means["high"] < 0.1, f"expected high mean < 0.1, got {means['high']}"


def test_high_only_track_has_high_high_low_bass_low_mid() -> None:
    audio, sr = decode_audio(FIXTURES / "high_only_8khz.wav")
    result = detect_band_energies(audio, sr)
    means = result["means"]
    assert means["high"] > 0.5, f"expected high mean > 0.5, got {means['high']}"
    assert means["mid"] < 0.1, f"expected mid mean < 0.1, got {means['mid']}"
    assert means["bass"] < 0.1, f"expected bass mean < 0.1, got {means['bass']}"


def test_curves_have_one_value_per_second() -> None:
    audio, sr = decode_audio(FIXTURES / "bass_only_80hz.wav")  # 6s fixture
    result = detect_band_energies(audio, sr)
    for band in ("bass", "mid", "high"):
        curve = result["curves"][band]
        assert 5 <= len(curve) <= 7, f"{band}: expected ~6 entries, got {len(curve)}"


def test_curve_values_in_unit_range() -> None:
    audio, sr = decode_audio(FIXTURES / "bass_only_80hz.wav")
    result = detect_band_energies(audio, sr)
    for band in ("bass", "mid", "high"):
        for v in result["curves"][band]:
            assert 0.0 <= v <= 1.0, f"{band}: value {v} out of [0, 1]"
