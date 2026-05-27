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
    assert cues["cue_in_seconds"] == pytest.approx(10.0, abs=0.5)


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
    assert cues["cue_out_seconds"] == pytest.approx(50.0, abs=0.5)


def test_detect_cues_uniform_loudness_uses_full_range():
    """Uniform loudness (all windows above max - 9 dB) -> cue_in=0, cue_out near duration."""
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


def test_intro_end_uses_novelty_peak():
    """Earliest novelty peak in [cue_in+8, cue_in+60] -> intro_end snaps to it.

    Two peaks in window (20s and 40s) verifies we pick the earliest, not the latest.
    """
    dur = 200.0
    novelty_peaks = [20.0, 40.0, 150.0]  # 20 and 40 both in window; 150 outside
    novelty_strengths = [0.8, 0.6, 0.6]
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(400)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 200,
        bpm=120.0,
        novelty_peaks=novelty_peaks,
        novelty_strengths=novelty_strengths,
        novelty_confidence=0.9,
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    # Earliest in-window peak is 20s. Should NOT be 40s.
    assert abs(cues["intro_end_seconds"] - 20.0) <= 2.0


def test_outro_start_uses_last_novelty_peak():
    """Latest novelty peak in [cue_out-60, cue_out-8] -> outro_start snaps to it.

    Two peaks in window (155s and 170s) verifies we pick the latest, not the earliest.
    """
    dur = 200.0
    novelty_peaks = [30.0, 155.0, 170.0]  # 155 and 170 both in window
    novelty_strengths = [0.5, 0.7, 0.8]
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(400)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 200,
        bpm=120.0,
        novelty_peaks=novelty_peaks,
        novelty_strengths=novelty_strengths,
        novelty_confidence=0.9,
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    # Latest in-window peak is 170s. Should NOT be 155s.
    assert abs(cues["outro_start_seconds"] - 170.0) <= 2.0


def test_intro_end_falls_back_when_no_peak_in_window():
    """No peak in [cue_in+8, cue_in+60] -> fall back to cue_in + 16 bars."""
    dur = 200.0
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(400)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 200,
        bpm=120.0,
        novelty_peaks=[5.0, 180.0],  # both outside the [8, 60]s window
        novelty_strengths=[0.8, 0.8],
        novelty_confidence=0.9,
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    # 16 bars at 120 BPM = 32s. cue_in is 0 here, so intro_end ~= 32s.
    assert abs(cues["intro_end_seconds"] - 32.0) <= 2.0


def test_intro_end_falls_back_when_confidence_low():
    """Peaks exist but confidence is below threshold -> fall back to +16 bars."""
    dur = 200.0
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(400)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 200,
        bpm=120.0,
        novelty_peaks=[24.0],
        novelty_strengths=[0.1],
        novelty_confidence=0.1,  # below NOVELTY_CONFIDENCE_THRESHOLD
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    assert abs(cues["intro_end_seconds"] - 32.0) <= 2.0  # fallback


def test_intro_end_respects_min_guard():
    """Peak at 4s (below MIN_INTRO=8) is ignored."""
    dur = 200.0
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(400)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 200,
        bpm=120.0,
        novelty_peaks=[4.0, 40.0],
        novelty_strengths=[0.8, 0.8],
        novelty_confidence=0.9,
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    # First peak 4s ignored (under MIN_INTRO=8). 40s peak used.
    assert abs(cues["intro_end_seconds"] - 40.0) <= 2.0


def test_intro_end_respects_max_guard():
    """Peak at 90s (above MAX_INTRO=60) is ignored; falls back."""
    dur = 200.0
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(400)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 200,
        bpm=120.0,
        novelty_peaks=[90.0],
        novelty_strengths=[0.8],
        novelty_confidence=0.9,
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    # 90s above MAX_INTRO; fall back to +16 bars (~32s).
    assert abs(cues["intro_end_seconds"] - 32.0) <= 2.0


def test_outro_start_falls_back_when_no_peak_in_window():
    """No novelty peak in [cue_out - 60, cue_out - 8] -> fall back to cue_out - 16 bars."""
    dur = 200.0
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(400)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 200,
        bpm=120.0,
        novelty_peaks=[10.0, 195.0],  # both outside outro window [~140, ~192]
        novelty_strengths=[0.8, 0.8],
        novelty_confidence=0.9,
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    # Fallback: cue_out (~200) - 16 bars (32s) = ~168s.
    # cue_out is snapped to last downbeat at 120 BPM = 198s.
    # So expected ~ 198 - 32 = 166s.
    assert abs(cues["outro_start_seconds"] - 166.0) <= 4.0


def test_outro_start_falls_back_when_confidence_low():
    """Peak exists in window but confidence below threshold -> fall back."""
    dur = 200.0
    cues = detect_cues(
        duration_seconds=dur,
        beats=[i * 0.5 for i in range(400)],
        downbeats=_downbeats_for(dur, bpm=120.0),
        energy_curve=[0.0] * 200,
        bpm=120.0,
        novelty_peaks=[170.0],
        novelty_strengths=[0.5],
        novelty_confidence=0.1,  # below NOVELTY_CONFIDENCE_THRESHOLD
        rms_db_curve=_loud_rms_db_curve(dur),
        max_rms_db=-6.0,
    )
    # Same fallback math as above test.
    assert abs(cues["outro_start_seconds"] - 166.0) <= 4.0
