from __future__ import annotations

from pathlib import Path

from dj_analyze.analyze import analyze_track

FIXTURES = Path(__file__).parent / "fixtures"


def test_analyze_returns_dict_with_required_fields() -> None:
    result = analyze_track(FIXTURES / "click_120bpm.wav")
    assert result["schema_version"] == 1
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


def test_analyze_120_bpm_click_detects_bpm_near_120() -> None:
    result = analyze_track(FIXTURES / "click_120bpm.wav")
    assert 118.0 <= result["bpm"] <= 122.0


def test_analyze_corrupt_file_returns_error_sidecar() -> None:
    result = analyze_track(FIXTURES / "corrupt.mp3")
    assert "error" in result
    assert result["schema_version"] == 1
    assert "bpm" not in result
