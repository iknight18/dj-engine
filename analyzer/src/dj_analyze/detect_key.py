"""Key detection. Uses essentia when available; falls back to librosa chroma + KS profiles.

All keys are returned in Camelot wheel notation (1A..12A for minor, 1B..12B for major)
so the engine can compute compatibility uniformly.
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np

# Camelot wheel mapping. Pitch class → (minor_code, major_code).
# Pitch classes use sharps; flats are normalized in `to_camelot`.
_PITCH_TO_CAMELOT: dict[str, tuple[str, str]] = {
    "C":  ("5A",  "8B"),
    "C#": ("12A", "3B"),
    "D":  ("7A",  "10B"),
    "D#": ("2A",  "5B"),
    "E":  ("9A",  "12B"),
    "F":  ("4A",  "7B"),
    "F#": ("11A", "2B"),
    "G":  ("6A",  "9B"),
    "G#": ("1A",  "4B"),
    "A":  ("8A",  "11B"),
    "A#": ("3A",  "6B"),
    "B":  ("10A", "1B"),
}

_FLAT_TO_SHARP: dict[str, str] = {
    "Db": "C#", "Eb": "D#", "Gb": "F#", "Ab": "G#", "Bb": "A#",
}


@dataclass(frozen=True, slots=True)
class KeyResult:
    camelot: str
    label: str           # e.g., "A minor"
    confidence: float    # 0..1


def to_camelot(pitch: str, mode: str) -> str:
    """Convert (pitch, mode) like ("A", "minor") to Camelot notation."""
    normalized = _FLAT_TO_SHARP.get(pitch, pitch)
    if normalized not in _PITCH_TO_CAMELOT:
        raise ValueError(f"Unknown pitch: {pitch}")
    minor_code, major_code = _PITCH_TO_CAMELOT[normalized]
    mode_lower = mode.lower()
    if mode_lower.startswith("min"):
        return minor_code
    if mode_lower.startswith("maj"):
        return major_code
    raise ValueError(f"Unknown mode: {mode}")


def detect_key(audio: np.ndarray, sr: int) -> KeyResult:
    """Detect key, returning Camelot code + human label + confidence."""
    try:
        return _essentia_key(audio, sr)
    except Exception:
        return _librosa_fallback_key(audio, sr)


def _essentia_key(audio: np.ndarray, sr: int) -> KeyResult:
    from essentia.standard import KeyExtractor  # type: ignore[import-not-found]

    extractor = KeyExtractor()
    pitch, mode, strength = extractor(audio.astype(np.float32))
    camelot = to_camelot(pitch, mode)
    return KeyResult(
        camelot=camelot,
        label=f"{pitch} {mode}",
        confidence=float(strength),
    )


# Krumhansl-Schmuckler key profiles (major and minor), 12 pitch classes.
_KS_MAJOR = np.array(
    [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88]
)
_KS_MINOR = np.array(
    [6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17]
)
_PITCH_CLASSES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]


def _librosa_fallback_key(audio: np.ndarray, sr: int) -> KeyResult:
    import librosa

    chroma = librosa.feature.chroma_cqt(y=audio, sr=sr)
    profile = chroma.mean(axis=1)
    # Try all 24 keys, pick the highest correlation.
    best_score = -np.inf
    best_pitch = "C"
    best_mode = "major"
    for i in range(12):
        major_score = float(np.corrcoef(profile, np.roll(_KS_MAJOR, i))[0, 1])
        minor_score = float(np.corrcoef(profile, np.roll(_KS_MINOR, i))[0, 1])
        if major_score > best_score:
            best_score = major_score
            best_pitch = _PITCH_CLASSES[i]
            best_mode = "major"
        if minor_score > best_score:
            best_score = minor_score
            best_pitch = _PITCH_CLASSES[i]
            best_mode = "minor"
    # Correlation is in [-1, 1] — map positively-correlated half to [0, 1].
    confidence = max(0.0, best_score)
    return KeyResult(
        camelot=to_camelot(best_pitch, best_mode),
        label=f"{best_pitch} {best_mode}",
        confidence=float(confidence),
    )
