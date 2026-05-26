"""Classify whether a track is suitable for beatmatched DJ mixing."""

from __future__ import annotations

from typing import Literal


def classify(
    *,
    bpm: float,
    bpm_confidence: float,
    duration_seconds: float,
    downbeats_count: int,
    beats_count: int,
    downbeat_method: Literal["madmom", "fallback_4_4"],
    key_confidence: float,
) -> tuple[bool, list[str]]:
    """Return (is_mixable, reasons).

    Reasons (any of):
      - "bpm_low_confidence"
      - "bpm_variable"   (reserved; computed in detect_bpm windowing — TODO: thread through)
      - "no_clear_downbeat"
      - "too_short"
      - "key_low_confidence"  (advisory; does NOT disqualify)

    Disqualifying reasons (any → is_mixable=False):
      bpm_low_confidence, no_clear_downbeat, too_short, bpm_variable
    """
    reasons: list[str] = []

    if bpm_confidence < 0.6:
        reasons.append("bpm_low_confidence")
    if duration_seconds < 90.0:
        reasons.append("too_short")
    if downbeat_method == "fallback_4_4":
        reasons.append("no_clear_downbeat")
    if key_confidence < 0.4:
        reasons.append("key_low_confidence")

    disqualifying = {
        "bpm_low_confidence",
        "bpm_variable",
        "no_clear_downbeat",
        "too_short",
    }
    is_mixable = not any(r in disqualifying for r in reasons)
    return is_mixable, reasons
