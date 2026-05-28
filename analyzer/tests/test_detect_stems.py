from __future__ import annotations

import numpy as np
import pytest

# These imports will fail until detect_stems.py exists — that's the TDD RED state.
from dj_analyze.detect_stems import (
    _per_bar_density,
    _vocal_segments_from_density,
    _VOCAL_PRESENT_THRESHOLD,
)


def test_per_bar_density_normalizes_to_unit_max() -> None:
    # 4 stems x 2 channels x 4*2 seconds @ sr=4
    sr = 4
    bars = [0.0, 2.0, 4.0, 6.0, 8.0]  # 4 bars of 2 seconds each
    # Build a fake sources array: stem 0 ("drums") is silent in bars 0,2; loud in bar 1; medium in bar 3.
    drums_mono = np.zeros(32, dtype=np.float32)
    drums_mono[8:16] = 1.0          # bar 1: amplitude 1
    drums_mono[24:32] = 0.5         # bar 3: amplitude 0.5
    drums = np.stack([drums_mono, drums_mono])  # stereo
    silence = np.zeros_like(drums)
    sources = np.stack([drums, silence, silence, silence])  # (4, 2, 32)

    density = _per_bar_density(sources, ["drums", "bass", "other", "vocals"], "drums", bars, sr=sr)
    # Loudest bar (1) normalized to 1.0; bar 3 should be ~0.5.
    assert len(density) == 4
    assert density[0] == pytest.approx(0.0, abs=1e-6)
    assert density[1] == pytest.approx(1.0, abs=1e-6)
    assert density[2] == pytest.approx(0.0, abs=1e-6)
    assert density[3] == pytest.approx(0.5, abs=1e-6)


def test_per_bar_density_returns_empty_for_unknown_stem() -> None:
    sources = np.zeros((4, 2, 16), dtype=np.float32)
    density = _per_bar_density(
        sources, ["drums", "bass", "other", "vocals"], "doesnotexist", [0.0, 1.0, 2.0], sr=4
    )
    assert density == []


def test_per_bar_density_all_silent() -> None:
    sources = np.zeros((4, 2, 16), dtype=np.float32)
    density = _per_bar_density(
        sources, ["drums", "bass", "other", "vocals"], "drums", [0.0, 1.0, 2.0, 3.0, 4.0], sr=4
    )
    assert density == [0.0, 0.0, 0.0, 0.0]


def test_vocal_segments_merges_contiguous_above_threshold() -> None:
    # 7 bars at 2-second boundaries → bar_edges = [0, 2, 4, 6, 8, 10, 12, 14]
    bar_edges = [0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0]
    # Densities: silent, present, present, silent, silent, present, silent
    densities = [0.0, 0.5, 0.5, 0.0, 0.0, 0.4, 0.0]
    segments = _vocal_segments_from_density(densities, bar_edges)
    # Expect (2.0, 6.0) for bars 1-2, and (10.0, 12.0) for bar 5.
    assert segments == [(2.0, 6.0), (10.0, 12.0)]


def test_vocal_segments_returns_empty_for_silence() -> None:
    bar_edges = [0.0, 2.0, 4.0]
    densities = [0.0, 0.0]
    assert _vocal_segments_from_density(densities, bar_edges) == []


def test_vocal_segments_handles_track_ending_with_vocals() -> None:
    bar_edges = [0.0, 2.0, 4.0, 6.0]
    densities = [0.0, 0.5, 0.5]
    segments = _vocal_segments_from_density(densities, bar_edges)
    assert segments == [(2.0, 6.0)]


def test_vocal_segments_handles_track_starting_with_vocals() -> None:
    bar_edges = [0.0, 2.0, 4.0]
    densities = [0.5, 0.0]
    segments = _vocal_segments_from_density(densities, bar_edges)
    assert segments == [(0.0, 2.0)]


def test_vocal_segments_below_threshold_doesnt_count() -> None:
    bar_edges = [0.0, 2.0, 4.0]
    # Densities just below threshold → no segments.
    densities = [_VOCAL_PRESENT_THRESHOLD - 0.001, _VOCAL_PRESENT_THRESHOLD - 0.001]
    assert _vocal_segments_from_density(densities, bar_edges) == []
