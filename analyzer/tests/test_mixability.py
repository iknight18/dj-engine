from __future__ import annotations

from dj_analyze.mixability import classify


def test_clean_track_is_beatmatchable_and_phrase_aligned() -> None:
    result = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.8,
    )
    assert result["is_beatmatchable"] is True
    assert result["is_phrase_aligned"] is True
    assert result["is_mixable"] is True
    assert result["reasons"] == []


def test_librosa_phrase_method_is_phrase_aligned() -> None:
    result = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="librosa_phrase",
        key_confidence=0.8,
    )
    assert result["is_beatmatchable"] is True
    assert result["is_phrase_aligned"] is True


def test_fallback_4_4_is_beatmatchable_but_not_phrase_aligned() -> None:
    """Crucial: the old fallback should NOT disqualify beatmatching."""
    result = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="fallback_4_4",
        key_confidence=0.8,
    )
    assert result["is_beatmatchable"] is True
    assert result["is_phrase_aligned"] is False
    assert "phrase_low_confidence" in result["reasons"]


def test_low_bpm_confidence_below_relaxed_threshold_disqualifies() -> None:
    """Below 0.4 (the new relaxed threshold) means not beatmatchable."""
    result = classify(
        bpm=120.0,
        bpm_confidence=0.3,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.8,
    )
    assert result["is_beatmatchable"] is False
    assert "bpm_low_confidence" in result["reasons"]


def test_bpm_confidence_in_grey_zone_passes() -> None:
    """0.5 used to disqualify (threshold was 0.6); should now pass (threshold is 0.4)."""
    result = classify(
        bpm=120.0,
        bpm_confidence=0.5,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.8,
    )
    assert result["is_beatmatchable"] is True


def test_too_short_disqualifies() -> None:
    result = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=60.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.8,
    )
    assert result["is_beatmatchable"] is False
    assert "too_short" in result["reasons"]


def test_low_key_confidence_is_advisory_not_disqualifying() -> None:
    result = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.2,
    )
    assert result["is_beatmatchable"] is True
    assert "key_low_confidence" in result["reasons"]
