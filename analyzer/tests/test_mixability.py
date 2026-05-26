from __future__ import annotations

from dj_analyze.mixability import classify


def test_clean_track_is_mixable() -> None:
    is_mixable, reasons = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.8,
    )
    assert is_mixable is True
    assert reasons == []


def test_low_bpm_confidence_disqualifies() -> None:
    is_mixable, reasons = classify(
        bpm=120.0,
        bpm_confidence=0.4,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.8,
    )
    assert is_mixable is False
    assert "bpm_low_confidence" in reasons


def test_too_short_disqualifies() -> None:
    is_mixable, reasons = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=60.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.8,
    )
    assert is_mixable is False
    assert "too_short" in reasons


def test_fallback_downbeats_marks_no_clear_downbeat() -> None:
    is_mixable, reasons = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="fallback_4_4",
        key_confidence=0.8,
    )
    assert is_mixable is False
    assert "no_clear_downbeat" in reasons


def test_low_key_confidence_is_reason_but_not_disqualifying() -> None:
    is_mixable, reasons = classify(
        bpm=120.0,
        bpm_confidence=0.9,
        duration_seconds=200.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="madmom",
        key_confidence=0.2,
    )
    assert is_mixable is True
    assert "key_low_confidence" in reasons


def test_multiple_reasons_accumulate() -> None:
    is_mixable, reasons = classify(
        bpm=120.0,
        bpm_confidence=0.4,
        duration_seconds=60.0,
        downbeats_count=50,
        beats_count=200,
        downbeat_method="fallback_4_4",
        key_confidence=0.8,
    )
    assert is_mixable is False
    assert "bpm_low_confidence" in reasons
    assert "too_short" in reasons
    assert "no_clear_downbeat" in reasons
