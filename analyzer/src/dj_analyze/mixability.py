"""Classify whether a track is suitable for beatmatched DJ mixing.

Returns split flags:
  - is_beatmatchable: safe to time-stretch + crossfade
  - is_phrase_aligned: downbeats accurate enough to snap to phrase boundaries
  - is_mixable: derived alias of is_beatmatchable (kept for v1 schema compat)
"""

from __future__ import annotations

from typing import Any, Literal

# Relaxed thresholds (was 0.6, 5%; now 0.4 to match the spec).
BPM_CONFIDENCE_THRESHOLD = 0.4
MIN_DURATION_SECONDS = 90.0
KEY_CONFIDENCE_ADVISORY_THRESHOLD = 0.4


def classify(
    *,
    bpm: float,
    bpm_confidence: float,
    duration_seconds: float,
    downbeats_count: int,
    beats_count: int,
    downbeat_method: Literal["madmom", "librosa_phrase", "fallback_4_4"],
    key_confidence: float,
) -> dict[str, Any]:
    """Classify the track. See module docstring for output shape."""
    reasons: list[str] = []

    if bpm_confidence < BPM_CONFIDENCE_THRESHOLD:
        reasons.append("bpm_low_confidence")
    if duration_seconds < MIN_DURATION_SECONDS:
        reasons.append("too_short")
    if key_confidence < KEY_CONFIDENCE_ADVISORY_THRESHOLD:
        reasons.append("key_low_confidence")

    # Phrase alignment requires actual structural data; fallback_4_4 doesn't count.
    is_phrase_aligned = downbeat_method in {"madmom", "librosa_phrase"}
    if not is_phrase_aligned:
        reasons.append("phrase_low_confidence")

    # Beatmatchability is much more permissive than before.
    disqualifying = {"bpm_low_confidence", "too_short"}
    is_beatmatchable = not any(r in disqualifying for r in reasons)

    return {
        "is_beatmatchable": is_beatmatchable,
        "is_phrase_aligned": is_phrase_aligned and is_beatmatchable,
        "is_mixable": is_beatmatchable,  # alias for v1 compat
        "reasons": reasons,
    }
