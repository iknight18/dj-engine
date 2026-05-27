import numpy as np
import pytest

from dj_analyze.detect_downbeats import compute_chroma_ssm
from dj_analyze.detect_novelty import NoveltyResult, detect_novelty


def _two_section_audio(sr: int, dur_s: float, freq1: float, freq2: float) -> np.ndarray:
    seg = int(sr * dur_s / 2)
    t1 = np.arange(seg) / sr
    t2 = np.arange(seg) / sr
    a = (np.sin(2 * np.pi * freq1 * t1) * 0.5).astype(np.float32)
    b = (np.sin(2 * np.pi * freq2 * t2) * 0.5).astype(np.float32)
    return np.concatenate([a, b])


def _beats_for(dur_s: float, bpm: float = 120.0) -> list[float]:
    period = 60.0 / bpm
    n = int(dur_s / period)
    return [period * i for i in range(n)]


def test_detect_novelty_two_section_track_has_peak_near_boundary():
    sr = 22050
    dur_s = 32.0
    audio = _two_section_audio(sr, dur_s, freq1=220.0, freq2=440.0)
    beats = _beats_for(dur_s)
    result = detect_novelty(audio, sr, beats=beats)
    assert isinstance(result, NoveltyResult)
    assert len(result.peaks_seconds) >= 1
    # Boundary is at 16 s. Expect a peak within +/- 2 s.
    closest = min(result.peaks_seconds, key=lambda t: abs(t - 16.0))
    assert abs(closest - 16.0) <= 2.0
    assert result.confidence > 0.0


def test_detect_novelty_uniform_track_has_low_confidence():
    sr = 22050
    dur_s = 30.0
    audio = (np.sin(2 * np.pi * 440 * np.arange(int(sr * dur_s)) / sr) * 0.5).astype(np.float32)
    beats = _beats_for(dur_s)
    result = detect_novelty(audio, sr, beats=beats)
    # No real structural change -> confidence should be low.
    assert result.confidence < 0.5


def test_detect_novelty_drum_drop_detected():
    sr = 22050
    dur_s = 32.0
    # First half: pure sine. Second half: same sine + click on every 4 beats.
    half = int(sr * dur_s / 2)
    t = np.arange(half) / sr
    sine = np.sin(2 * np.pi * 220 * t) * 0.3
    sine2 = np.sin(2 * np.pi * 220 * (np.arange(half) / sr + dur_s / 2)) * 0.3
    # Add clicks at every 0.5s in the second half.
    clicks = np.zeros(half, dtype=np.float32)
    for k in range(int(dur_s / 2 / 0.5)):
        idx = int(k * 0.5 * sr)
        if idx < half:
            clicks[idx : idx + 50] = 0.5  # short impulse
    audio = np.concatenate([sine.astype(np.float32), (sine2 + clicks).astype(np.float32)])
    beats = _beats_for(dur_s)
    result = detect_novelty(audio, sr, beats=beats)
    # Expect at least one peak near the drum-entry boundary at 16 s.
    assert len(result.peaks_seconds) >= 1
    closest = min(result.peaks_seconds, key=lambda t: abs(t - 16.0))
    assert abs(closest - 16.0) <= 3.0


def test_detect_novelty_accepts_precomputed_ssm():
    sr = 22050
    dur_s = 32.0
    audio = _two_section_audio(sr, dur_s, freq1=220.0, freq2=440.0)
    beats = _beats_for(dur_s)
    ssm = compute_chroma_ssm(audio, sr, beats)
    r1 = detect_novelty(audio, sr, beats=beats)
    r2 = detect_novelty(audio, sr, beats=beats, chroma_ssm=ssm)
    assert r1.peaks_seconds == pytest.approx(r2.peaks_seconds, abs=0.01)
    assert abs(r1.confidence - r2.confidence) < 1e-6


def test_detect_novelty_empty_beats_returns_empty():
    sr = 22050
    audio = np.zeros(sr * 5, dtype=np.float32)
    result = detect_novelty(audio, sr, beats=[])
    assert result.peaks_seconds == []
    assert result.confidence == 0.0
