import math

import pytest

from dj_analyze.detect_cues import detect_cues


def _downbeats_for(duration_s: float, bpm: float = 120.0) -> list[float]:
    beat_period = 60.0 / bpm
    bar_period = beat_period * 4.0
    n = int(duration_s / bar_period)
    return [bar_period * i for i in range(n)]


def _silent_rms_db_curve(duration_s: float, window_s: float = 2.0) -> list[float]:
    n = max(1, int(duration_s / window_s))
    return [-100.0] * n


def _loud_rms_db_curve(duration_s: float, window_s: float = 2.0, db: float = -6.0) -> list[float]:
    n = max(1, int(duration_s / window_s))
    return [db] * n


def test_detect_cues_silent_track_safe_defaults():
    """All-silent track: cue_in=0, cue_out=duration."""
    dur = 60.0
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(120)],
        downbeats=_downbeats_for(dur),
        energy_curve=[0.0] * 60,
        bpm=120.0,
        novelty_peaks=[],
        novelty_strengths=[],
        novelty_confidence=0.0,
        rms_db_curve=_silent_rms_db_curve(dur),
        max_rms_db=-120.0,
    )
    assert cues["cue_in_seconds"] == 0.0
    assert cues["cue_out_seconds"] == pytest.approx(dur, abs=0.01)


def test_detect_cues_loudness_anchor_silent_intro():
    """10s of silence at start, then loud audio.

    Expected: cue_in snaps up to the first downbeat at or after t=10s.
    """
    dur = 60.0
    rms = [-100.0] * 5 + [-6.0] * 25  # 30 windows of 2s = 60s
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(120)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 60,  # unused but required
        bpm=120.0,
        novelty_peaks=[],
        novelty_strengths=[],
        novelty_confidence=0.0,
        rms_db_curve=rms,
        max_rms_db=-6.0,
    )
    # First loud window starts at index 5 -> t=10s. First downbeat >= 10s at 120 BPM
    # (bar period 2.0s) is t=10.0s exactly.
    assert cues["cue_in_seconds"] == pytest.approx(10.0, abs=2.0)


def test_detect_cues_loudness_anchor_silent_outro():
    """Loud for 50s, then 10s silence at end.

    Expected: cue_out snaps down to the last downbeat at or before the silent boundary.
    """
    dur = 60.0
    rms = [-6.0] * 25 + [-100.0] * 5
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(120)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 60,
        bpm=120.0,
        novelty_peaks=[],
        novelty_strengths=[],
        novelty_confidence=0.0,
        rms_db_curve=rms,
        max_rms_db=-6.0,
    )
    # Last loud window ends at t=50s. Last downbeat <= 50s at 120 BPM is t=50.0s.
    assert cues["cue_out_seconds"] == pytest.approx(50.0, abs=2.0)


def test_detect_cues_loudness_below_threshold_yields_no_match():
    """Whole track is more than 9 dB below max -> falls back to no-loud-window behavior."""
    dur = 30.0
    # Every window is exactly at -20 dB; max is -20 dB. -20 - (-20) = 0 >= -9 -> all windows qualify.
    rms = [-20.0] * 15
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(60)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 30,
        bpm=120.0,
        novelty_peaks=[],
        novelty_strengths=[],
        novelty_confidence=0.0,
        rms_db_curve=rms,
        max_rms_db=-20.0,
    )
    # Uniformly loud: cue_in snaps to first downbeat (t=0), cue_out to last (<= duration).
    assert cues["cue_in_seconds"] == pytest.approx(0.0, abs=0.01)
    assert cues["cue_out_seconds"] <= dur + 0.01
