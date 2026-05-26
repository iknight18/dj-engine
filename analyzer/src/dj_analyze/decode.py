"""Audio decode wrapper. Loads any file as mono float32 at 44.1 kHz."""

from __future__ import annotations

from pathlib import Path

import librosa
import numpy as np

TARGET_SAMPLE_RATE = 44100


class DecodeError(Exception):
    """Raised when a file cannot be decoded."""


def decode_audio(path: Path) -> tuple[np.ndarray, int]:
    """Load `path` as mono float32 at TARGET_SAMPLE_RATE.

    Returns (audio, sr).
    Raises DecodeError on any failure (missing file, unsupported format, corrupt data).
    """
    if not path.exists():
        raise DecodeError(f"File does not exist: {path}")
    try:
        audio, sr = librosa.load(str(path), sr=TARGET_SAMPLE_RATE, mono=True)
    except Exception as e:
        raise DecodeError(f"Failed to decode {path}: {e}") from e
    return audio.astype(np.float32, copy=False), int(sr)
