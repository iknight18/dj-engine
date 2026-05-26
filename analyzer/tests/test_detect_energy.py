from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_energy import detect_energy

FIXTURES = Path(__file__).parent / "fixtures"


def test_energy_curve_length_matches_duration_in_seconds() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    curve, mean = detect_energy(audio, sr)
    # 8s click → curve of ~8 entries (one per second)
    assert 7 <= len(curve) <= 9


def test_energy_curve_values_in_unit_range() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    curve, _ = detect_energy(audio, sr)
    assert all(0.0 <= v <= 1.0 for v in curve)


def test_silence_then_beep_curve_has_low_then_high_then_low() -> None:
    audio, sr = decode_audio(FIXTURES / "silence_then_beep.wav")
    curve, _ = detect_energy(audio, sr)
    # 10s file: roughly seconds 0-2 silence, 3-6 beep, 7-9 silence.
    assert curve[1] < 0.2          # in the silent intro
    assert curve[5] > 0.4          # in the beep
    assert curve[8] < 0.2          # in the silent outro
