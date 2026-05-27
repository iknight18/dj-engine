from __future__ import annotations

from pathlib import Path

import numpy as np

from dj_analyze.decode import decode_audio
from dj_analyze.detect_energy import EnergyResult, detect_energy

FIXTURES = Path(__file__).parent / "fixtures"


def test_energy_curve_length_matches_duration_in_seconds() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    result = detect_energy(audio, sr)
    curve = result.curve
    # 8s click → curve of ~8 entries (one per second)
    assert 7 <= len(curve) <= 9


def test_energy_curve_values_in_unit_range() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    result = detect_energy(audio, sr)
    curve = result.curve
    assert all(0.0 <= v <= 1.0 for v in curve)


def test_silence_then_beep_curve_has_low_then_high_then_low() -> None:
    audio, sr = decode_audio(FIXTURES / "silence_then_beep.wav")
    result = detect_energy(audio, sr)
    curve = result.curve
    # 10s file: roughly seconds 0-2 silence, 3-6 beep, 7-9 silence.
    assert curve[1] < 0.2          # in the silent intro
    assert curve[5] > 0.4          # in the beep
    assert curve[8] < 0.2          # in the silent outro


def test_detect_energy_returns_energy_result():
    sr = 22050
    audio = np.sin(2 * np.pi * 440 * np.arange(sr * 4) / sr).astype(np.float32) * 0.5
    result = detect_energy(audio, sr)
    assert isinstance(result, EnergyResult)
    assert hasattr(result, "curve")
    assert hasattr(result, "mean")
    assert hasattr(result, "rms_db_curve")
    assert hasattr(result, "max_rms_db")


def test_detect_energy_rms_db_curve_window_count():
    # 4-second audio, 2-second windows -> ~2 windows
    sr = 22050
    audio = np.sin(2 * np.pi * 440 * np.arange(sr * 4) / sr).astype(np.float32) * 0.5
    result = detect_energy(audio, sr)
    # Allow off-by-one due to hop/frame boundaries.
    assert 1 <= len(result.rms_db_curve) <= 3
    assert result.max_rms_db == max(result.rms_db_curve)


def test_detect_energy_silent_audio_dB_safe():
    sr = 22050
    audio = np.zeros(sr * 4, dtype=np.float32)
    result = detect_energy(audio, sr)
    # Silence must not produce NaN or +inf; we cap at a floor.
    assert all(np.isfinite(v) for v in result.rms_db_curve)
    assert np.isfinite(result.max_rms_db)


def test_detect_energy_loud_section_has_higher_db():
    sr = 22050
    quiet = np.sin(2 * np.pi * 440 * np.arange(sr * 2) / sr).astype(np.float32) * 0.05
    loud = np.sin(2 * np.pi * 440 * np.arange(sr * 2) / sr).astype(np.float32) * 0.5
    audio = np.concatenate([quiet, loud])
    result = detect_energy(audio, sr)
    # The loud half should be at least 15 dB above the quiet half.
    assert result.rms_db_curve[-1] - result.rms_db_curve[0] > 15.0
