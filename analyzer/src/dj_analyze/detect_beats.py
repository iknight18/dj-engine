"""Beat-time detection. Given a known BPM, returns a list of beat onsets (seconds)."""

from __future__ import annotations

import librosa
import numpy as np


def detect_beats(audio: np.ndarray, sr: int, bpm: float) -> list[float]:
    """Return beat times in seconds, given a BPM estimate."""
    _, beat_frames = librosa.beat.beat_track(
        y=audio, sr=sr, start_bpm=bpm, tightness=100, units="frames"
    )
    times: np.ndarray = librosa.frames_to_time(beat_frames, sr=sr)
    return [float(t) for t in times]
