"""Per-band (bass / mid / high) RMS energy. Bands match standard DJ EQ split."""

from __future__ import annotations

from typing import Any

import librosa
import numpy as np
from scipy.signal import butter, sosfiltfilt

# Band edges in Hz.
BASS_HIGH = 250.0
MID_LOW = 250.0
MID_HIGH = 2000.0
HIGH_LOW = 2000.0

# Filter order. 4 is a good balance for Butterworth — flat passband, ~24 dB/oct rolloff.
_FILTER_ORDER = 4


def detect_band_energies(audio: np.ndarray, sr: int) -> dict[str, Any]:
    """Return per-band RMS curves (per-second) and means, normalized to [0, 1].

    Bands share a single normalization reference (the maximum 95th-percentile RMS
    across the three bands) so that a band with negligible content yields values
    near zero rather than being amplified by self-normalization. This matches the
    intuition encoded in the unit tests (bass-only fixture → mid/high near zero).

    Output shape:
        {
          "curves": {"bass": [float, ...], "mid": [float, ...], "high": [float, ...]},
          "means":  {"bass": float, "mid": float, "high": float},
        }
    """
    bass = _filter_band(audio, sr, kind="lowpass", cutoff=BASS_HIGH)
    mid = _filter_band(audio, sr, kind="bandpass", low=MID_LOW, high=MID_HIGH)
    high = _filter_band(audio, sr, kind="highpass", cutoff=HIGH_LOW)

    bass_rms = _per_second_rms(bass, sr)
    mid_rms = _per_second_rms(mid, sr)
    high_rms = _per_second_rms(high, sr)

    # Shared normalization reference: the max p95 across all three bands. Using a
    # cross-band reference is what gives a bass-only track high `bass` and low
    # `mid` / `high`. Self-normalizing each band would push residual filter leakage
    # back up to ~1.0 and erase the band-specific signal we care about.
    references = [_p95(r) for r in (bass_rms, mid_rms, high_rms)]
    ref = max(references) if references else 0.0

    bass_curve, bass_mean = _normalize(bass_rms, ref)
    mid_curve, mid_mean = _normalize(mid_rms, ref)
    high_curve, high_mean = _normalize(high_rms, ref)

    return {
        "curves": {"bass": bass_curve, "mid": mid_curve, "high": high_curve},
        "means": {"bass": bass_mean, "mid": mid_mean, "high": high_mean},
    }


def _filter_band(
    audio: np.ndarray,
    sr: int,
    *,
    kind: str,
    cutoff: float | None = None,
    low: float | None = None,
    high: float | None = None,
) -> np.ndarray:
    nyq = sr / 2.0
    if kind == "lowpass":
        sos = butter(_FILTER_ORDER, cutoff / nyq, btype="lowpass", output="sos")
    elif kind == "highpass":
        sos = butter(_FILTER_ORDER, cutoff / nyq, btype="highpass", output="sos")
    elif kind == "bandpass":
        sos = butter(_FILTER_ORDER, [low / nyq, high / nyq], btype="bandpass", output="sos")
    else:
        raise ValueError(f"unknown filter kind: {kind}")
    return sosfiltfilt(sos, audio).astype(np.float32)


def _per_second_rms(audio: np.ndarray, sr: int) -> np.ndarray:
    rms = librosa.feature.rms(y=audio, frame_length=sr, hop_length=sr)[0]
    return rms.astype(np.float64)


def _p95(rms: np.ndarray) -> float:
    if len(rms) == 0:
        return 0.0
    return float(np.percentile(rms, 95))


def _normalize(rms: np.ndarray, ref: float) -> tuple[list[float], float]:
    if len(rms) == 0:
        return [], 0.0
    if ref <= 0.0:
        return [0.0] * len(rms), 0.0
    normalized = np.clip(rms / ref, 0.0, 1.0)
    return [float(v) for v in normalized], float(normalized.mean())
