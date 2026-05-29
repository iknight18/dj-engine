from __future__ import annotations

from dj_analyze.analyze import compute_local_bpm


def test_compute_local_bpm_uniform_120():
    """Four bars of 120 BPM: each bar has 4 beats at 0.5s spacing."""
    downbeats = [0.0, 2.0, 4.0, 6.0, 8.0]  # 4 bars
    beats = [0.5 * i for i in range(17)]  # 0.0, 0.5, ..., 8.0
    result = compute_local_bpm(beats=beats, downbeats=downbeats)
    assert len(result) == 4  # one entry per bar
    for bpm in result:
        assert bpm is not None
        assert abs(bpm - 120.0) < 0.5


def test_compute_local_bpm_underdetermined_bar_returns_none():
    """A bar with fewer than 2 beats yields None."""
    downbeats = [0.0, 2.0, 4.0]
    # Bar 0 has 4 beats (in [0.0, 2.0]); bar 1 has only 1 beat (in [2.0, 4.0]).
    beats = [0.0, 0.5, 1.0, 1.5, 3.0]
    result = compute_local_bpm(beats=beats, downbeats=downbeats)
    assert len(result) == 2
    assert result[0] is not None
    assert result[1] is None


def test_compute_local_bpm_empty_inputs():
    assert compute_local_bpm(beats=[], downbeats=[]) == []
    assert compute_local_bpm(beats=[0.0, 0.5], downbeats=[]) == []
    assert compute_local_bpm(beats=[], downbeats=[0.0, 2.0]) == [None]


def test_compute_local_bpm_drift_within_bar():
    """A bar whose IBIs are 0.5s/0.5s/0.5s vs another whose IBIs vary."""
    downbeats = [0.0, 2.0, 4.0]
    # Bar 0: 120 BPM clean. Bar 1: slowing down (140 -> 120 -> 100 BPM equivalents).
    beats = [
        0.0, 0.5, 1.0, 1.5,
        2.0, 2.43, 2.93, 3.53, 4.0,  # IBIs 0.43, 0.5, 0.6, 0.47 - median ~0.485 -> ~123 BPM
    ]
    result = compute_local_bpm(beats=beats, downbeats=downbeats)
    assert result[0] is not None
    assert abs(result[0] - 120.0) < 0.5
    assert result[1] is not None
    # Median of [0.43, 0.5, 0.6, 0.47] = 0.485 -> 60/0.485 ~ 123.7
    assert 120.0 < result[1] < 128.0
