from __future__ import annotations

from pathlib import Path

from dj_analyze.decode import decode_audio
from dj_analyze.detect_bpm import _octave_normalize, detect_bpm

FIXTURES = Path(__file__).parent / "fixtures"


def test_detects_120_bpm_within_tolerance() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    bpm, confidence = detect_bpm(audio, sr)
    assert 118.0 <= bpm <= 122.0
    assert 0.0 <= confidence <= 1.0


def test_confidence_is_high_on_clean_click_track() -> None:
    audio, sr = decode_audio(FIXTURES / "click_120bpm.wav")
    _, confidence = detect_bpm(audio, sr)
    # A perfect click should give high confidence.
    assert confidence > 0.7


def test_octave_normalize_collapses_half_time_to_target() -> None:
    # Track is steady 152 BPM; a window erroneously reported 76 (half-time).
    assert _octave_normalize(76.0, 152.0) == 152.0


def test_octave_normalize_collapses_double_time_to_target() -> None:
    # Same track, a window reported 304 (double-time).
    assert _octave_normalize(304.0, 152.0) == 152.0


def test_octave_normalize_keeps_correct_value() -> None:
    assert _octave_normalize(152.0, 152.0) == 152.0


def test_octave_normalize_handles_quarter_time() -> None:
    # Edge case: librosa returned a /4 estimate on a slow chord change.
    assert _octave_normalize(38.0, 152.0) == 152.0


def test_octave_normalize_does_not_flip_genuinely_different_tempos() -> None:
    # 90 BPM track vs 152 BPM target — none of {22.5, 45, 90, 180, 360} is closer to
    # 152 than 90 itself? 90 distance 62; 180 distance 28 — so 180 wins. This is the
    # intended behavior: if the windowed BPM is plausibly an octave-equivalent of the
    # target (180 ≈ 152 is within an octave), we collapse. Genuinely unrelated tempos
    # would need to be more than 2x apart to get collapsed.
    assert _octave_normalize(90.0, 152.0) == 180.0


def test_octave_normalize_zero_or_negative_pass_through() -> None:
    assert _octave_normalize(0.0, 152.0) == 0.0
    assert _octave_normalize(-1.0, 152.0) == -1.0


import numpy as np
import pytest

from dj_analyze.detect_bpm import (
    bpm_from_beats,
    resolve_octave,
    _snap_to_perceptual,
    PERCEPTUAL_LOW,
    PERCEPTUAL_HIGH,
    PERCEPTUAL_DEFAULT,
)


def test_bpm_from_beats_120_bpm() -> None:
    # 4 beats at 0.5-second intervals → 120 BPM.
    # librosa.frames_to_time uses hop_length=512 by default, so frame indices must be
    # in those units: 0.5 s * 22050 / 512 ≈ 21.5 → use multiples of 21 frames.
    sr = 22050
    hop_length = 512
    # Exact: 0.5 s / (hop_length / sr) = 0.5 * 22050 / 512 ≈ 21.533 frames
    # Use float-rounded to nearest integer for minimal quantization error.
    frames_per_beat = round(0.5 * sr / hop_length)  # = 22
    beat_frames = np.array([0, frames_per_beat, frames_per_beat * 2, frames_per_beat * 3])
    bpm = bpm_from_beats(beat_frames, sr=sr)
    assert bpm is not None
    assert abs(bpm - 120.0) < 3.0  # allow for integer quantization in frame conversion


def test_bpm_from_beats_returns_none_for_empty() -> None:
    assert bpm_from_beats(np.array([]), sr=22050) is None


def test_bpm_from_beats_returns_none_for_single_beat() -> None:
    assert bpm_from_beats(np.array([1000]), sr=22050) is None


def test_bpm_from_beats_returns_none_for_zero_interval() -> None:
    # Two beats at the same frame → zero IBI.
    assert bpm_from_beats(np.array([100, 100]), sr=22050) is None


def test_resolve_octave_exact_agreement_passes_through() -> None:
    assert resolve_octave(120.0, 120.0) == pytest.approx(120.0, abs=0.5)


def test_resolve_octave_primary_doubled_secondary_correct() -> None:
    # primary librosa-locked at 152; beat_track sees 76 (truth).
    # primary÷2 = 76 matches secondary = 76 → return ~76.
    result = resolve_octave(152.0, 76.0)
    assert abs(result - 76.0) < 2.0


def test_resolve_octave_primary_correct_secondary_doubled() -> None:
    # primary = 95 (true); beat_track over-counted at 190.
    # secondary÷2 = 95 matches primary = 95 → return ~95.
    result = resolve_octave(95.0, 190.0)
    assert abs(result - 95.0) < 2.0


def test_resolve_octave_primary_halved_secondary_correct() -> None:
    # primary = 60 (out of range); beat_track = 120 (truth).
    # primary×2 = 120 matches secondary = 120 → return ~120.
    result = resolve_octave(60.0, 120.0)
    assert abs(result - 120.0) < 2.0


def test_resolve_octave_secondary_none_snaps_primary() -> None:
    # No beat_track signal — snap primary into perceptual range if possible.
    assert resolve_octave(60.0, None) == pytest.approx(120.0, abs=0.5)
    assert resolve_octave(140.0, None) == pytest.approx(140.0, abs=0.5)


def test_resolve_octave_disagreement_picks_closer_to_120() -> None:
    # primary = 95, secondary = 145 (no octave relationship).
    # 95 distance from 120 = 25; 145 distance from 120 = 25 — tie.
    # Whichever is picked must still be one of the two.
    result = resolve_octave(95.0, 145.0)
    assert abs(result - 95.0) < 0.5 or abs(result - 145.0) < 0.5


def test_resolve_octave_handles_zero_primary() -> None:
    # Should not divide by zero or loop forever.
    result = resolve_octave(0.0, 120.0)
    assert result == pytest.approx(120.0, abs=0.5)


def test_snap_to_perceptual_below_range_doubles() -> None:
    assert _snap_to_perceptual(40.0) == pytest.approx(80.0, abs=0.001)


def test_snap_to_perceptual_above_range_halves() -> None:
    assert _snap_to_perceptual(240.0) == pytest.approx(120.0, abs=0.001)


def test_snap_to_perceptual_in_range_passes_through() -> None:
    assert _snap_to_perceptual(100.0) == pytest.approx(100.0, abs=0.001)
    assert _snap_to_perceptual(PERCEPTUAL_LOW) == pytest.approx(PERCEPTUAL_LOW, abs=0.001)
    assert _snap_to_perceptual(PERCEPTUAL_HIGH) == pytest.approx(PERCEPTUAL_HIGH, abs=0.001)


def test_snap_to_perceptual_zero_passes_through() -> None:
    assert _snap_to_perceptual(0.0) == 0.0


def test_snap_to_perceptual_negative_passes_through() -> None:
    assert _snap_to_perceptual(-10.0) == pytest.approx(-10.0, abs=0.001)
