from __future__ import annotations

from pathlib import Path

import numpy as np

from dj_analyze.decode import decode_audio
from dj_analyze.detect_downbeats import (
    DownbeatResult,
    compute_chroma_ssm,
    detect_downbeats,
    detect_phrases,
)

FIXTURES = Path(__file__).parent / "fixtures"


def test_detect_downbeats_method_is_always_madmom():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    result: DownbeatResult = detect_downbeats(audio, sr)
    assert result.method == "madmom"


def test_detect_downbeats_returns_sorted_times():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    result = detect_downbeats(audio, sr)
    assert all(a <= b for a, b in zip(result.downbeats, result.downbeats[1:]))


# detect_phrases / compute_chroma_ssm tests preserved — used independently by analyze.py
# for phrase-boundary detection. These do not depend on downbeat fallback paths.


def test_detect_phrases_on_clean_click_returns_high_confidence_and_boundaries():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = [0.5 * i for i in range(16)]
    result = detect_phrases(audio, sr, beats=beats, output_stride_bars=1)
    assert result.confidence > 0.3
    assert len(result.boundaries) >= 2


def test_detect_phrases_16_bar_stride_yields_fewer_boundaries():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = [0.5 * i for i in range(16)]
    result = detect_phrases(audio, sr, beats=beats)
    assert len(result.boundaries) <= 1


def test_detect_phrases_on_noise_returns_low_confidence():
    rng = np.random.default_rng(seed=42)
    audio = rng.uniform(-0.5, 0.5, size=44100 * 4).astype(np.float32)
    beats = [0.5 * i for i in range(8)]
    result = detect_phrases(audio, sr=44100, beats=beats, output_stride_bars=1)
    assert result.confidence < 0.6


def test_detect_phrases_handles_too_few_beats():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    result = detect_phrases(audio, sr, beats=[0.0, 0.5], output_stride_bars=1)
    assert result.boundaries == []
    assert result.confidence == 0.0


def test_detect_phrases_phrase_bars_legacy_alias_still_works():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    beats = [0.5 * i for i in range(16)]
    via_legacy = detect_phrases(audio, sr, beats=beats, phrase_bars=1)
    via_new = detect_phrases(audio, sr, beats=beats, output_stride_bars=1)
    assert via_legacy.boundaries == via_new.boundaries
    assert via_legacy.confidence == via_new.confidence


def test_compute_chroma_ssm_returns_square_matrix():
    sr = 22050
    seg = sr * 4
    a = np.sin(2 * np.pi * 220 * np.arange(seg) / sr).astype(np.float32) * 0.5
    b = np.sin(2 * np.pi * 330 * np.arange(seg) / sr).astype(np.float32) * 0.5
    audio = np.concatenate([a, b])
    beats = [0.5 * i for i in range(16)]
    ssm = compute_chroma_ssm(audio, sr, beats)
    assert ssm is not None
    assert ssm.ndim == 2
    assert ssm.shape[0] == ssm.shape[1]


def test_compute_chroma_ssm_too_short_returns_none():
    sr = 22050
    audio = np.zeros(100, dtype=np.float32)
    assert compute_chroma_ssm(audio, sr, beats=[]) is None
