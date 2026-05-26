from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_cues import detect_cues

FIXTURES = Path(__file__).parent / "fixtures"


def test_cues_on_silence_then_beep() -> None:
    audio, sr = decode_audio(FIXTURES / "silence_then_beep.wav")
    duration = len(audio) / sr
    # Synthetic downbeats every 2s (close to 120 BPM in 4/4)
    downbeats = [0.0, 2.0, 4.0, 6.0, 8.0]
    energy_curve = [0.0, 0.0, 0.0, 0.6, 0.6, 0.6, 0.6, 0.0, 0.0, 0.0]
    cues = detect_cues(
        duration_seconds=duration,
        beats=[0.5 * i for i in range(20)],
        downbeats=downbeats,
        energy_curve=energy_curve,
        bpm=120.0,
    )
    # cue_in should be at the first downbeat where energy is meaningful (≥3s)
    assert 2.0 <= cues["cue_in_seconds"] <= 4.0
    # cue_out should be at the last downbeat where energy is meaningful (≤7s)
    assert 6.0 <= cues["cue_out_seconds"] <= 8.0


def test_short_track_clamps_intro_and_outro() -> None:
    cues = detect_cues(
        duration_seconds=10.0,
        beats=[0.5 * i for i in range(20)],
        downbeats=[0.0, 2.0, 4.0, 6.0, 8.0],
        energy_curve=[0.5] * 10,
        bpm=120.0,
    )
    # With bpm=120 a bar is 2s; 16 bars = 32s but track is 10s, so intro_end
    # should be clamped sensibly (no later than cue_out).
    assert cues["intro_end_seconds"] <= cues["cue_out_seconds"]
    assert cues["outro_start_seconds"] >= cues["cue_in_seconds"]
    assert cues["intro_end_seconds"] <= cues["outro_start_seconds"]


def test_silent_track_uses_endpoints() -> None:
    """Fully silent energy curve → cue_in=0, cue_out=duration."""
    duration = 8.0
    cues = detect_cues(
        duration_seconds=duration,
        beats=[0.5 * i for i in range(16)],
        downbeats=[0.0, 2.0, 4.0, 6.0],
        energy_curve=[0.0] * 8,
        bpm=120.0,
    )
    assert cues["cue_in_seconds"] == 0.0
    assert cues["cue_out_seconds"] == duration
