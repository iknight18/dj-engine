"""Per-second RMS energy curve + per-2s RMS-dB curve.

The normalized per-second curve (and its mean) feed the selector and engine.
The dB curve and the track-max dB feed Schwarz's loudness-anchor cue detection.
"""

from __future__ import annotations

from dataclasses import dataclass

import librosa
import numpy as np

_DB_FLOOR = -120.0  # dB floor for safe handling of digital silence
_RMS_DB_WINDOW_S = 2.0


@dataclass(frozen=True, slots=True)
class EnergyResult:
    curve: list[float]          # per-second normalized RMS, in [0, 1]
    mean: float                 # mean of curve
    rms_db_curve: list[float]   # per-2s-window peak RMS, in dB (FS reference: 1.0)
    max_rms_db: float           # max of rms_db_curve, or _DB_FLOOR if all silent


def detect_energy(audio: np.ndarray, sr: int) -> EnergyResult:
    """Return (per-second normalized curve + mean) and (per-2s dB curve + max).

    Per-second curve normalization divides by the 95th percentile to be robust
    to short loud transients. The dB curve is NOT normalized; it carries true
    loudness so downstream rules can threshold on -9 dB-of-max (Schwarz).
    """
    # ---- per-second normalized curve (existing behavior) ----
    sec_hop = sr
    sec_frame = sr
    sec_rms = librosa.feature.rms(y=audio, frame_length=sec_frame, hop_length=sec_hop)
    sec_raw = sec_rms[0].astype(np.float64)
    if len(sec_raw) == 0:
        return EnergyResult(curve=[], mean=0.0, rms_db_curve=[], max_rms_db=_DB_FLOOR)
    p95 = float(np.percentile(sec_raw, 95))
    if p95 <= 0:
        normalized = np.zeros_like(sec_raw)
    else:
        normalized = np.clip(sec_raw / p95, 0.0, 1.0)
    curve = [float(v) for v in normalized]
    mean = float(normalized.mean())

    # ---- per-2s peak-RMS curve in dB (new) ----
    win_frames = int(round(_RMS_DB_WINDOW_S * sr))
    if win_frames < 1 or len(audio) < win_frames:
        return EnergyResult(curve=curve, mean=mean, rms_db_curve=[], max_rms_db=_DB_FLOOR)

    db_rms = librosa.feature.rms(y=audio, frame_length=win_frames, hop_length=win_frames)
    db_raw = db_rms[0].astype(np.float64)
    # Convert to dB with a floor for safe log on zero.
    with np.errstate(divide="ignore"):
        db_vals = 20.0 * np.log10(np.maximum(db_raw, 1e-9))
    db_vals = np.maximum(db_vals, _DB_FLOOR)
    rms_db_curve = [float(v) for v in db_vals]
    max_rms_db = float(np.max(db_vals)) if len(db_vals) else _DB_FLOOR

    return EnergyResult(
        curve=curve,
        mean=mean,
        rms_db_curve=rms_db_curve,
        max_rms_db=max_rms_db,
    )
