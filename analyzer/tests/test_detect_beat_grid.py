from __future__ import annotations

from pathlib import Path

import numpy as np

from dj_analyze.decode import decode_audio
from dj_analyze.detect_beat_grid import BeatGrid, detect_beat_grid

FIXTURES = Path(__file__).parent / "fixtures"


def test_detect_beat_grid_returns_populated_grid_on_click_fixture():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    grid = detect_beat_grid(audio, sr)
    assert isinstance(grid, BeatGrid)
    assert len(grid.beats) > 0
    assert len(grid.downbeats) > 0
    # downbeats are a subset of beats (by time, within numerical tolerance).
    for db in grid.downbeats:
        assert any(abs(db - b) < 1e-3 for b in grid.beats)


def test_detect_beat_grid_length_invariants():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    grid = detect_beat_grid(audio, sr)
    assert len(grid.beat_confidence) == len(grid.beats)
    assert len(grid.downbeat_confidence) == len(grid.downbeats)


def test_detect_beat_grid_confidence_in_unit_interval():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    grid = detect_beat_grid(audio, sr)
    for c in grid.beat_confidence:
        assert 0.0 <= c <= 1.0, f"beat confidence out of range: {c}"
    for c in grid.downbeat_confidence:
        assert 0.0 <= c <= 1.0, f"downbeat confidence out of range: {c}"


def test_detect_beat_grid_beats_are_sorted():
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    grid = detect_beat_grid(audio, sr)
    for a, b in zip(grid.beats, grid.beats[1:]):
        assert a < b


def test_detect_beat_grid_handles_too_short_audio():
    """Audio under one second should not crash; may return empty arrays."""
    rng = np.random.default_rng(seed=42)
    audio = rng.uniform(-0.1, 0.1, size=22050).astype(np.float32)  # 0.5s
    grid = detect_beat_grid(audio, sr=44100)
    assert isinstance(grid, BeatGrid)
    # Don't assert non-empty — madmom may legitimately find nothing in noise.
    assert len(grid.beat_confidence) == len(grid.beats)
    assert len(grid.downbeat_confidence) == len(grid.downbeats)
