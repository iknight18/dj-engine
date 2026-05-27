from __future__ import annotations

from pathlib import Path

from dj_analyze.analyze import analyze_track

FIXTURES = Path(__file__).parent / "fixtures"


def test_analyze_returns_dict_with_required_fields() -> None:
    result = analyze_track(FIXTURES / "click_120bpm.wav")
    assert result["schema_version"] == 2
    assert "analyzer_version" in result
    assert result["source_path"] == str((FIXTURES / "click_120bpm.wav").resolve())
    assert "source_mtime" in result
    assert "source_size_bytes" in result
    assert "duration_seconds" in result
    assert "bpm" in result
    assert "beats_seconds" in result
    assert "downbeats_seconds" in result
    assert "key" in result
    assert "energy_curve" in result
    assert "cue_in_seconds" in result
    assert "intro_end_seconds" in result
    assert "is_mixable" in result
    assert "band_energy_curves" in result
    assert "is_beatmatchable" in result
    assert "is_phrase_aligned" in result


def test_analyze_120_bpm_click_detects_bpm_near_120() -> None:
    result = analyze_track(FIXTURES / "click_120bpm.wav")
    assert 118.0 <= result["bpm"] <= 122.0


def test_analyze_corrupt_file_returns_error_sidecar() -> None:
    result = analyze_track(FIXTURES / "corrupt.mp3")
    assert "error" in result
    assert result["schema_version"] == 2
    assert "bpm" not in result


def test_analyze_emits_v2_schema_fields() -> None:
    result = analyze_track(FIXTURES / "click_120bpm.wav")
    assert result["schema_version"] == 2
    assert "band_energy_curves" in result
    assert set(result["band_energy_curves"].keys()) == {"bass", "mid", "high"}
    assert "band_energy_means" in result
    assert "is_beatmatchable" in result
    assert "is_phrase_aligned" in result
    # is_mixable is still emitted as alias.
    assert result["is_mixable"] == result["is_beatmatchable"]


def test_analyze_emits_phrase_boundaries_when_aligned() -> None:
    result = analyze_track(FIXTURES / "click_120bpm.wav")
    # If aligned, boundaries should be present (and could be empty for very short tracks).
    if result.get("is_phrase_aligned"):
        assert "phrase_boundaries_seconds" in result


def test_analyze_emits_cue_fields_consistent_with_novelty():
    """End-to-end orchestration: cue fields are computed by the new code path
    (loudness anchor + novelty) and present in the output dict.
    """
    candidates = sorted(FIXTURES.glob("*.wav"))
    if not candidates:
        import pytest
        pytest.skip("No WAV fixture available for end-to-end check.")
    out = analyze_track(candidates[0])
    # No error from decode.
    assert "error" not in out
    # All four cue fields are present, ordered: cue_in <= intro_end <= outro_start <= cue_out.
    cue_in = out["cue_in_seconds"]
    intro_end = out["intro_end_seconds"]
    outro_start = out["outro_start_seconds"]
    cue_out = out["cue_out_seconds"]
    assert cue_in <= intro_end <= outro_start + 0.01  # tolerate equality at the midpoint clamp
    assert outro_start <= cue_out + 0.01
